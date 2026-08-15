// Compatibility barrel. New UI code should import from the focused resource client when practical.
export { getOperatorName, setOperatorName } from "./api/operator";
export { applyWorkItemReconcile, loadWorkItem, loadWorkItemFlow, previewWorkItemReconcile } from "./api/workItems";
export { batchDecideApprovalGates, decideApproval, decideApprovalGate } from "./api/governance";
export { cancelRun, loadRunDetail, submitRun, subscribeRunEvents } from "./api/runs";
export { dispatchTektonE2eSmoke, loadPipelineIntent, prepareTektonE2eSmoke } from "./api/delivery";
export { loadAuditEvents, loadDashboardData, loadTriage, loadTriageSummary, loadWorkPlanFlow } from "./api/dashboard";
