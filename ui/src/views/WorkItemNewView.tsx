import { useEffect, useMemo, useState } from "react";
import { ArrowLeft, ArrowRight, CheckCircle, Plus, ShieldCheck, Trash } from "@phosphor-icons/react";
import { createWorkItem, loadEnvironmentProfiles, loadSystemReadiness, preflightWorkItem, verifySystemCapability } from "../api/workItems";
import { fetchJson, withQuery } from "../api/http";
import { EmptyState, StatusPill } from "../components/Operational";
import { statusText } from "../lib/formatters";

const protectedTarget = {
  target_environment: "production",
  target_namespace: "apps-prod",
  argo_application: "yfinance-wrapper",
  workload_kind: "Deployment",
  workload_name: "yfinance-wrapper",
  gitops_repo: "https://github.com/lward27/lucas_engineering.git",
  gitops_ref: "main",
  gitops_kustomization_path: "charts/yfinance-wrapper/kustomization.yaml",
  gitops_image_name: "registry.lucas.engineering/yfinance_wrapper",
  rollback_owner: "lucas",
};

const initialForm = {
  title: "Add ticker and date validation",
  intent: "Add a pure validation module for ticker normalization and date ranges, wire it into FastAPI endpoints, add standard-library tests, and update usage documentation.",
  acceptance_criteria: ["python -m unittest discover -s tests -v", "python -m compileall -q src tests"],
  source_repo: "https://github.com/lward27/yfinance_wrapper.git",
  source_ref: "main",
  source_commit: "",
  pipeline_contract_id: "",
  deployment_contract_id: "",
  production_impacting: true,
  environment_profile_id: "python-3.11",
  max_attempts: 2,
  max_elapsed_seconds: 3600,
  initial_turn_budget: 48,
  hard_turn_budget: 100,
  initial_token_budget: 400000,
  hard_token_budget: 1000000,
  active_execution_seconds: 3600,
  recoverable_tool_error_limit: 4,
  identical_failure_limit: 2,
  ...protectedTarget,
};

const steps = ["Immutable source", "Intent and acceptance", "Environment and budget", "Contracts and preflight", "Mutation summary"];

export function WorkItemNewView({ operatorName, onCancel, onCreated }: { operatorName?: string; onCancel: () => void; onCreated: (id: string) => void }) {
  const [step, setStep] = useState(0);
  const [form, setForm] = useState<any>(initialForm);
  const [contracts, setContracts] = useState<any>({ pipelines: [], deployments: [] });
  const [profiles, setProfiles] = useState<any[]>([]);
  const [providerTransportAttempts, setProviderTransportAttempts] = useState<number | null>(null);
  const [advancedBudget, setAdvancedBudget] = useState(false);
  const [preflight, setPreflight] = useState<any>(null);
  const [warningAck, setWarningAck] = useState(false);
  const [state, setState] = useState<any>({ status: "idle", error: null });
  const payload = useMemo(() => ({ ...form, actor: operatorName ?? "console-operator" }), [form, operatorName]);

  useEffect(() => {
    Promise.all([
      fetchJson(withQuery("/api/pipeline-contracts", { status: "active", limit: 100 })),
      fetchJson(withQuery("/api/deployment-contracts", { status: "active", limit: 100 })),
      loadEnvironmentProfiles(),
    ]).then(([pipelines, deployments, environmentProfiles]) => { setContracts({
      pipelines: pipelines?.pipeline_contracts ?? [], deployments: deployments?.deployment_contracts ?? [],
    }); setProfiles(environmentProfiles?.profiles ?? []); setProviderTransportAttempts(environmentProfiles?.provider_transport_attempts ?? null); }).catch((error) => setState({ status: "error", error: error instanceof Error ? error.message : String(error) }));
  }, []);

  const runPreflight = async () => {
    setState({ status: "preflighting", error: null });
    try {
      const readiness = await loadSystemReadiness();
      const profileCapability = `environment_profile:${form.environment_profile_id}`;
      await Promise.all(
        [...(readiness.capabilities ?? []).map((entry: any) => entry.capability), profileCapability]
          .filter((capability: string, index: number, values: string[]) => values.indexOf(capability) === index)
          .map((capability: string) => readiness.capabilities?.find((entry: any) => entry.capability === capability) ?? { capability, status: profiles.find((profile) => `environment_profile:${profile.id}` === capability)?.status })
          .filter((entry: any) => entry.status !== "available")
          .map((entry: any) => verifySystemCapability(entry.capability)),
      );
      const result = await preflightWorkItem(payload);
      setPreflight(result);
      setWarningAck(false);
      setState({ status: "idle", error: null });
      return result;
    } catch (error) {
      setState({ status: "error", error: error instanceof Error ? error.message : String(error) });
      return null;
    }
  };

  const submit = async () => {
    if (!preflight?.ready || (preflight.warnings?.length && !warningAck)) return;
    setState({ status: "submitting", error: null });
    try {
      const created = await createWorkItem({ ...payload, preflight_state_hash: preflight.state_hash });
      onCreated(created.id);
    } catch (error) {
      setState({ status: "error", error: error instanceof Error ? error.message : String(error) });
    }
  };

  const update = (field: string, value: any) => {
    setForm((current: any) => ({ ...current, [field]: value }));
    setPreflight(null);
  };
  const updateCriterion = (index: number, value: string) => update("acceptance_criteria", form.acceptance_criteria.map((entry: string, entryIndex: number) => entryIndex === index ? value : entry));
  const canContinue = step === 0 ? form.source_repo.trim() && form.source_ref.trim() && /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(form.source_commit.trim())
    : step === 1 ? form.intent.trim() && form.acceptance_criteria.some((entry: string) => entry.trim())
      : true;

  return <section className="work-item-new">
    <div className="section-heading"><div><button className="back-link" type="button" onClick={onCancel}>WorkItems</button><h1>New WorkItem</h1><p>Define an immutable, supervised delivery contract before PHarness creates durable state.</p></div><StatusPill tone={preflight?.ready ? "healthy" : "pending"}>{preflight?.ready ? "Preflight passed" : `Step ${step + 1} of 5`}</StatusPill></div>
    <ol className="wizard-steps">{steps.map((label, index) => <li className={index === step ? "active" : index < step ? "complete" : ""} key={label}><span>{index < step ? <CheckCircle size={16} /> : index + 1}</span>{label}</li>)}</ol>

    <section className="wizard-card">
      {step === 0 ? <><span className="eyebrow">Repository and immutable source</span><h2>Pin the exact object PHarness may edit</h2><label>Repository<input value={form.source_repo} onChange={(event) => update("source_repo", event.target.value)} /></label><div className="wizard-grid"><label>PR base branch<input value={form.source_ref} onChange={(event) => update("source_ref", event.target.value)} /></label><label>Full source commit SHA<input value={form.source_commit} onChange={(event) => update("source_commit", event.target.value)} placeholder="40-character commit SHA" /></label></div><p className="field-note">The worker checks out this exact commit and rejects any resolved-SHA mismatch.</p></> : null}
      {step === 1 ? <><span className="eyebrow">Intent and acceptance</span><h2>State the bounded change and executable proof</h2><label>Title<input value={form.title} onChange={(event) => update("title", event.target.value)} /></label><label>Intent<textarea rows={5} value={form.intent} onChange={(event) => update("intent", event.target.value)} /></label><div className="acceptance-editor"><strong>Acceptance commands</strong>{form.acceptance_criteria.map((criterion: string, index: number) => <div key={index}><input aria-label={`Acceptance command ${index + 1}`} value={criterion} onChange={(event) => updateCriterion(index, event.target.value)} /><button type="button" aria-label={`Remove acceptance command ${index + 1}`} onClick={() => update("acceptance_criteria", form.acceptance_criteria.filter((_: string, entryIndex: number) => entryIndex !== index))}><Trash size={16} /></button></div>)}<button type="button" onClick={() => update("acceptance_criteria", [...form.acceptance_criteria, ""])}><Plus size={16} /> Add command</button></div></> : null}
      {step === 2 ? <><span className="eyebrow">Environment and attempt budget</span><h2>Bound execution before it begins</h2><div className="wizard-grid"><label>Environment<input value={form.target_environment} readOnly /></label><label>Namespace<input value={form.target_namespace} readOnly /></label><label>Argo Application<input value={form.argo_application} readOnly /></label><label>Workload<input value={`${form.workload_kind}/${form.workload_name}`} readOnly /></label><label>Runner profile<select value={form.environment_profile_id} onChange={(event) => update("environment_profile_id", event.target.value)}><option value="">Select immutable profile</option>{profiles.map((profile: any) => <option key={profile.id} value={profile.id}>{profile.id} · {statusText(profile.status)}</option>)}</select></label><label>Maximum attempts<input type="number" min="1" max="3" value={form.max_attempts} onChange={(event) => update("max_attempts", Number(event.target.value))} /></label><label>Provider transport attempts<input value={providerTransportAttempts == null ? "Loading from server" : `${providerTransportAttempts} · server-owned`} readOnly /></label></div><button type="button" onClick={() => setAdvancedBudget((value) => !value)}>{advancedBudget ? "Hide advanced budgets" : "Advanced budgets"}</button>{advancedBudget ? <div className="wizard-grid advanced-budget"><label>Initial turns<input type="number" min="1" max="100" value={form.initial_turn_budget} onChange={(event) => update("initial_turn_budget", Number(event.target.value))} /></label><label>Hard turn maximum<input type="number" min="1" max="100" value={form.hard_turn_budget} onChange={(event) => update("hard_turn_budget", Number(event.target.value))} /></label><label>Initial tokens<input type="number" min="1" max="1000000" value={form.initial_token_budget} onChange={(event) => update("initial_token_budget", Number(event.target.value))} /></label><label>Hard token maximum<input type="number" min="1" max="1000000" value={form.hard_token_budget} onChange={(event) => update("hard_token_budget", Number(event.target.value))} /></label><label>Active execution seconds<input type="number" min="60" max="86400" value={form.active_execution_seconds} onChange={(event) => { update("active_execution_seconds", Number(event.target.value)); update("max_elapsed_seconds", Number(event.target.value)); }} /></label><label>Recoverable tool errors<input type="number" min="0" max="8" value={form.recoverable_tool_error_limit} onChange={(event) => update("recoverable_tool_error_limit", Number(event.target.value))} /></label><label>Identical failure limit<input type="number" min="1" max="3" value={form.identical_failure_limit} onChange={(event) => update("identical_failure_limit", Number(event.target.value))} /></label></div> : null}<p className="field-note">Standard: 2 attempts, 48 initial / 100 hard turns, 400,000 initial / 1,000,000 hard tokens, 3,600 active seconds. Maximum envelope: {form.max_attempts * form.hard_turn_budget} turns and {form.max_attempts * form.hard_token_budget} tokens across attempts.</p></> : null}
      {step === 3 ? <><span className="eyebrow">Delivery contracts and capability preflight</span><h2>Bind contracts, then verify every isolated identity</h2><div className="wizard-grid"><label>PipelineContract<select value={form.pipeline_contract_id} onChange={(event) => update("pipeline_contract_id", event.target.value)}><option value="">Select active contract</option>{contracts.pipelines.map((contract: any) => <option key={contract.id} value={contract.id}>{contract.namespace}/{contract.pipeline_ref} · {contract.id}</option>)}</select></label><label>DeploymentContract<select value={form.deployment_contract_id} onChange={(event) => update("deployment_contract_id", event.target.value)}><option value="">Select active contract</option>{contracts.deployments.map((contract: any) => <option key={contract.id} value={contract.id}>{contract.environment}/{contract.namespace} · {contract.id}</option>)}</select></label></div><button className="primary-action" type="button" onClick={runPreflight} disabled={state.status === "preflighting" || !form.pipeline_contract_id || !form.deployment_contract_id}><ShieldCheck size={17} /> {state.status === "preflighting" ? "Checking capabilities" : "Run read-only preflight"}</button>{preflight ? <PreflightResult preflight={preflight} /> : <EmptyState title="Preflight not run" body="Submission stays blocked until contracts, allowlists, identities, target, and rollback prerequisites pass." />}</> : null}
      {step === 4 ? <><span className="eyebrow">Final mutation summary</span><h2>Review exactly what PHarness may change</h2><dl className="mutation-summary"><div><dt>Source</dt><dd>{form.source_repo} @ {form.source_commit}</dd></div><div><dt>Runner</dt><dd>{form.environment_profile_id}</dd></div><div><dt>Dependency lock</dt><dd>{preflight?.normalized_submission?.repository_contract?.dependency_lock?.path ?? "Not validated"}</dd></div><div><dt>Writable paths</dt><dd>{preflight?.normalized_submission?.repository_contract?.writable_paths?.join(" · ") ?? "Not validated"}</dd></div><div><dt>GitOps target</dt><dd>{form.gitops_repo} · {form.gitops_kustomization_path}</dd></div><div><dt>Deployment</dt><dd>{form.argo_application} → {form.target_namespace}/{form.workload_name}</dd></div><div><dt>Rollback owner</dt><dd>{form.rollback_owner}</dd></div></dl><ul className="mutation-list">{preflight?.predicted_external_mutations?.map((mutation: string) => <li key={mutation}>{mutation}</li>)}</ul>{preflight?.warnings?.length ? <label className="warning-ack"><input type="checkbox" checked={warningAck} onChange={(event) => setWarningAck(event.target.checked)} />I acknowledge the non-blocking preflight warnings.</label> : null}<button className="primary-action" type="button" onClick={submit} disabled={!preflight?.ready || state.status === "submitting" || (preflight?.warnings?.length && !warningAck)}>{state.status === "submitting" ? "Submitting" : "Create supervised WorkItem"}</button></> : null}
    </section>
    {state.error ? <div className="api-banner">{state.error}</div> : null}
    <div className="wizard-navigation"><button type="button" onClick={() => setStep((current) => Math.max(0, current - 1))} disabled={step === 0}><ArrowLeft size={16} /> Back</button>{step < 4 ? <button className="primary-action" type="button" onClick={async () => { if (step === 3 && !preflight) await runPreflight(); setStep((current) => Math.min(4, current + 1)); }} disabled={!canContinue || (step === 3 && !preflight?.ready)}>{step === 3 ? "Review mutations" : "Continue"} <ArrowRight size={16} /></button> : null}</div>
  </section>;
}

function PreflightResult({ preflight }: { preflight: any }) {
  return <section className={`preflight-result ${preflight.ready ? "ready" : "blocked"}`}><div><strong>{preflight.ready ? "Submission ready" : `${preflight.blockers?.length ?? 0} blocking checks`}</strong><StatusPill tone={preflight.ready ? "healthy" : "blocked"}>{preflight.ready ? "Passed" : "Blocked"}</StatusPill></div><div className="capability-grid">{preflight.checks?.map((check: any) => <article key={check.capability}><span className={`capability-dot ${check.status}`} /><strong>{statusText(check.capability)}</strong><em>{statusText(check.status)}</em><p>{check.summary}</p></article>)}</div>{preflight.blockers?.length ? <ul className="preflight-blockers">{preflight.blockers.map((blocker: string) => <li key={blocker}>{blocker}</li>)}</ul> : null}</section>;
}
