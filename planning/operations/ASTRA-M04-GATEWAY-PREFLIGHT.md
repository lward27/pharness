# ASTRA M04: Check an exact gateway policy before evaluation

Use after the [policy-bound protocol correction](../evidence/autonomous-sdlc/ASTRA-M04-POLICY-BOUND-PROTOCOL.md) has a verified immutable deployment and service window. Earlier APIs reject the new request field. This procedure makes bounded model calls and persists verification evidence; it does not submit application work or qualify/activate a policy.

1. Read `/api/system/readiness` and `/api/inference-policies`. Confirm the intended API source, registry hash, exact policy revision and its target. Keep one live evaluation at a time and existing limits. Read the named operator Secret only through the maintained secret-safe interface; never put its value in command arguments, logs or this document.
2. In Settings → Model inference, choose **Check protocol** on that policy. For an API caller, POST to `/api/inference-targets/{target_id}/revisions/{target_revision}/preflight` with the body below, using the current registry hash. Record operation identity and the request before dispatch. Do not repeat an uncertain POST; reconcile the returned verification list first.
3. Retain the response. Check its actual policy ID/revision/hash, target identity, registry and runtime, passing 30/30 calibration and unchanged 900-second expiry. Check the latest matching record; a newer failed check supersedes an older pass. The actual report hash is distinct from the deterministic expected protocol-contract hash in an evaluation binding.
4. Submit the already declared one-attempt diagnostic through the existing policy qualification endpoint/maintained evaluation helper while that receipt remains fresh. A matched control needs its own exact policy check, even if it shares the primary's model. Preserve native reports, execution receipts, failed attempts and scope. Full qualification thresholds remain unchanged.

```json
{
  "actor": "lucas",
  "reason": "ASTRA M04 declared Planner protocol check",
  "config_hash": "CURRENT_REGISTRY_HASH",
  "policy": { "policy_id": "planner-kimi-k3-v2", "revision": "v1" }
}
```

The server rejects omitted policies on shared targets, mismatched references, old unbound receipts and changed runtime/configuration. GET requests never dispatch checks. A failed protocol timeout is not proof that credentials or model visibility independently failed, nor permission to extend deadlines or change models. Diagnose the observed boundary before retrying or requesting a configuration decision.
