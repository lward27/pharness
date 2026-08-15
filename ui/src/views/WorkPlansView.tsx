import { useEffect, useState } from "react";
import { ArrowsClockwise } from "@phosphor-icons/react";
import { EmptyState, OperatorListFilters, ReadinessFacts, ReviewItem, StatusPill } from "../components/Operational";
import { ServerGroups } from "../components/ServerGroups";
import { compactId, lifecycleTone, riskTone, statusText } from "../lib/formatters";
import { matchesOperationalFilters, operationalFilterOptions } from "../lib/operational";
import { resourceLabel } from "../lib/resourcePresentation";
import { loadWorkPlanFlow } from "../api/dashboard";

type WorkPlansViewProps = { dashboard: any; selectedId: string | null; navigate: (view: string, param?: any) => void };

function WorkPlanFlowSummary({ flow, status }: { flow: any; status: string }) {
  if (!flow) return <EmptyState title="No WorkPlan flow loaded" body={status === "error" ? "The API did not return a flow for this WorkPlan." : "Select a WorkPlan to load its flow."} />;
  const readiness = flow.readiness;
  const downstream = [
    ["ChangeSet", flow.change_set?.status, flow.change_set?.id],
    ["PipelineIntent", flow.pipeline_intent?.status, flow.pipeline_intent?.id],
    ["DeploymentIntent", flow.deployment_intent?.status, flow.deployment_intent?.id],
    ["Release", flow.release?.status, flow.release?.id],
    ["RegistryEvidence", flow.registry_evidence?.status, flow.registry_evidence?.image_ref],
  ];
  return <><section className="workplan-readiness"><div><span>Readiness</span><strong>{readiness?.summary ?? "readiness unavailable"}</strong></div><div><span>Blockers</span><strong>{readiness?.blockers?.length ?? 0}</strong></div><div><span>Warnings</span><strong>{readiness?.warnings?.length ?? 0}</strong></div></section>
    <div className="downstream-list">{downstream.map(([label, statusValue, target]) => <div key={label as string}><span>{label}</span><StatusPill tone={statusValue ? lifecycleTone(statusValue as string) : "future"}>{statusText(statusValue as string, "Missing")}</StatusPill><strong title={(target as string) ?? "not created"}>{target ? compactId(String(target)) : "not created"}</strong></div>)}</div>
    <ReadinessFacts readiness={readiness} />
  </>;
}

export function WorkPlansView({ dashboard, selectedId, navigate }: WorkPlansViewProps) {
  const workPlans = dashboard.data?.workPlans ?? [];
  const workPlanGroups = dashboard.data?.workPlanGroups ?? [];
  const scopeOptions = dashboard.data?.scopeOptions ?? {};
  const [listFilters, setListFilters] = useState({ search: "", status: "", actor: "", origin: "" });
  const filteredWorkPlans = workPlans.filter((plan: any) => matchesOperationalFilters(plan, listFilters, (item) => [item.title, item.summary, resourceLabel(item), item.id].filter(Boolean).join(" ")));
  const filterOptions = operationalFilterOptions(workPlans, scopeOptions);
  const filtersActive = Object.values(listFilters).some(Boolean);
  const [detail, setDetail] = useState({ status: "idle", flow: null as any, error: null as string | null });
  const selectedWorkPlan = filteredWorkPlans.find((plan: any) => plan.id === selectedId) ?? filteredWorkPlans[0] ?? null;
  const statusBuckets = filteredWorkPlans.reduce((counts: Record<string, number>, plan: any) => ({ ...counts, [plan.status]: (counts[plan.status] ?? 0) + 1 }), {});
  const highRisk = filteredWorkPlans.filter((plan: any) => ["high", "critical"].includes(plan.risk_level)).length;
  const metrics = [["Plans", String(filteredWorkPlans.length), "latest page"], ["Approved", String(statusBuckets.approved ?? 0), "ready for changes"], ["Blocked", String(statusBuckets.blocked ?? 0), "needs review"], ["High risk", String(highRisk), "operator attention"]];

  useEffect(() => {
    let active = true;
    if (!selectedWorkPlan?.id) { setDetail({ status: "idle", flow: null, error: null }); return () => { active = false; }; }
    setDetail((current) => ({ ...current, status: current.flow ? "refreshing" : "loading", error: null }));
    loadWorkPlanFlow(selectedWorkPlan.id).then((flow) => { if (active) setDetail({ status: "ready", flow, error: null }); }).catch((error) => { if (active) setDetail((current) => ({ status: "error", flow: current.flow, error: error instanceof Error ? error.message : String(error) })); });
    return () => { active = false; };
  }, [selectedWorkPlan?.id]);

  const readiness = detail.flow?.readiness;
  return <section className="workplans-view">
    <div className="section-heading"><div><h1>WorkPlans</h1><p>Bounded SDLC plans with live readiness and downstream evidence state.</p></div><button className="primary-action" type="button" onClick={dashboard.refresh} disabled={dashboard.status === "refreshing"}><ArrowsClockwise size={17} /> {dashboard.status === "refreshing" ? "Refreshing" : "Refresh"}</button></div>
    <div className="summary-grid">{metrics.map(([label, value, note]) => <div className="metric" key={label}><span>{label}</span><strong>{value}</strong><small>{note}</small></div>)}</div>
    <OperatorListFilters value={listFilters} statuses={filterOptions.statuses} actors={filterOptions.actors} origins={filterOptions.origins} onChange={setListFilters} />
    {!filtersActive ? <ServerGroups label="WorkPlans" groups={workPlanGroups} onOpen={(id) => navigate("WorkPlans", id)} /> : null}
    {filteredWorkPlans.length ? <div className="workplan-layout"><div className="workplan-list">{filteredWorkPlans.map((plan: any) => <button className={`workplan-card ${plan.id === selectedWorkPlan?.id ? "is-active" : ""}`} key={plan.id} type="button" onClick={() => navigate("WorkPlans", plan.id)}><span><StatusPill tone={lifecycleTone(plan.status)}>{statusText(plan.status)}</StatusPill><b className={`risk-${riskTone(plan.risk_level)}`}>{plan.risk_level}</b></span><strong title={plan.title}>{plan.title}</strong><p>{plan.summary}</p><small title={plan.id}>{compactId(plan.id)} · revision {plan.revision}</small></button>)}</div>
      <section className="review-surface"><div className="table-heading"><div><h2>{selectedWorkPlan?.title ?? "WorkPlan detail"}</h2><p>{selectedWorkPlan?.summary ?? "Select a WorkPlan to inspect its live control-plane flow."}</p>{selectedWorkPlan ? <button className="link-text" type="button" onClick={() => navigate("Flow", { kind: "work_plan", id: selectedWorkPlan.id })}>Open in Flow</button> : null}</div><StatusPill tone={readiness?.ready ? "healthy" : "blocked"}>{detail.status === "loading" ? "Loading" : readiness?.ready ? "Ready" : "Blocked"}</StatusPill></div>
        {detail.error ? <div className="api-banner">WorkPlan flow failed: {detail.error}</div> : null}
        <div className="review-grid"><ReviewItem label="Resource" value={resourceLabel(selectedWorkPlan)} /><ReviewItem label="Risk" value={statusText(selectedWorkPlan?.risk_level, "Unknown")} tone={riskTone(selectedWorkPlan?.risk_level) === "high" ? "risk" : undefined} /><ReviewItem label="Requires approval" value={String(Boolean(selectedWorkPlan?.requires_approval))} /><ReviewItem label="Run" value={compactId(String(selectedWorkPlan?.run_id ?? ""))} /></div>
        <WorkPlanFlowSummary flow={detail.flow} status={detail.status} />
      </section></div> : <EmptyState title={workPlans.length ? "No WorkPlans match these filters" : "No WorkPlans"} body={workPlans.length ? "Clear or adjust filters to inspect other durable plans." : "Create the SDLC root chain from the CLI or smoke script, then refresh this view."} />}
  </section>;
}
