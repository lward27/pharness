// Presentation fixtures only. These records never qualify a model or contact a provider.
import { payload } from "./lamina.mjs";
export const policyId = "planner-kimi-k3-v2";
export function protocolPayload(path, params, state = "unverified") {
  const expires = state === "stale" ? "1" : "4102444800";
  const verification = state === "unverified" ? null : {
    id: "presentation-verification", status: state === "failed" ? "failed" : "passed", expires_at: expires,
    sanitized_failure: state === "failed" ? "Protocol response timed out; no semantic qualification was dispatched." : null,
    observed_capabilities: { policy: { policy_id: policyId, revision: "v1", policy_hash: "fixture-policy" } },
  };
  if (path === "/api/inference-targets") return {
    gateway_enabled: true, registry_hash: "fixture-registry",
    gateway: { status: "available", registry_aligned: true, api_registry_hash: "fixture-registry", gateway_registry_hash: "fixture-registry" },
    targets: [{ target_id: "fireworks-kimi-k3", revision: "v1", display_name: "Kimi K3", backend_kind: "fireworks", upstream_model: "kimi-k3",
      allowed_stages: ["onboarding", "plan"], transport: { scheme: "https", private_network: false }, authentication_configured: true,
      context_limit_tokens: 65536, output_limit_tokens: 8192, selectable: true, config_hash: "fixture-target",
      latest_verification: { status: "passed", expires_at: "4102444800", observed_capabilities: { policy: { policy_id: "onboarding-kimi-k3-v2", revision: "v1" } } } }],
  };
  if (path === "/api/inference-policies") return { registry_hash: "fixture-registry", policies: [{
    policy_id: policyId, revision: "v1", display_name: "Planner Kimi K3", eligible_stages: ["plan"], eligible_profiles: ["repo-planner"],
    target: { target_id: "fireworks-kimi-k3", revision: "v1" }, reasoning: { effort: "high", context_mode: "current_turn" },
    tool_choice: "required", context_assembly_limit: 65536, maximum_output_tokens: 8192, selectable: true,
    qualified: false, qualification_status: "not_qualified", policy_hash: "fixture-policy",
    protocol_ready: state === "passed", latest_protocol_verification: verification,
    qualification_contract: { suite_id: "planner-v2", agent_profile_id: "repo-planner", agent_profile_hash: "fixture-profile" },
  }] };
  return payload(path, params, "waiting");
}
