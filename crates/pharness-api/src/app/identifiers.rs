pub(in crate::app) fn safe_id_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub(in crate::app) fn is_git_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(in crate::app) fn is_sha256_digest(value: &str) -> bool {
    value.len() == "sha256:".len() + 64
        && value.starts_with("sha256:")
        && value["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

pub(in crate::app) fn is_github_pr_url(value: &str) -> bool {
    let parts = value
        .strip_prefix("https://github.com/")
        .map(|value| value.split('/').collect::<Vec<_>>());
    matches!(parts, Some(parts) if parts.len() == 4 && parts[2] == "pull" && parts[3].parse::<u64>().is_ok())
}
