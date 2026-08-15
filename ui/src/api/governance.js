import { fetchJson, postJson, withQuery } from "./http";
import { getOperatorName } from "./operator";

export function decideApproval(approvalId, decision) {
  const endpoint = decision === "approved" ? "approve" : "deny";
  return postJson(`/api/approvals/${encodeURIComponent(approvalId)}/${endpoint}`, {
    decided_by: getOperatorName(),
    reason: `operator ${decision} from pharness ui`,
  });
}

export function decideApprovalGate(gateId, decision, decidedBy = getOperatorName(), reason = "") {
  const endpoint = { satisfied: "satisfy", waived: "waive", rejected: "reject" }[decision];
  if (!endpoint) throw new Error(`unsupported approval gate decision: ${decision}`);
  return postJson(`/api/approval-gates/${encodeURIComponent(gateId)}/${endpoint}`, {
    decided_by: decidedBy,
    reason,
  });
}

export function batchDecideApprovalGates(gateIds, decision, decidedBy, reason) {
  return postJson("/api/approval-gates/batch-decide", { gate_ids: gateIds, decision, decided_by: decidedBy, reason });
}

export function loadApprovals(filters = {}) {
  return fetchJson(withQuery("/api/approvals", filters));
}

export function loadApprovalGates(filters = {}) {
  return fetchJson(withQuery("/api/approval-gates", filters));
}
