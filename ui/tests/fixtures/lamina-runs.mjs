export const execution = {
  driver:"codex_app_server",
  selection:{binding:{policy:{policy_id:"codex-builder-gpt56-sol-v1",revision:"r3",display_name:"Codex Builder",driver:"codex_app_server",codex_version:"codex-cli 0.150.1",model:"gpt-5.6-sol",reasoning_effort:"high",prompt_revision:"codex-repo-builder-v1",prompt_hash:"sha256:prompt",output_schema_hash:"sha256:schema"},host_pool:"codex-reliability",authentication_class:"chatgpt_session",runner_image:`registry.example/node@sha256:${"a".repeat(64)}`,binding_hash:"sha256:binding"}},
  lease:{id:"lease_fixture",state:"paused",host_pool:"codex-reliability",workspace_id:"workspace_market",remote_thread_id:"thread_market"},host:{display_name:"lucas-desktop",lifecycle_state:"unavailable"},
};
export function runPayload(path) {
  if(!path.startsWith("/api/runs/run_")) return undefined;
  const compacted=path.includes("run_compacted");
  const id=compacted?"run_compacted":"run_codex";
  const summary={run_id:id,turns:compacted?12:4,changed_paths:["src/market.js"],test_commands:["npm test"],test_results:[{command:"npm test",passed:true}],acceptance_evidence:[{command:"npm test",passed:true}],pending_approvals:[],stop_category:compacted?"completed":"agent_host_unavailable",stop_reason:compacted?"completed":"agent_host_unavailable",actual_total_tokens:null,agent_execution:execution};
  if(path.endsWith("/operator-summary")) return summary;
  if(path.endsWith("/events")) return {events:[]};
  if(path.endsWith("/artifacts")) return {artifacts:[]};
  if(path.endsWith("/diff")) return {run_id:id,changes:[{id:"diff_market",path:"src/market.js",diff:compacted?"[purged by retention policy]":"+export const market = {};"}]};
  if(path.endsWith("/environment-preparation")) return {};
  return {id,status:compacted?"completed":"blocked",task:"Build the Finance market overview",started_at:"2026-09-04T10:30:00Z",finished_at:compacted?"2026-09-04T11:30:00Z":null,retention_state:compacted?"compacted":"retained",sealed_summary:compacted?summary:null,run_budget:{initial_turns:48,initial_tokens:400000,active_execution_seconds:3600},budget_consumption:{turns_used:summary.turns,allowed_turns:48,allowed_tokens:400000,active_execution_seconds_used:120},agent_execution:execution};
}
