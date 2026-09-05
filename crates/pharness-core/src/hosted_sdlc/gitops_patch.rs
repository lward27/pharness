//! Existing digest-only GitOps edit shared by the controller and isolated worker.
use serde_yaml::Value as YamlValue;

/// Update one declared Kustomize image entry to an immutable image digest.
///
/// This supports only the standard `images` list and requires exactly one
/// matching `name`. A GitOps writer stops for review rather than guessing
/// among aliases or rewriting an arbitrary manifest.
pub fn update_kustomization_image(
    source: &str,
    image_name: &str,
    image_ref: &str,
) -> Result<String, String> {
    validate_kustomization_image_name(image_name)?;
    let (desired_reference, digest) = parse_digest_pinned_image_reference(image_ref)?;
    let desired_repository = image_repository_without_optional_tag(&desired_reference)?;
    if desired_repository != image_name {
        return Err("kustomization image repository does not match the declared image name".into());
    }
    let document: YamlValue = serde_yaml::from_str(source)
        .map_err(|_| "kustomization document is not valid YAML".to_string())?;
    let root = document
        .as_mapping()
        .ok_or_else(|| "kustomization document must be a YAML mapping".to_string())?;
    let images_key = YamlValue::String("images".to_string());
    let images = root
        .get(&images_key)
        .and_then(YamlValue::as_sequence)
        .ok_or_else(|| "kustomization document must contain an images sequence".to_string())?;

    let matching = images
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            entry
                .as_mapping()
                .and_then(|mapping| mapping.get(YamlValue::String("name".to_string())))
                .and_then(YamlValue::as_str)
                .filter(|name| *name == image_name)
                .map(|_| index)
        })
        .collect::<Vec<_>>();
    let index = match matching.as_slice() {
        [index] => *index,
        [] => return Err("kustomization image entry was not found".into()),
        _ => return Err("kustomization image entry is ambiguous".into()),
    };
    let entry = images[index]
        .as_mapping()
        .ok_or_else(|| "kustomization image entry must be a mapping".to_string())?;
    if entry
        .get(YamlValue::String("newName".to_string()))
        .and_then(YamlValue::as_str)
        .is_some_and(|new_name| new_name != image_name)
    {
        return Err("kustomization image newName does not match the declared image name".into());
    }
    if entry.contains_key(YamlValue::String("newTag".to_string())) {
        return Err("kustomization image entry must not contain newTag".into());
    }
    let existing_digest = entry
        .get(YamlValue::String("digest".to_string()))
        .and_then(YamlValue::as_str)
        .filter(|value| valid_sha256_digest(value))
        .ok_or_else(|| {
            "kustomization image entry must already contain an immutable sha256 digest".to_string()
        })?;
    let (digest_start, digest_end) =
        standard_kustomization_digest_span(source, image_name, existing_digest)?;

    // Preserve the reviewed Kustomization byte-for-byte except for the one
    // immutable digest scalar. Re-serializing YAML creates unrelated formatting
    // changes, and copying the convenience tag into `newName` broadens the
    // approved GitOps mutation beyond a digest-only promotion.
    let mut updated = String::with_capacity(source.len());
    updated.push_str(&source[..digest_start]);
    updated.push_str(&digest);
    updated.push_str(&source[digest_end..]);
    Ok(updated)
}

fn standard_kustomization_digest_span(
    source: &str,
    image_name: &str,
    existing_digest: &str,
) -> Result<(usize, usize), String> {
    let mut lines = Vec::new();
    let mut offset = 0;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let trimmed = line.trim_start_matches(' ');
        if trimmed.starts_with('\t') {
            return Err("kustomization image entry must use standard space indentation".into());
        }
        lines.push((offset, line, line.len() - trimmed.len(), trimmed));
        offset += raw_line.len();
    }
    if source.is_empty() {
        return Err("kustomization image entry was not found in standard YAML form".into());
    }

    let matching_entries = lines
        .iter()
        .enumerate()
        .filter_map(|(index, (_, _, indent, trimmed))| {
            trimmed
                .strip_prefix("- ")
                .and_then(|value| value.strip_prefix("name:"))
                .map(str::trim)
                .filter(|value| *value == image_name)
                .map(|_| (index, *indent))
        })
        .collect::<Vec<_>>();
    let (entry_index, entry_indent) = match matching_entries.as_slice() {
        [(index, indent)] => (*index, *indent),
        [] => return Err("kustomization image entry was not found in standard YAML form".into()),
        _ => return Err("kustomization image entry is ambiguous in source text".into()),
    };

    let mut digest_spans = Vec::new();
    for (line_offset, line, indent, trimmed) in lines.iter().skip(entry_index + 1) {
        if !trimmed.is_empty()
            && (*indent < entry_indent || (*indent == entry_indent && trimmed.starts_with("- ")))
        {
            break;
        }
        let Some(value) = trimmed.strip_prefix("digest:") else {
            continue;
        };
        if *indent <= entry_indent || !value.contains(existing_digest) {
            continue;
        }
        let local_start = line
            .find(existing_digest)
            .expect("digest presence checked above");
        digest_spans.push((
            line_offset + local_start,
            line_offset + local_start + existing_digest.len(),
        ));
    }
    match digest_spans.as_slice() {
        [span] => Ok(*span),
        [] => Err("kustomization image digest was not found in standard YAML form".into()),
        _ => Err("kustomization image digest occurrence is ambiguous".into()),
    }
}

pub fn validate_kustomization_image_name(image_name: &str) -> Result<(), String> {
    if image_name.trim().is_empty()
        || image_name != image_name.trim()
        || image_name.contains(['\0', '\n', '\r', '@'])
    {
        return Err("invalid kustomization image name".into());
    }
    Ok(())
}

pub fn parse_digest_pinned_image_reference(image_ref: &str) -> Result<(String, String), String> {
    let (repository, digest) = image_ref
        .split_once('@')
        .ok_or_else(|| "image reference must be digest pinned".to_string())?;
    if repository.trim().is_empty()
        || repository != repository.trim()
        || repository.contains(['\0', '\n', '\r', '@'])
        || !digest.starts_with("sha256:")
        || digest.len() != "sha256:".len() + 64
        || !digest["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("image reference must contain a valid sha256 digest".into());
    }
    Ok((repository.to_string(), digest.to_string()))
}

fn image_repository_without_optional_tag(reference: &str) -> Result<&str, String> {
    let last_slash = reference.rfind('/');
    let last_colon = reference.rfind(':');
    if last_colon.is_some_and(|colon| match last_slash {
        Some(slash) => colon > slash,
        None => true,
    }) {
        let colon = last_colon.expect("checked above");
        if colon + 1 == reference.len() {
            return Err("image reference contains an empty tag".into());
        }
        Ok(&reference[..colon])
    } else {
        Ok(reference)
    }
}

fn valid_sha256_digest(value: &str) -> bool {
    value.starts_with("sha256:")
        && value.len() == "sha256:".len() + 64
        && value["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}
