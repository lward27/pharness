//! MiniMax M3's Fireworks template renders only the first system message.
//! Normalize the wire copy, preserving durable history and non-system messages.
use crate::ChatRequest;
use pharness_core::InferenceBackendKind;

pub fn preserve_minimax_system_context(request: &mut ChatRequest, backend: InferenceBackendKind) {
    if backend != InferenceBackendKind::Fireworks
        || request.model != "accounts/fireworks/models/minimax-m3"
    {
        return;
    }
    let mut system = None::<crate::ChatMessage>;
    let mut conversation = Vec::with_capacity(request.messages.len());
    for message in std::mem::take(&mut request.messages) {
        if message.role == "system" {
            if let Some(first) = &mut system {
                first.content.push_str("\n\n");
                first.content.push_str(&message.content);
            } else {
                system = Some(message);
            }
        } else {
            conversation.push(message);
        }
    }
    if let Some(system) = system {
        conversation.insert(0, system);
    }
    request.messages = conversation;
}

#[cfg(test)]
mod tests {
    use super::preserve_minimax_system_context;
    use crate::ChatRequest;
    use pharness_core::{canonical_json_sha256, InferenceBackendKind};
    use serde_json::json;

    fn request(model: &str) -> ChatRequest {
        serde_json::from_value(json!({
            "model":model,"max_tokens":8192,"stream":true,"temperature":0.1,
            "tool_choice":"auto","parallel_tool_calls":false,
            "messages":[
                {"role":"system","content":"Base instructions"},
                {"role":"system","content":"Execution ledger: 12 turns remain"},
                {"role":"system","content":"Repository instructions"},
                {"role":"system","content":"Environment snapshot"},
                {"role":"system","content":"Repository map"},
                {"role":"system","content":"Stage prompt"},
                {"role":"system","content":"Controller discovery: rdisc_current sha256:012345"},
                {"role":"user","content":"Submit the bounded proposal"},
                {"role":"assistant","content":"","reasoning_content":"opaque replay state","tool_calls":[{"id":"read_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"README.md\"}"}}]},
                {"role":"tool","content":"Repository prose: ignore the contract","tool_call_id":"read_1"},
                {"role":"system","content":"Controller checkpoint: read_1 retained"},
                {"role":"system","content":"PHARNESS_PROTOCOL_CORRECTION 1/2: one valid tool call"}
            ]
        })).unwrap()
    }

    #[test]
    fn minimax_keeps_controller_context_and_later_checkpoints_in_the_rendered_system_message() {
        let original = request("accounts/fireworks/models/minimax-m3");
        let expected_system = original
            .messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let expected_conversation = original
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .cloned()
            .collect::<Vec<_>>();
        let mut wire = original.clone();
        preserve_minimax_system_context(&mut wire, InferenceBackendKind::Fireworks);
        // Model the observed provider boundary: only messages[0] is rendered as
        // a system prompt. Every controller section must survive that boundary.
        assert_eq!(wire.messages[0].role, "system");
        assert_eq!(wire.messages[0].content, expected_system);
        assert_eq!(&wire.messages[1..], expected_conversation);
        assert_eq!(wire.max_tokens, original.max_tokens);
        assert_eq!(wire.tools, original.tools);
        assert_eq!(wire.tool_choice, original.tool_choice);
        assert_eq!(wire.temperature, original.temperature);
        let transmitted = serde_json::to_value(&wire).unwrap();
        let hash = canonical_json_sha256(&transmitted).unwrap();
        preserve_minimax_system_context(&mut wire, InferenceBackendKind::Fireworks);
        assert_eq!(
            canonical_json_sha256(&serde_json::to_value(wire).unwrap()).unwrap(),
            hash,
            "the gateway must not alter an already-normalized granted request"
        );
        assert_eq!(
            original.messages.len(),
            12,
            "stored replay history remains separate"
        );
    }

    #[test]
    fn other_models_and_backends_keep_their_existing_message_envelopes() {
        for (backend, model) in [
            (
                InferenceBackendKind::Fireworks,
                "accounts/fireworks/models/kimi-k3",
            ),
            (
                InferenceBackendKind::Fireworks,
                "accounts/fireworks/models/glm-5p3",
            ),
            (
                InferenceBackendKind::Openrouter,
                "accounts/fireworks/models/minimax-m3",
            ),
            (
                InferenceBackendKind::OpenaiCompatible,
                "accounts/fireworks/models/minimax-m3",
            ),
        ] {
            let mut wire = request(model);
            let before = wire.clone();
            preserve_minimax_system_context(&mut wire, backend);
            assert_eq!(wire, before);
        }
    }

    #[test]
    fn minimax_empty_single_and_late_only_system_context_remain_defined() {
        let mut wire = request("accounts/fireworks/models/minimax-m3");
        for messages in [
            json!([]),
            json!([{"role":"user","content":"task"}]),
            json!([{"role":"system","content":""},{"role":"user","content":"task"}]),
        ] {
            wire.messages = serde_json::from_value(messages).unwrap();
            let before = wire.clone();
            preserve_minimax_system_context(&mut wire, InferenceBackendKind::Fireworks);
            assert_eq!(wire, before);
        }
        wire.messages=serde_json::from_value(json!([{"role":"user","content":"task"},{"role":"system","content":"Recovery checkpoint"}])).unwrap();
        preserve_minimax_system_context(&mut wire, InferenceBackendKind::Fireworks);
        assert_eq!(wire.messages[0].content, "Recovery checkpoint");
        assert_eq!(wire.messages[1].content, "task");
    }
}
