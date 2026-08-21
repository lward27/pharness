pub(in crate::app) fn truncate_audit_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}...[truncated]", &value[..end])
}

pub(in crate::app) fn compact_delivery_subject(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let subject = if compact.is_empty() {
        "Pharness ChangeSet".to_string()
    } else {
        compact
    };
    subject.chars().take(72).collect()
}
