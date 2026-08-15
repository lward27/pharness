import { useEffect, useState } from "react";
import { RocketLaunch, ShieldCheck } from "@phosphor-icons/react";
import { ReviewItem } from "../components/Operational";
import { compactId, statusText } from "../lib/formatters";
import { dispatchTektonE2eSmoke, loadPipelineIntent, prepareTektonE2eSmoke } from "../api/delivery";
import { loadSystemReadiness } from "../api/workItems";

function ReleaseReadinessPanel() {
  const [state, setState] = useState<any>({ status: "loading", data: null, error: null });
  const compiledUiRevision = import.meta.env.VITE_PHARNESS_BUILD_REVISION ?? "unknown";
  const refresh = () => {
    setState((current: any) => ({ ...current, status: current.data ? "refreshing" : "loading", error: null }));
    loadSystemReadiness().then((data) => setState({ status: "ready", data, error: null })).catch((error) => setState({ status: "error", data: null, error: error instanceof Error ? error.message : String(error) }));
  };
  useEffect(refresh, []);
  const readiness = state.data;
  const uiMatches = readiness && compiledUiRevision !== "unknown" && compiledUiRevision === readiness.ui_revision && compiledUiRevision === readiness.api_revision;
  return <section className="release-readiness" aria-label="Release readiness">
    <div className="table-heading"><div><span className="eyebrow">Release readiness</span><h2>Running provenance and isolated capabilities</h2><p>Configured is not treated as verified. Production submission requires fresh passing checks.</p></div><button type="button" onClick={refresh}>Refresh</button></div>
    {readiness ? <><div className="readiness-provenance"><ReviewItem label="API revision" value={readiness.api_revision} tone={uiMatches ? "healthy" : "risk"} /><ReviewItem label="Compiled UI revision" value={compiledUiRevision} tone={uiMatches ? "healthy" : "risk"} /><ReviewItem label="Runtime digest" value={readiness.runtime_image_digest} /><ReviewItem label="UI digest" value={readiness.ui_image_digest} /><ReviewItem label="Version alignment" value={uiMatches && readiness.platform_versions_match ? "Matched" : "Mismatch"} tone={uiMatches && readiness.platform_versions_match ? "healthy" : "risk"} /></div><div className="capability-grid">{readiness.capabilities?.map((capability: any) => <article key={capability.capability}><span className={`capability-dot ${capability.status}`} /><strong>{statusText(capability.capability)}</strong><em>{statusText(capability.status)}</em><p>{capability.summary}</p></article>)}</div><details className="readiness-details"><summary>Repository allowlists and protected target</summary><pre>{JSON.stringify({ repository_allowlists: readiness.repository_allowlists, protected_target: readiness.targets }, null, 2)}</pre></details>{readiness.blockers?.length ? <ul className="preflight-blockers">{readiness.blockers.map((blocker: string) => <li key={blocker}>{blocker}</li>)}</ul> : null}</> : <EmptyReadiness status={state.status} error={state.error} />}
  </section>;
}

function EmptyReadiness({ status, error }: { status: string; error?: string }) { return <div className="api-banner">{error ?? (status === "loading" ? "Loading release readiness…" : "Release readiness unavailable.")}</div>; }

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
  return <section className="status-view"><div className="section-heading"><div><h1>System status</h1><p>Immutable release provenance, capability truth, and bounded fixture information.</p></div></div><ReleaseReadinessPanel /><DeliveryTestView refreshDashboard={dashboard.refresh} navigate={navigate} /></section>;
}
