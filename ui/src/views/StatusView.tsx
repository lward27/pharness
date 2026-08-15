import { useState } from "react";
import { RocketLaunch, ShieldCheck } from "@phosphor-icons/react";
import { ReviewItem } from "../components/Operational";
import { compactId, statusText } from "../lib/formatters";
import { dispatchTektonE2eSmoke, loadPipelineIntent, prepareTektonE2eSmoke } from "../api/delivery";

const plannedCapabilities = ["ChangeSet detail views", "Capability catalog", "Cluster mutations", "Registry auth", "Database operator", "RAG context", "MCP adapters"];

function ImplementationStrip({ dashboard }: { dashboard: any }) {
  const worker = dashboard.data?.config?.worker;
  const liveSurfaces = ["Flow read model", "WorkPlan list", "Run queue", "Run detail live events", worker?.enabled ? `${worker?.mode ?? "model"} worker` : "Worker disabled", "Tool approvals", "Approval gates", "Incidents", "Remediation plans", "Observations", "Audit log"];
  return <section className="implementation-strip" aria-label="Implementation status"><div><strong>Live API-backed</strong><span>{liveSurfaces.join(" / ")}</span></div><div><strong>Planned only</strong><span>{plannedCapabilities.slice(0, 5).join(" / ")}</span></div></section>;
}

function DeliveryTestView({ refreshDashboard, navigate }: { refreshDashboard: () => Promise<unknown> | void; navigate: (view: string, param?: any) => void }) {
  const [acknowledged, setAcknowledged] = useState(false);
  const [state, setState] = useState<any>({ phase: "idle", data: null, error: null, detail: null });
  const prepare = async () => {
    setState({ phase: "preparing", data: null, error: null, detail: "Creating the audited delivery chain and validating a dry-run." });
    try { const data = await prepareTektonE2eSmoke(); setState({ phase: "ready", data, error: null, detail: "Preflight passed. No PipelineRun has been created." }); refreshDashboard(); }
    catch (error) { setState({ phase: "failed", data: null, error: error instanceof Error ? error.message : String(error), detail: null }); }
  };
  const dispatch = async () => {
    const pipelineIntentId = state.data?.pipelineIntent?.id;
    if (!pipelineIntentId) return;
    setState((current: any) => ({ ...current, phase: "dispatching", error: null, detail: "Dispatching the dedicated executor Job." }));
    try {
      const dispatchResult = await dispatchTektonE2eSmoke(pipelineIntentId);
      setState((current: any) => ({ ...current, phase: "observing", detail: `Executor ${dispatchResult.executor_job_name} dispatched. Waiting for durable terminal evidence.` }));
      for (let attempt = 0; attempt < 80; attempt += 1) {
        const intent = await loadPipelineIntent(pipelineIntentId);
        const execution = intent.execution_evidence;
        if (execution?.status === "succeeded") { setState((current: any) => ({ ...current, phase: "completed", detail: `PipelineRun ${execution.pipeline_run?.namespace}/${execution.pipeline_run?.name} completed successfully.`, data: { ...current.data, pipelineIntent: intent } })); refreshDashboard(); return; }
        if (execution?.status === "failed") throw new Error(execution.error || "The Tekton executor reported failure.");
        await new Promise((resolve) => window.setTimeout(resolve, 3_000));
      }
      throw new Error("Timed out waiting for the executor's terminal evidence.");
    } catch (error) { setState((current: any) => ({ ...current, phase: "failed", error: error instanceof Error ? error.message : String(error), detail: null })); }
  };
  const busy = ["preparing", "dispatching", "observing"].includes(state.phase);
  const intent = state.data?.pipelineIntent;
  const execution = intent?.execution_evidence;
  return <section className="delivery-test-view"><header className="delivery-test-heading"><div><span className="eyebrow">Bounded execution</span><h1>Tekton Delivery Test</h1><p>Exercises the real Pharness execution path with one inert Pipeline. It does not read secrets or change finance applications.</p></div><span className={`status-chip ${state.phase === "completed" ? "healthy" : state.phase === "failed" ? "blocked" : "pending"}`}>{statusText(state.phase, "Ready")}</span></header>
    <section className="delivery-test-scope"><ReviewItem label="Fixture" value="tekton-pipelines/pharness-e2e-noop" /><ReviewItem label="Pipeline inputs" value="No parameters or workspaces" /><ReviewItem label="Application impact" value="None" tone="healthy" /><ReviewItem label="Evidence" value="Audit chain and terminal PipelineRun receipt" /></section>
    <section className="delivery-test-actions"><label className="delivery-test-ack"><input type="checkbox" checked={acknowledged} onChange={(event) => setAcknowledged(event.target.checked)} disabled={busy || state.phase === "completed"} /><span>I understand this creates durable smoke records and, after preflight, one inert PipelineRun.</span></label><div className="delivery-test-buttons"><button className="primary-action" type="button" onClick={prepare} disabled={!acknowledged || busy || state.phase === "ready" || state.phase === "completed"}><ShieldCheck size={18} /> {state.phase === "preparing" ? "Preparing" : "Prepare preflight"}</button><button className="secondary-action" type="button" onClick={dispatch} disabled={state.phase !== "ready" || !acknowledged}><RocketLaunch size={18} /> Dispatch inert PipelineRun</button></div><p className="delivery-test-detail">{state.detail ?? "Preflight is required before the dispatch button becomes available."}</p>{state.error ? <div className="api-banner">Delivery test failed: {state.error}</div> : null}</section>
    {state.data ? <section className="delivery-test-result"><h2>Durable records</h2><div className="review-grid"><ReviewItem label="WorkPlan" value={compactId(state.data.workPlan?.id)} /><ReviewItem label="ChangeSet" value={compactId(state.data.changeSet?.id)} /><ReviewItem label="PipelineIntent" value={compactId(intent?.id)} /><ReviewItem label="PipelineContract" value={compactId(state.data.pipelineContract?.id)} /><ReviewItem label="Preflight" value={state.data.preview?.ready ? "Passed" : "Blocked"} tone={state.data.preview?.ready ? "healthy" : "blocked"} /><ReviewItem label="PipelineRun" value={execution?.pipeline_run ? `${execution.pipeline_run.namespace}/${execution.pipeline_run.name}` : "Not dispatched"} tone={execution?.status === "succeeded" ? "healthy" : undefined} /></div><button className="text-action" type="button" onClick={() => intent?.id && navigate("Flow", { kind: "change_set", id: state.data.changeSet.id })}>Open delivery flow</button></section> : null}
  </section>;
}

export function StatusView({ dashboard, navigate }: { dashboard: any; navigate: (view: string, param?: any) => void }) {
  return <section className="status-view"><div className="section-heading"><div><h1>System status</h1><p>Implementation and bounded fixture information live here, outside the operator workflow.</p></div></div><ImplementationStrip dashboard={dashboard} /><DeliveryTestView refreshDashboard={dashboard.refresh} navigate={navigate} /></section>;
}
