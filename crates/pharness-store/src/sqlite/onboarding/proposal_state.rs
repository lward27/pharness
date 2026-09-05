use serde_json::{json, Value};

/// API/runhost validate the full DTO. Storage independently prevents approval from
/// erasing unresolved statements, including records written by older readers.
pub(super) fn blockers(document: &Value) -> Vec<Value> {
    let mut result = Vec::new();
    for (field, code) in [
        ("blockers", "proposal_blocker"),
        ("conflicts", "proposal_conflict"),
    ] {
        match document.get(field) {
            Some(Value::Array(values)) => result.extend(values.iter().map(|value| {
                json!({"code":code,"summary":value.as_str().unwrap_or("invalid proposal statement")})
            })),
            Some(_) => result.push(json!({"code":"proposal_invalid","summary":format!("{field} must be an array")})),
            None => (),
        }
    }
    if !document
        .get("candidate_contract")
        .is_some_and(Value::is_object)
    {
        result.push(json!({"code":"candidate_contract_missing","summary":"No executable candidate contract was proposed"}));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_conflicts_and_absence_including_historical_documents() {
        let blocked = blockers(
            &json!({"schema_version":"v1alpha1","candidate_contract":{},"blockers":["lock missing"],"conflicts":["alias differs"]}),
        );
        assert_eq!(blocked.len(), 2);
        assert_eq!(blocked[0]["summary"], "lock missing");
        assert_eq!(blocked[1]["summary"], "alias differs");
        for invalid in [
            json!({}),
            json!({"candidate_contract":null}),
            json!({"candidate_contract":{},"blockers":null}),
        ] {
            assert!(!blockers(&invalid).is_empty());
        }
        assert!(
            blockers(&json!({"candidate_contract":{},"blockers":[],"conflicts":[]})).is_empty()
        );
    }
}
