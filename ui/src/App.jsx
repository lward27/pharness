import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowsClockwise,
  ChartLineUp,
  CheckCircle,
  CircleHalf,
  ClipboardText,
  ClockCounterClockwise,
  Cube,
  FileText,
  FlowArrow,
  GitBranch,
  GitPullRequest,
  HardDrives,
  Kanban,
  MagnifyingGlass,
  Moon,
  Pulse,
  RocketLaunch,
  Rows,
  ShieldCheck,
  ShieldWarning,
  SignOut,
  Siren,
  Stack,
  ToggleRight,
  X,
} from "@phosphor-icons/react";
import { cancelRun, decideApproval, decideApprovalGate, dispatchTektonE2eSmoke, loadAuditEvents, loadDashboardData, loadPipelineIntent, loadRunDetail, loadWorkPlanFlow, prepareTektonE2eSmoke, submitRun, subscribeRunEvents } from "./pharnessApi";

const navItems = [
  { id: "Flow", view: "Flow", icon: FlowArrow },
  { id: "WorkPlans", view: "WorkPlans", icon: Kanban },
  { id: "Runs", view: "Queue", icon: Pulse },
  { id: "Delivery Test", view: "Delivery Test", icon: RocketLaunch },
  { id: "Approvals", view: "Approvals", icon: ShieldWarning },
  { id: "Approval Gates", view: "Approval Gates", icon: ShieldCheck },
  { id: "Observations", view: "Observations", icon: ChartLineUp },
  { id: "Incidents", view: "Incidents", icon: Siren },
  { id: "Remediation Plans", view: "Remediation Plans", icon: ClipboardText },
  { id: "Capabilities", icon: Cube, planned: true },
  { id: "Audit", view: "Audit", icon: ClockCounterClockwise },
];

const plannedCapabilities = [
  "ChangeSet detail views",
  "Capability catalog",
  "Cluster mutations",
  "Registry auth",
  "Database operator",
  "RAG context",
  "MCP adapters",
];

// Hash routing: #/<segment>[/<id>] with Flow roots as #/flow/<kind>/<id>.
const viewSegments = {
  Flow: "flow",
  WorkPlans: "workplans",
  Queue: "queue",
  "Delivery Test": "delivery-test",
  "Run Detail": "runs",
  Approvals: "approvals",
  "Approval Gates": "gates",
  Audit: "audit",
  Incidents: "incidents",
  "Remediation Plans": "remediation-plans",
  Observations: "observations",
};

function parseHash() {
  const parts = window.location.hash.replace(/^#\/?/, "").split("/").filter(Boolean).map(decodeURIComponent);
  const [segment, first, second] = parts;
  const view = Object.keys(viewSegments).find((key) => viewSegments[key] === segment) ?? "Flow";
  if (view === "Flow" && first && second) {
    return { view, param: { kind: first, id: second } };
  }
  return { view, param: first ?? null };
}

function hashForRoute(view, param) {
  const segment = viewSegments[view] ?? "flow";
  if (view === "Flow" && param?.kind && param?.id) {
    return `#/${segment}/${encodeURIComponent(param.kind)}/${encodeURIComponent(param.id)}`;
  }
  if (param && typeof param === "string") {
    return `#/${segment}/${encodeURIComponent(param)}`;
  }
  return `#/${segment}`;
}

function navigate(view, param) {
  const next = hashForRoute(view, param);
  if (window.location.hash !== next) {
    window.location.hash = next;
  }
}

const EMPTY_SCOPE = {
  namespace: "",
  repo: "",
  branch: "",
  productionImpacting: "",
};

function usePharnessDashboard(flowRoot, scope) {
  const [state, setState] = useState({
    status: "loading",
    data: null,
    error: null,
  });

  const refresh = async () => {
    setState((current) => ({ ...current, status: current.data ? "refreshing" : "loading" }));
    try {
      const data = await loadDashboardData(flowRoot, scope);
      setState({ status: "ready", data, error: null });
    } catch (error) {
      setState((current) => ({
        status: "error",
        data: current.data,
        error: error instanceof Error ? error.message : String(error),
      }));
    }
  };

  const flowRootKey = flowRoot ? `${flowRoot.kind}:${flowRoot.id}` : "";
  const scopeKey = JSON.stringify(scope);
  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, 15_000);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [flowRootKey, scopeKey]);

  return { ...state, refresh };
}

function lifecycleTone(status) {
  if (["completed", "approved", "verified", "satisfied", "ready", "merged"].includes(status)) {
    return "healthy";
  }
  if (["running", "executing", "in_progress"].includes(status)) {
    return "running";
  }
  if (["blocked", "rejected", "failed", "stale"].includes(status)) {
    return "blocked";
  }
  if (["draft", "proposed", "pending", "approval_required"].includes(status)) {
    return "pending";
  }
  return "future";
}

function statusText(status, fallback = "Future-backed") {
  if (!status) {
    return fallback;
  }
  return status
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function summarizeJson(value, fallback) {
  if (!value || typeof value !== "object") {
    return fallback;
  }
  if (typeof value.summary === "string") {
    return value.summary;
  }
  if (typeof value.title === "string") {
    return value.title;
  }
  const keys = Object.keys(value);
  return keys.length ? keys.slice(0, 3).join(", ") : fallback;
}

function compactId(value) {
  if (!value || typeof value !== "string") {
    return "not scoped";
  }
  if (value.length <= 18) {
    return value;
  }
  return `${value.slice(0, 10)}...${value.slice(-5)}`;
}

function compactImageRef(value) {
  if (!value || typeof value !== "string") {
    return "image verification";
  }
  const [repository, tag] = value.split(":");
  const name = repository?.split("/").pop() ?? repository;
  if (!tag) {
    return compactId(name);
  }
  return `${compactId(name)}:${compactId(tag)}`;
}

function buildTopology(flow) {
  if (!flow) {
    return [];
  }

  const workPlan = flow.work_plan;
  const changeSet = flow.change_set;
  const pipelineIntent = flow.pipeline_intent;
  const deploymentIntent = flow.deployment_intent;
  const release = flow.release;
  const registryEvidence = flow.registry_evidence;
  const readiness = flow.readiness;

  return [
    {
      id: "work-plan",
      label: "WorkPlan",
      icon: Kanban,
      status: lifecycleTone(workPlan?.status),
      statusLabel: statusText(workPlan?.status),
      meta: compactId(workPlan?.id),
      subline: workPlan?.title ?? workPlan?.summary ?? "bounded plan",
    },
    {
      id: "change-set",
      label: "ChangeSet",
      icon: GitPullRequest,
      status: changeSet ? lifecycleTone(changeSet.status) : "future",
      statusLabel: changeSet ? statusText(changeSet.status) : "Not created",
      meta: changeSet ? compactId(changeSet.id) : "0 changesets",
      subline: changeSet?.title ?? "waiting for source changes",
    },
    {
      id: "pipeline-intent",
      label: "PipelineIntent",
      icon: RocketLaunch,
      status: pipelineIntent ? lifecycleTone(pipelineIntent.status) : "future",
      statusLabel: pipelineIntent ? statusText(pipelineIntent.status) : "Not created",
      meta: pipelineIntent ? compactId(pipelineIntent.id) : "0 intents",
      subline: pipelineIntent?.intent_kind ?? "build/test/package",
    },
    {
      id: "pipeline-analysis",
      label: "PipelineRunAnalysis",
      icon: MagnifyingGlass,
      status: lifecycleTone(pipelineIntent?.execution_evidence?.status ?? pipelineIntent?.intent_json?.evidence?.status),
      statusLabel: statusText(pipelineIntent?.execution_evidence?.status ?? pipelineIntent?.intent_json?.evidence?.status, "Missing"),
      meta: pipelineIntent?.execution_evidence?.pipeline_run?.name ?? pipelineIntent?.intent_json?.evidence?.summary?.image_alignment_status ?? "no evidence",
      subline: compactId(pipelineIntent?.execution_evidence?.artifact_id ?? pipelineIntent?.intent_json?.evidence?.observation_id ?? "Tekton evidence"),
    },
    {
      id: "deployment-intent",
      label: "DeploymentIntent",
      icon: SignOut,
      status: deploymentIntent ? lifecycleTone(deploymentIntent.status) : "future",
      statusLabel: deploymentIntent ? statusText(deploymentIntent.status) : "Not created",
      meta: deploymentIntent ? compactId(deploymentIntent.id) : "0 intents",
      subline: deploymentIntent?.argo_application ?? deploymentIntent?.target_namespace ?? "Argo sync gated",
    },
    {
      id: "release",
      label: "Release",
      icon: Cube,
      status: release ? lifecycleTone(release.status) : "future",
      statusLabel: release ? statusText(release.status) : "Not created",
      meta: release?.version ?? release?.id ?? "0 releases",
      subline: release?.release_kind ?? "release pending",
    },
    {
      id: "registry-evidence",
      label: "RegistryEvidence",
      icon: HardDrives,
      status: registryEvidence ? lifecycleTone(registryEvidence.status) : "future",
      statusLabel: registryEvidence ? statusText(registryEvidence.status) : "Not created",
      meta: registryEvidence?.verification_status ?? "no evidence",
      subline: compactImageRef(registryEvidence?.image_ref),
    },
  ];
}

function buildEvidenceRows(flow) {
  if (!flow) {
    return [];
  }

  return [
    {
      source: "Readiness",
      icon: ShieldCheck,
      status: flow.readiness?.ready ? "Ready" : "Blocked",
      tone: flow.readiness?.ready ? "healthy" : "blocked",
      resource: flow.resource_kind,
      target: flow.resource_id,
      finding: flow.readiness?.summary ?? "readiness unavailable",
      lastEvent: `${flow.readiness?.blockers?.length ?? 0} blockers, ${flow.readiness?.warnings?.length ?? 0} warnings`,
      link: "Readiness",
    },
    {
      source: "WorkPlan",
      icon: Kanban,
      status: statusText(flow.work_plan?.status),
      tone: lifecycleTone(flow.work_plan?.status),
      resource: "WorkPlan",
      target: flow.work_plan?.id ?? "missing",
      finding: flow.work_plan?.summary ?? flow.work_plan?.title ?? "plan available",
      lastEvent: `revision ${flow.work_plan?.revision ?? 1}`,
      link: "Plan",
    },
    {
      source: "ChangeSet",
      icon: GitPullRequest,
      status: flow.change_set ? statusText(flow.change_set.status) : "Missing",
      tone: flow.change_set ? lifecycleTone(flow.change_set.status) : "future",
      resource: "ChangeSet",
      target: flow.change_set?.id ?? "not created",
      finding: flow.change_set?.summary ?? "source changes not created yet",
      lastEvent: flow.change_set ? `revision ${flow.change_set.revision}` : "waiting",
      link: "Diff",
    },
    {
      source: "Pipeline",
      icon: RocketLaunch,
      status: flow.pipeline_intent ? statusText(flow.pipeline_intent.status) : "Missing",
      tone: flow.pipeline_intent ? lifecycleTone(flow.pipeline_intent.status) : "future",
      resource: "PipelineIntent",
      target: flow.pipeline_intent?.id ?? "not created",
      finding: summarizeJson(flow.pipeline_intent?.execution_evidence ?? flow.pipeline_intent?.intent_json?.evidence, "pipeline evidence not attached"),
      lastEvent: flow.pipeline_intent?.intent_kind ?? "planned",
      link: "Tekton",
    },
    {
      source: "Deployment",
      icon: Pulse,
      status: flow.deployment_intent ? statusText(flow.deployment_intent.status) : "Missing",
      tone: flow.deployment_intent ? lifecycleTone(flow.deployment_intent.status) : "future",
      resource: "DeploymentIntent",
      target: flow.deployment_intent?.id ?? "not created",
      finding: summarizeJson(flow.deployment_intent?.intent_json?.deployment_evidence, "deployment evidence not attached"),
      lastEvent: flow.deployment_intent?.argo_application ?? "planned",
      link: "Argo",
    },
    {
      source: "Registry",
      icon: HardDrives,
      status: flow.registry_evidence ? statusText(flow.registry_evidence.status) : "Missing",
      tone: flow.registry_evidence ? lifecycleTone(flow.registry_evidence.status) : "future",
      resource: "RegistryEvidence",
      target: flow.registry_evidence?.image_ref ?? "not created",
      finding: flow.registry_evidence?.verification_status ?? "supply-chain evidence not attached",
      lastEvent: flow.registry_evidence?.source ?? "planned",
      link: "Image",
    },
  ];
}

function buildEvents(flow) {
  if (!flow?.audit_events?.length) {
    return [];
  }

  return flow.audit_events.slice(-6).map((event) => ({
    kind: event.kind,
    tone: event.kind.includes("audit") ? "audit" : event.kind.includes("gate") ? "policy" : "tool",
    time: formatTimestamp(event.created_at),
    detail: `${event.resource_kind}/${event.resource_id}`,
    resourceKind: event.resource_kind,
    resourceId: event.resource_id,
  }));
}

function formatTimestamp(value) {
  const millis = Number(value);
  if (!Number.isFinite(millis)) {
    return "unknown";
  }
  return new Date(millis).toLocaleTimeString();
}

function badgeForNav(id, data) {
  if (id === "WorkPlans") {
    return data?.workPlans?.length ?? null;
  }
  if (id === "Approvals") {
    return data?.approvals?.filter((approval) => approval.status === "pending").length ?? 0;
  }
  if (id === "Approval Gates") {
    return data?.approvalGates?.filter((gate) => gate.status === "pending").length ?? 0;
  }
  if (id === "Runs") {
    return statusCount(data?.runsSummary?.summary, "running") || null;
  }
  if (id === "Audit") {
    return data?.auditEvents?.length ?? null;
  }
  if (id === "Incidents") {
    return data?.incidents?.length || null;
  }
  if (id === "Remediation Plans") {
    return data?.remediationPlans?.length || null;
  }
  if (id === "Observations") {
    return data?.observations?.length || null;
  }
  return null;
}

function statusCount(summary, status) {
  const bucket = summary?.by_status?.find((item) => item.value === status);
  return bucket?.count ?? 0;
}

function riskTone(risk) {
  if (risk === "critical" || risk === "high") {
    return "high";
  }
  if (risk === "medium") {
    return "medium";
  }
  return "low";
}

function runScopeLabel(scope) {
  if (!scope) {
    return "unscoped";
  }
  return [scope.namespace, scope.repo, scope.branch].filter(Boolean).map(compactId).join(" / ") || "unscoped";
}

function canCancelRun(run) {
  return Boolean(run?.status) && !["completed", "failed", "cancelled"].includes(run.status);
}

function resourceLabel(resource) {
  return [resource?.resource_kind, resource?.resource_name].filter(Boolean).join("/") || resource?.resource_namespace || "not scoped";
}

function approvalActionName(approval) {
  return approval?.action?.action ?? approval?.kind ?? "tool approval";
}

function approvalPreviewPath(approval) {
  return approval?.preview?.path ?? approval?.action?.path ?? "no preview path";
}

function approvalPreviewDiff(approval) {
  return approval?.preview?.diff ?? approval?.action?.diff ?? approval?.summary ?? "No diff preview is available for this approval.";
}

const statusLabels = {
  completed: "Completed",
  healthy: "Healthy",
  running: "Running",
  pending: "Pending",
  blocked: "Blocked",
  future: "Future-backed",
};

function StatusPill({ tone, children }) {
  return <span className={`pill pill-${tone}`}>{children}</span>;
}

function IconButton({ label, children, onClick, active = false }) {
  return (
    <button className={`icon-button ${active ? "is-active" : ""}`} type="button" aria-label={label} title={label} onClick={onClick}>
      {children}
    </button>
  );
}

function AppShell({
  route,
  selectedRunId,
  theme,
  setTheme,
  selectedNode,
  setSelectedNode,
  gateState,
  setGateState,
  toolApprovalState,
  setToolApprovalState,
  actionNotice,
  setActionNotice,
  dashboard,
  scope,
  setScope,
}) {
  const activeView = route.view;
  const routeParam = typeof route.param === "string" ? route.param : null;
  const openRun = (runId) => navigate("Run Detail", String(runId));
  const dashboardData = dashboard.data;
  const topologyNodes = useMemo(() => buildTopology(dashboardData?.flow), [dashboardData?.flow]);
  const liveEvidenceRows = useMemo(() => buildEvidenceRows(dashboardData?.flow), [dashboardData?.flow]);
  const liveEvents = useMemo(() => buildEvents(dashboardData?.flow), [dashboardData?.flow]);

  return (
    <div className={`app theme-${theme}`}>
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark"><ShieldCheck size={24} weight="fill" /></div>
          <div>
            <strong>PHarness</strong>
            <span>SDLC control plane</span>
          </div>
        </div>
        <nav className="nav-list" aria-label="Primary">
          {navItems.map((item) => {
            const Icon = item.icon;
            const active = item.view === activeView;
            const badge = item.view ? badgeForNav(item.id, dashboardData) ?? item.badge : null;
            return (
              <button
                className={`nav-item ${active ? "is-active" : ""}`}
                key={item.id}
                type="button"
                disabled={!item.view}
                onClick={() => item.view && navigate(item.view)}
                title={item.view ? item.id : `${item.id}: planned UI surface`}
              >
                <Icon size={20} />
                <span>{item.id}</span>
                {badge ? <b>{badge}</b> : item.planned ? <em>planned</em> : null}
              </button>
            );
          })}
        </nav>
        <div className="health-card">
          <div className="health-row">
            <span className={`pulse-dot ${dashboard.status === "error" ? "is-error" : ""}`} />
            <div>
              <span>Controller</span>
              <strong>{dashboard.status === "error" ? "Offline" : dashboard.status === "loading" ? "Loading" : "Healthy"}</strong>
            </div>
          </div>
          <div className="health-row muted">
            <Pulse size={18} />
            <div>
              <span>Worker</span>
              <strong>
                {dashboardData?.config?.worker?.enabled
                  ? dashboardData?.config?.worker?.mode ?? "enabled"
                  : "Disabled"}
              </strong>
            </div>
          </div>
          <div className="health-row muted">
            <Cube size={18} />
            <div>
              <span>Flow root</span>
              <strong>{dashboardData?.flowRoot?.kind ?? "none"}</strong>
            </div>
          </div>
        </div>
        <small className="version">v0.12.0</small>
      </aside>

      <main className="workspace">
        <TopBar theme={theme} setTheme={setTheme} dashboard={dashboard} route={route} scope={scope} setScope={setScope} />
        <div className="mode-bar">
          <div className="view-switcher" role="tablist" aria-label="Operator views">
            {[
              ["Flow", FlowArrow],
              ["WorkPlans", Kanban],
              ["Queue", Rows],
              ["Delivery Test", RocketLaunch],
              ...(selectedRunId ? [["Run Detail", FileText]] : []),
              ["Approvals", ShieldWarning],
              ["Approval Gates", ShieldCheck],
              ["Audit", ClockCounterClockwise],
            ].map(([view, Icon]) => (
              <button
                key={view}
                className={activeView === view ? "selected" : ""}
                type="button"
                onClick={() => navigate(view, view === "Run Detail" ? selectedRunId : undefined)}
              >
                <Icon size={17} />
                {view}
              </button>
            ))}
          </div>
          <div className="live-controls">
            <span>{dashboard.status === "error" ? "API offline" : "Auto-refresh"}</span>
            <ToggleRight size={28} weight="fill" className="toggle-on" />
            <span>Last updated: {dashboardData?.loadedAt ?? "not connected"}</span>
            <button className="live-button" type="button" onClick={dashboard.refresh}>
              <span className={dashboard.status === "error" ? "is-error" : ""} /> {dashboard.status === "refreshing" ? "Refreshing" : "Live"}
            </button>
          </div>
        </div>

        <section className="content-shell">
          <section className="primary-panel">
            <ImplementationStrip dashboard={dashboard} />
            {activeView === "Flow" ? (
              <FlowView
                selectedNode={selectedNode}
                setSelectedNode={setSelectedNode}
                dashboard={dashboard}
                topologyNodes={topologyNodes}
                evidenceRows={liveEvidenceRows}
                events={liveEvents}
              />
            ) : activeView === "Queue" ? (
              <QueueView dashboard={dashboard} openRun={openRun} />
            ) : activeView === "Delivery Test" ? (
              <DeliveryTestView refreshDashboard={dashboard.refresh} />
            ) : activeView === "WorkPlans" ? (
              <WorkPlansView dashboard={dashboard} selectedId={routeParam} />
            ) : activeView === "Run Detail" ? (
              <RunDetailView runId={selectedRunId} refreshDashboard={dashboard.refresh} />
            ) : activeView === "Approvals" ? (
              <ToolApprovalsView
                dashboard={dashboard}
                selectedId={routeParam}
                toolApprovalState={toolApprovalState}
                setToolApprovalState={setToolApprovalState}
                actionNotice={actionNotice}
                setActionNotice={setActionNotice}
                openRun={openRun}
              />
            ) : activeView === "Audit" ? (
              <AuditView dashboard={dashboard} openRun={openRun} selectedSearch={routeParam} scope={scope} />
            ) : activeView === "Incidents" ? (
              <IncidentsView dashboard={dashboard} selectedId={routeParam} openRun={openRun} />
            ) : activeView === "Remediation Plans" ? (
              <RemediationPlansView dashboard={dashboard} selectedId={routeParam} />
            ) : activeView === "Observations" ? (
              <ObservationsView dashboard={dashboard} selectedId={routeParam} openRun={openRun} />
            ) : (
              <ApprovalGatesView
                dashboard={dashboard}
                selectedId={routeParam}
                gateState={gateState}
                setGateState={setGateState}
                actionNotice={actionNotice}
                setActionNotice={setActionNotice}
              />
            )}
          </section>
          <Inspector
            selectedNode={selectedNode}
            topologyNodes={topologyNodes}
            flow={dashboardData?.flow}
            pendingToolApprovals={
              dashboardData?.approvals?.filter((approval) => approval.status === "pending").length ?? 0
            }
            actionNotice={actionNotice}
          />
        </section>
      </main>
    </div>
  );
}

function ImplementationStrip({ dashboard }) {
  const worker = dashboard.data?.config?.worker;
  const liveSurfaces = [
    "Flow read model",
    "WorkPlan list",
    "Run queue",
    "Run detail live events",
    worker?.enabled ? `${worker?.mode ?? "model"} worker` : "Worker disabled",
    "Tool approvals",
    "Approval gates",
    "Incidents",
    "Remediation plans",
    "Observations",
    "Audit log",
  ];

  return (
    <section className="implementation-strip" aria-label="Implementation status">
      <div>
        <strong>Live API-backed</strong>
        <span>{liveSurfaces.join(" / ")}</span>
      </div>
      <div>
        <strong>Planned only</strong>
        <span>{plannedCapabilities.slice(0, 5).join(" / ")}</span>
      </div>
    </section>
  );
}

function scopeOptions(data, key) {
  const values = [];
  for (const run of data?.runs ?? []) {
    values.push(run.scope?.[key]);
  }
  for (const approval of data?.approvals ?? []) {
    values.push(approval.scope?.[key]);
  }
  if (key === "namespace") {
    for (const gate of data?.approvalGates ?? []) {
      values.push(gate.resource_namespace);
    }
    for (const plan of data?.workPlans ?? []) {
      values.push(plan.resource_namespace);
    }
  }
  return [...new Set(values.filter(Boolean))].sort();
}

function TopBar({ theme, setTheme, dashboard, route, scope, setScope }) {
  const [search, setSearch] = useState(route.view === "Audit" && typeof route.param === "string" ? route.param : "");
  const namespaceOptions = scopeOptions(dashboard.data, "namespace");
  const repoOptions = scopeOptions(dashboard.data, "repo");
  const branchOptions = scopeOptions(dashboard.data, "branch");

  useEffect(() => {
    if (route.view === "Audit") {
      setSearch(typeof route.param === "string" ? route.param : "");
    }
  }, [route.view, route.param]);

  const updateScope = (key, value) => {
    setScope((current) => ({ ...current, [key]: value }));
  };

  const submitSearch = (event) => {
    event.preventDefault();
    navigate("Audit", search.trim() || undefined);
  };

  return (
    <header className="topbar">
      <div className="scope-group">
        <ScopeValue icon={Stack} label="Environment" value="homelab" />
        <ScopeSelect icon={Cube} label="Namespace" value={scope.namespace} options={namespaceOptions} onChange={(value) => updateScope("namespace", value)} />
        <ScopeSelect icon={Cube} label="Repository" value={scope.repo} options={repoOptions} onChange={(value) => updateScope("repo", value)} />
        <ScopeSelect icon={GitBranch} label="Branch" value={scope.branch} options={branchOptions} onChange={(value) => updateScope("branch", value)} />
        <ScopeSelect
          icon={ShieldCheck}
          label="Impact"
          value={scope.productionImpacting}
          options={[
            { value: "false", label: "Non-production" },
            { value: "true", label: "Production" },
          ]}
          onChange={(value) => updateScope("productionImpacting", value)}
        />
      </div>
      <form className="search" onSubmit={submitSearch}>
        <MagnifyingGlass size={18} />
        <input aria-label="Search audit events" placeholder="Search audit events..." value={search} onChange={(event) => setSearch(event.target.value)} />
        <button type="submit" aria-label="Run audit search" title="Run audit search"><MagnifyingGlass size={16} /></button>
      </form>
      <div className="theme-toggle" aria-label="Theme">
        <IconButton label="Light theme" onClick={() => setTheme("light")} active={theme === "light"}>
          <CircleHalf size={18} />
        </IconButton>
        <button className={theme === "dark" ? "selected" : ""} type="button" onClick={() => setTheme("dark")}>
          <Moon size={16} weight="fill" />
          Dark
        </button>
        <button className={theme === "light" ? "selected" : ""} type="button" onClick={() => setTheme("light")}>Light</button>
      </div>
      <button className="avatar" type="button">WL<span /></button>
    </header>
  );
}

function ScopeValue({ icon: Icon, label, value }) {
  return (
    <div className="scope-select scope-value" title={`${label}: ${value}`}>
      <Icon size={19} />
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function ScopeSelect({ icon: Icon, label, value, options, onChange }) {
  const normalized = options.map((option) => typeof option === "string" ? { value: option, label: option } : option);
  if (value && !normalized.some((option) => option.value === value)) {
    normalized.unshift({ value, label: value });
  }
  return (
    <label className="scope-select" title={`${label}: ${value || "All"}`}>
      <Icon size={19} />
      <span>{label}</span>
      <select aria-label={`${label} scope`} value={value} onChange={(event) => onChange(event.target.value)}>
        <option value="">All</option>
        {normalized.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
      </select>
    </label>
  );
}

function FlowRootPicker({ dashboard }) {
  const changeSets = dashboard.data?.changeSets ?? [];
  const workPlans = dashboard.data?.workPlans ?? [];
  const current = dashboard.data?.flowRoot;
  const value = current ? `${current.kind}:${current.id}` : "";
  const known =
    (current?.kind === "change_set" && changeSets.some((item) => item.id === current.id)) ||
    (current?.kind === "work_plan" && workPlans.some((item) => item.id === current.id));

  if (!changeSets.length && !workPlans.length) {
    return null;
  }

  return (
    <label className="root-picker">
      <span>Root</span>
      <select
        value={value}
        onChange={(event) => {
          const [kind, ...idParts] = event.target.value.split(":");
          navigate("Flow", { kind, id: idParts.join(":") });
        }}
      >
        {!known && current ? (
          <option value={value}>{`${statusText(current.kind)} · ${compactId(current.id)}`}</option>
        ) : null}
        {changeSets.map((item) => (
          <option key={item.id} value={`change_set:${item.id}`}>
            {`ChangeSet · ${compactId(item.id)} · ${statusText(item.status)}`}
          </option>
        ))}
        {workPlans.map((item) => (
          <option key={item.id} value={`work_plan:${item.id}`}>
            {`WorkPlan · ${compactId(item.id)} · ${statusText(item.status)}`}
          </option>
        ))}
      </select>
    </label>
  );
}

function FlowView({ selectedNode, setSelectedNode, dashboard, topologyNodes, evidenceRows, events }) {
  const flow = dashboard.data?.flow;
  const title = flow
    ? `${statusText(flow.resource_kind)} Flow`
    : "SDLC Flow";
  const summary = flow
    ? `${flow.readiness?.summary ?? "Readiness unavailable"} for ${flow.resource_kind}/${flow.resource_id}.`
    : dashboard.status === "error"
      ? "API unavailable. Live SDLC state cannot be loaded."
      : "No SDLC flow records found yet.";

  return (
    <>
      <div className="section-heading">
        <div>
          <h1>{title}</h1>
          <p>{summary}</p>
        </div>
        <div className="legend">
          <FlowRootPicker dashboard={dashboard} />
          <span><i className="dot healthy" /> Healthy</span>
          <span><i className="dot pending" /> Pending</span>
          <span><i className="dot risk" /> Risk</span>
          <span><i className="dot blocked" /> Blocked</span>
          <span><i className="dot running" /> Running</span>
        </div>
      </div>
      {dashboard.error ? <div className="api-banner">API connection failed: {dashboard.error}</div> : null}
      {flow ? (
        <>
          <div className="topology" aria-label="SDLC topology">
            {topologyNodes.map((node, index) => {
              const Icon = node.icon;
              return (
                <div className="topology-step" key={node.id}>
                  <button
                    className={`flow-node node-${node.status} ${selectedNode === node.id ? "is-selected" : ""}`}
                    type="button"
                    title={`${node.label}: ${node.subline} (${node.meta})`}
                    onClick={() => setSelectedNode(node.id)}
                  >
                    <Icon size={26} />
                    <span>{node.label}</span>
                    <StatusPill tone={node.status}>{node.statusLabel}</StatusPill>
                    <strong>{node.subline}</strong>
                    <small>{node.meta}</small>
                  </button>
                  {index < topologyNodes.length - 1 ? <div className={`connector connector-${topologyNodes[index + 1].status}`}><span /></div> : null}
                </div>
              );
            })}
          </div>
          <EvidenceTable rows={evidenceRows} onSelectSource={setSelectedNode} />
          <EventTimeline events={events} />
        </>
      ) : (
        <EmptyState
          title="No live SDLC flow"
          body="The UI did not find a WorkPlan or ChangeSet flow through the API. Run the e2e smoke or create SDLC resources, then refresh."
        />
      )}
    </>
  );
}

const evidenceNodeBySource = {
  WorkPlan: "work-plan",
  ChangeSet: "change-set",
  Pipeline: "pipeline-intent",
  Deployment: "deployment-intent",
  Release: "release",
  Registry: "registry-evidence",
  Readiness: "work-plan",
};

function EvidenceTable({ rows, onSelectSource }) {
  return (
    <section className="evidence">
      <div className="table-heading">
        <div>
          <h2>Evidence & Signals</h2>
          <p>Typed reads that support the selected SDLC state.</p>
        </div>
        <button type="button">Export evidence</button>
      </div>
      <div className="evidence-table">
        <div className="evidence-head">
          <span>Source</span>
          <span>Status</span>
          <span>Resource / Target</span>
          <span>Finding</span>
          <span>Last Event</span>
          <span>Artifact</span>
        </div>
        {rows.map((row) => {
          const Icon = row.icon;
          return (
            <button
              className="evidence-row"
              key={row.source}
              type="button"
              onClick={() => {
                const nodeId = evidenceNodeBySource[row.source];
                if (nodeId && onSelectSource) {
                  onSelectSource(nodeId);
                }
              }}
            >
              <span className="source"><Icon size={23} /> {row.source}</span>
              <span><i className={`dot ${row.tone}`} /> {row.status}</span>
              <span>{row.resource}<strong>{row.target}</strong></span>
              <span>{row.finding}</span>
              <span>{row.lastEvent}</span>
              <span className="link-text">{row.link}</span>
            </button>
          );
        })}
        {!rows.length ? <div className="table-empty">No evidence rows are available for this flow.</div> : null}
      </div>
    </section>
  );
}

function EventTimeline({ events }) {
  return (
    <section className="timeline-wrap">
      <div className="timeline-title">
        <h2>Control-Plane Timeline</h2>
        <div className="event-filters">
          {["Model", "Tool", "Policy", "System", "Audit"].map((label) => (
            <label key={label}>
              <input type="checkbox" defaultChecked />
              {label}
            </label>
          ))}
          <select aria-label="Event filter" defaultValue="all">
            <option value="all">All Events</option>
            <option value="policy">Policy only</option>
            <option value="tools">Tools only</option>
          </select>
        </div>
      </div>
      <div className="timeline">
        {events.length ? (
          events.map((event, index) => {
            const target = navTargetForResource(event.resourceKind, event.resourceId);
            return (
              <button
                className={`event-card event-${event.tone}`}
                key={`${event.kind}-${event.time}-${index}`}
                type="button"
                title={`${event.kind}: ${event.detail}`}
                onClick={() => target && navigate(target[0], target[1])}
              >
                <span className="event-time">{event.time}</span>
                <strong>{event.kind}</strong>
                <p>{event.detail}</p>
              </button>
            );
          })
        ) : (
          <div className="timeline-empty">No audit events are attached to this flow yet.</div>
        )}
      </div>
    </section>
  );
}

function QueueView({ dashboard, openRun }) {
  const liveRuns = dashboard.data?.runs ?? [];
  const summary = dashboard.data?.runsSummary?.summary;
  const workerEnabled = Boolean(dashboard.data?.config?.worker?.enabled);
  const pendingApprovals = dashboard.data?.approvals?.filter((approval) => approval.status === "pending").length ?? 0;
  const pendingGates = dashboard.data?.approvalGates?.filter((gate) => gate.status === "pending").length ?? 0;
  const [task, setTask] = useState("List the top-level files, then finish with one sentence.");
  const [cwd, setCwd] = useState(".");
  const [maxTurns, setMaxTurns] = useState(20);
  const [queueNotice, setQueueNotice] = useState("");
  const metrics = [
    ["Running", String(statusCount(summary, "running")), "active worker"],
    ["Tool approvals", String(pendingApprovals), "execution decisions"],
    ["Approval gates", String(pendingGates), "governance state"],
    ["Completed", String(statusCount(summary, "completed")), "all time"],
  ];

  const handleSubmitRun = async (event) => {
    event.preventDefault();
    const trimmedTask = task.trim();
    if (!trimmedTask) {
      setQueueNotice("Task is required.");
      return;
    }
    if (!workerEnabled) {
      setQueueNotice("Worker is disabled. Start the API with a configured model provider before submitting runs.");
      return;
    }
    setQueueNotice("Submitting run...");
    try {
      const run = await submitRun({ task: trimmedTask, cwd: cwd.trim() || ".", maxTurns });
      setQueueNotice(`Run submitted: ${compactId(String(run.id))}`);
      openRun(run.id);
      await dashboard.refresh();
    } catch (error) {
      setQueueNotice(`Run submit failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const handleCancelRun = async (runId) => {
    setQueueNotice(`Cancelling ${compactId(String(runId))}...`);
    try {
      await cancelRun(String(runId));
      setQueueNotice(`Cancel requested: ${compactId(String(runId))}`);
      await dashboard.refresh();
    } catch (error) {
      setQueueNotice(`Cancel failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  return (
    <section className="queue-view">
      <div className="section-heading">
        <div>
          <h1>Run Queue</h1>
          <p>Same resources as Flow, optimized for triage and stale run cleanup.</p>
        </div>
        <button className="primary-action" type="button" onClick={dashboard.refresh} disabled={dashboard.status === "refreshing"}>
          <ArrowsClockwise size={17} /> {dashboard.status === "refreshing" ? "Refreshing" : "Refresh"}
        </button>
      </div>
      <div className="summary-grid">
        {metrics.map(([label, value, note]) => (
          <div className="metric" key={label}>
            <span>{label}</span>
            <strong>{value}</strong>
            <small>{note}</small>
          </div>
        ))}
      </div>
      <form className="run-submit" onSubmit={handleSubmitRun}>
        {!workerEnabled ? (
          <div className="inline-warning">
            Worker disabled. The API can queue runs, but it will not execute them until a model-backed worker is enabled.
          </div>
        ) : null}
        <label>
          <span>Task</span>
          <textarea value={task} onChange={(event) => setTask(event.target.value)} rows={3} />
        </label>
        <div className="run-submit-grid">
          <label>
            <span>CWD</span>
            <input value={cwd} onChange={(event) => setCwd(event.target.value)} />
          </label>
          <label>
            <span>Max turns</span>
            <input min="1" max="80" type="number" value={maxTurns} onChange={(event) => setMaxTurns(event.target.value)} />
          </label>
          <button className="primary-action" type="submit" disabled={!workerEnabled}><RocketLaunch size={17} /> Submit run</button>
        </div>
        {queueNotice ? <span className="action-notice">{queueNotice}</span> : null}
      </form>
      {liveRuns.length ? (
        <div className="run-list">
          {liveRuns.map((run) => (
            <div className="run-row" key={run.id}>
              <span>
                <strong>{run.task}</strong>
                <small>{compactId(run.id)}</small>
              </span>
              <span>{runScopeLabel(run.scope)}</span>
              <StatusPill tone={run.status === "approval_required" ? "pending" : lifecycleTone(run.status)}>{statusText(run.status)}</StatusPill>
              <span>{run.result?.turns != null ? `${run.result.turns} turns` : "\u2014"}</span>
              <span>{formatTimestamp(run.started_at)}</span>
              <span className="row-actions">
                <button
                  type="button"
                  onClick={() => openRun(run.id)}
                >
                  Open
                </button>
                <button className="deny" type="button" disabled={!canCancelRun(run)} onClick={() => handleCancelRun(run.id)}>Cancel</button>
              </span>
              {!workerEnabled && run.status === "queued" ? <small className="run-warning">worker disabled</small> : null}
            </div>
          ))}
        </div>
      ) : (
        <EmptyState title="No runs yet" body="Submit a run above or from the CLI and it will appear here." />
      )}
    </section>
  );
}

function DeliveryTestView({ refreshDashboard }) {
  const [acknowledged, setAcknowledged] = useState(false);
  const [state, setState] = useState({ phase: "idle", data: null, error: null, detail: null });

  const prepare = async () => {
    setState({ phase: "preparing", data: null, error: null, detail: "Creating the audited delivery chain and validating a dry-run." });
    try {
      const data = await prepareTektonE2eSmoke();
      setState({ phase: "ready", data, error: null, detail: "Preflight passed. No PipelineRun has been created." });
      refreshDashboard();
    } catch (error) {
      setState({ phase: "failed", data: null, error: error instanceof Error ? error.message : String(error), detail: null });
    }
  };

  const dispatch = async () => {
    const pipelineIntentId = state.data?.pipelineIntent?.id;
    if (!pipelineIntentId) {
      return;
    }
    setState((current) => ({ ...current, phase: "dispatching", error: null, detail: "Dispatching the dedicated executor Job." }));
    try {
      const dispatchResult = await dispatchTektonE2eSmoke(pipelineIntentId);
      setState((current) => ({ ...current, phase: "observing", detail: `Executor ${dispatchResult.executor_job_name} dispatched. Waiting for durable terminal evidence.` }));
      for (let attempt = 0; attempt < 80; attempt += 1) {
        const intent = await loadPipelineIntent(pipelineIntentId);
        const execution = intent.execution_evidence;
        if (execution?.status === "completed") {
          setState((current) => ({ ...current, phase: "completed", detail: `PipelineRun ${execution.pipeline_run?.namespace}/${execution.pipeline_run?.name} completed successfully.`, data: { ...current.data, pipelineIntent: intent } }));
          refreshDashboard();
          return;
        }
        if (execution?.status === "failed") {
          throw new Error(execution.error || "The Tekton executor reported failure.");
        }
        await new Promise((resolve) => window.setTimeout(resolve, 3000));
      }
      throw new Error("Timed out waiting for the executor's terminal evidence.");
    } catch (error) {
      setState((current) => ({ ...current, phase: "failed", error: error instanceof Error ? error.message : String(error), detail: null }));
    }
  };

  const busy = ["preparing", "dispatching", "observing"].includes(state.phase);
  const intent = state.data?.pipelineIntent;
  const execution = intent?.execution_evidence;

  return (
    <section className="delivery-test-view">
      <header className="delivery-test-heading">
        <div>
          <span className="eyebrow">Bounded execution</span>
          <h1>Tekton Delivery Test</h1>
          <p>Exercises the real Pharness execution path with one inert Pipeline. It does not read secrets or change finance applications.</p>
        </div>
        <span className={`status-chip ${state.phase === "completed" ? "healthy" : state.phase === "failed" ? "blocked" : "pending"}`}>{statusText(state.phase, "Ready")}</span>
      </header>

      <section className="delivery-test-scope">
        <ReviewItem label="Fixture" value="tekton-pipelines/pharness-e2e-noop" />
        <ReviewItem label="Pipeline inputs" value="No parameters or workspaces" />
        <ReviewItem label="Application impact" value="None" tone="healthy" />
        <ReviewItem label="Evidence" value="Audit chain and terminal PipelineRun receipt" />
      </section>

      <section className="delivery-test-actions">
        <label className="delivery-test-ack">
          <input type="checkbox" checked={acknowledged} onChange={(event) => setAcknowledged(event.target.checked)} disabled={busy || state.phase === "completed"} />
          <span>I understand this creates durable smoke records and, after preflight, one inert PipelineRun.</span>
        </label>
        <div className="delivery-test-buttons">
          <button className="primary-action" type="button" onClick={prepare} disabled={!acknowledged || busy || state.phase === "ready" || state.phase === "completed"}>
            <ShieldCheck size={18} /> {state.phase === "preparing" ? "Preparing" : "Prepare preflight"}
          </button>
          <button className="secondary-action" type="button" onClick={dispatch} disabled={state.phase !== "ready" || !acknowledged}>
            <RocketLaunch size={18} /> Dispatch inert PipelineRun
          </button>
        </div>
        <p className="delivery-test-detail">{state.detail ?? "Preflight is required before the dispatch button becomes available."}</p>
        {state.error ? <div className="api-banner">Delivery test failed: {state.error}</div> : null}
      </section>

      {state.data ? (
        <section className="delivery-test-result">
          <h2>Durable records</h2>
          <div className="review-grid">
            <ReviewItem label="WorkPlan" value={compactId(state.data.workPlan?.id)} />
            <ReviewItem label="ChangeSet" value={compactId(state.data.changeSet?.id)} />
            <ReviewItem label="PipelineIntent" value={compactId(intent?.id)} />
            <ReviewItem label="PipelineContract" value={compactId(state.data.pipelineContract?.id)} />
            <ReviewItem label="Preflight" value={state.data.preview?.ready ? "Passed" : "Blocked"} tone={state.data.preview?.ready ? "healthy" : "blocked"} />
            <ReviewItem label="PipelineRun" value={execution?.pipeline_run ? `${execution.pipeline_run.namespace}/${execution.pipeline_run.name}` : "Not dispatched"} tone={execution?.status === "completed" ? "healthy" : undefined} />
          </div>
          <button className="text-action" type="button" onClick={() => intent?.id && navigate("Flow", { kind: "change_set", id: state.data.changeSet.id })}>Open delivery flow</button>
        </section>
      ) : null}
    </section>
  );
}

function WorkPlansView({ dashboard, selectedId }) {
  const workPlans = dashboard.data?.workPlans ?? [];
  const [detail, setDetail] = useState({ status: "idle", flow: null, error: null });
  const selectedWorkPlan =
    workPlans.find((plan) => plan.id === selectedId) ??
    workPlans[0] ??
    null;
  const statusBuckets = workPlans.reduce((counts, plan) => {
    counts[plan.status] = (counts[plan.status] ?? 0) + 1;
    return counts;
  }, {});
  const highRisk = workPlans.filter((plan) => ["high", "critical"].includes(plan.risk_level)).length;
  const metrics = [
    ["Plans", String(workPlans.length), "latest page"],
    ["Approved", String(statusBuckets.approved ?? 0), "ready for changes"],
    ["Blocked", String(statusBuckets.blocked ?? 0), "needs review"],
    ["High risk", String(highRisk), "operator attention"],
  ];

  useEffect(() => {
    let active = true;
    async function loadSelectedFlow() {
      if (!selectedWorkPlan?.id) {
        setDetail({ status: "idle", flow: null, error: null });
        return;
      }
      setDetail((current) => ({ ...current, status: current.flow ? "refreshing" : "loading", error: null }));
      try {
        const flow = await loadWorkPlanFlow(selectedWorkPlan.id);
        if (active) {
          setDetail({ status: "ready", flow, error: null });
        }
      } catch (error) {
        if (active) {
          setDetail((current) => ({
            status: "error",
            flow: current.flow,
            error: error instanceof Error ? error.message : String(error),
          }));
        }
      }
    }
    loadSelectedFlow();
    return () => {
      active = false;
    };
  }, [selectedWorkPlan?.id]);

  const flow = detail.flow;
  const readiness = flow?.readiness;

  return (
    <section className="workplans-view">
      <div className="section-heading">
        <div>
          <h1>WorkPlans</h1>
          <p>Bounded SDLC plans with live readiness and downstream evidence state.</p>
        </div>
        <button className="primary-action" type="button" onClick={dashboard.refresh} disabled={dashboard.status === "refreshing"}>
          <ArrowsClockwise size={17} /> {dashboard.status === "refreshing" ? "Refreshing" : "Refresh"}
        </button>
      </div>
      <div className="summary-grid">
        {metrics.map(([label, value, note]) => (
          <div className="metric" key={label}>
            <span>{label}</span>
            <strong>{value}</strong>
            <small>{note}</small>
          </div>
        ))}
      </div>
      {workPlans.length ? (
        <div className="workplan-layout">
          <div className="workplan-list">
            {workPlans.map((plan) => (
              <button
                className={`workplan-card ${plan.id === selectedWorkPlan?.id ? "is-active" : ""}`}
                key={plan.id}
                type="button"
                onClick={() => navigate("WorkPlans", plan.id)}
              >
                <span>
                  <StatusPill tone={lifecycleTone(plan.status)}>{statusText(plan.status)}</StatusPill>
                  <b className={`risk-${riskTone(plan.risk_level)}`}>{plan.risk_level}</b>
                </span>
                <strong title={plan.title}>{plan.title}</strong>
                <p>{plan.summary}</p>
                <small title={plan.id}>{compactId(plan.id)} · revision {plan.revision}</small>
              </button>
            ))}
          </div>
          <section className="review-surface">
            <div className="table-heading">
              <div>
                <h2>{selectedWorkPlan?.title ?? "WorkPlan detail"}</h2>
                <p>{selectedWorkPlan?.summary ?? "Select a WorkPlan to inspect its live control-plane flow."}</p>
                {selectedWorkPlan ? (
                  <button
                    className="link-text"
                    type="button"
                    onClick={() => navigate("Flow", { kind: "work_plan", id: selectedWorkPlan.id })}
                  >
                    Open in Flow
                  </button>
                ) : null}
              </div>
              <StatusPill tone={readiness?.ready ? "healthy" : "blocked"}>
                {detail.status === "loading" ? "Loading" : readiness?.ready ? "Ready" : "Blocked"}
              </StatusPill>
            </div>
            {detail.error ? <div className="api-banner">WorkPlan flow failed: {detail.error}</div> : null}
            <div className="review-grid">
              <ReviewItem label="Resource" value={resourceLabel(selectedWorkPlan)} />
              <ReviewItem label="Risk" value={statusText(selectedWorkPlan?.risk_level, "Unknown")} tone={riskTone(selectedWorkPlan?.risk_level) === "high" ? "risk" : undefined} />
              <ReviewItem label="Requires approval" value={String(Boolean(selectedWorkPlan?.requires_approval))} />
              <ReviewItem label="Run" value={compactId(String(selectedWorkPlan?.run_id ?? ""))} />
            </div>
            <WorkPlanFlowSummary flow={flow} status={detail.status} />
          </section>
        </div>
      ) : (
        <EmptyState title="No WorkPlans" body="Create the SDLC root chain from the CLI or smoke script, then refresh this view." />
      )}
    </section>
  );
}

function WorkPlanFlowSummary({ flow, status }) {
  if (!flow) {
    return <EmptyState title="No WorkPlan flow loaded" body={status === "error" ? "The API did not return a flow for this WorkPlan." : "Select a WorkPlan to load its flow."} />;
  }

  const readiness = flow.readiness;
  const downstream = [
    ["ChangeSet", flow.change_set?.status, flow.change_set?.id],
    ["PipelineIntent", flow.pipeline_intent?.status, flow.pipeline_intent?.id],
    ["DeploymentIntent", flow.deployment_intent?.status, flow.deployment_intent?.id],
    ["Release", flow.release?.status, flow.release?.id],
    ["RegistryEvidence", flow.registry_evidence?.status, flow.registry_evidence?.image_ref],
  ];

  return (
    <>
      <section className="workplan-readiness">
        <div>
          <span>Readiness</span>
          <strong>{readiness?.summary ?? "readiness unavailable"}</strong>
        </div>
        <div>
          <span>Blockers</span>
          <strong>{readiness?.blockers?.length ?? 0}</strong>
        </div>
        <div>
          <span>Warnings</span>
          <strong>{readiness?.warnings?.length ?? 0}</strong>
        </div>
      </section>
      <div className="downstream-list">
        {downstream.map(([label, statusValue, target]) => (
          <div key={label}>
            <span>{label}</span>
            <StatusPill tone={statusValue ? lifecycleTone(statusValue) : "future"}>{statusText(statusValue, "Missing")}</StatusPill>
            <strong title={target ?? "not created"}>{target ? compactId(String(target)) : "not created"}</strong>
          </div>
        ))}
      </div>
      <ReadinessFacts readiness={readiness} />
    </>
  );
}

function RunDetailView({ runId, refreshDashboard }) {
  const [state, setState] = useState({ status: runId ? "loading" : "empty", detail: null, error: null });
  const [reloadToken, setReloadToken] = useState(0);
  const [streamState, setStreamState] = useState({ status: "idle", error: null });
  const [runNotice, setRunNotice] = useState(null);
  const streamRunIdRef = useRef(null);
  const [streamCursor, setStreamCursor] = useState(null);

  useEffect(() => {
    streamRunIdRef.current = null;
    setStreamCursor(null);
    setStreamState({ status: "idle", error: null });
    setRunNotice(null);
  }, [runId]);

  useEffect(() => {
    let active = true;
    async function load() {
      if (!runId) {
        setState({ status: "empty", detail: null, error: null });
        return;
      }
      setState((current) => ({ ...current, status: current.detail ? "refreshing" : "loading" }));
      try {
        const detail = await loadRunDetail(runId);
        if (active) {
          setState({ status: "ready", detail, error: null });
          if (streamRunIdRef.current !== runId) {
            streamRunIdRef.current = runId;
            if (isTerminalStatus(detail.run?.status)) {
              setStreamState({ status: "closed", error: null });
              setStreamCursor(null);
            } else {
              setStreamCursor(latestEventSeq(detail.events));
            }
          }
        }
      } catch (error) {
        if (active) {
          setState((current) => ({
            status: "error",
            detail: current.detail,
            error: error instanceof Error ? error.message : String(error),
          }));
        }
      }
    }
    load();
    return () => {
      active = false;
    };
  }, [runId, reloadToken]);

  useEffect(() => {
    if (!runId) {
      setStreamState({ status: "idle", error: null });
      return undefined;
    }
    if (streamCursor === null || streamRunIdRef.current !== runId) {
      return undefined;
    }

    setStreamState({ status: "connecting", error: null });
    let closeStream = () => {};
    closeStream = subscribeRunEvents(runId, {
      afterSeq: streamCursor,
      onEvent: (event) => {
        setStreamState({ status: isTerminalEvent(event) ? "closed" : "live", error: null });
        setState((current) => ({
          ...current,
          detail: mergeRunEvent(current.detail, runId, event),
        }));
        if (eventShouldRefreshRunDetail(event)) {
          setReloadToken((value) => value + 1);
        }
        if (isTerminalEvent(event)) {
          closeStream();
        }
      },
      onError: (error) => {
        setStreamState({ status: "error", error: error.message });
      },
    });

    return closeStream;
  }, [runId, streamCursor]);

  const detail = state.detail;
  const run = detail?.run;
  const result = run?.result ?? {};
  const events = detail?.events ?? [];
  const changes = detail?.diff?.changes ?? [];
  const artifacts = detail?.artifacts ?? [];
  const cancelAllowed = canCancelRun(run);

  const handleCancelSelectedRun = async () => {
    if (!runId || !cancelAllowed) {
      return;
    }
    setRunNotice(`Cancelling ${compactId(runId)}...`);
    try {
      await cancelRun(runId);
      setRunNotice(`Cancel requested: ${compactId(runId)}`);
      setReloadToken((value) => value + 1);
      await refreshDashboard?.();
    } catch (error) {
      setRunNotice(`Cancel failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  if (!runId) {
    return (
      <EmptyState
        title="No run selected"
        body="Open a run from the Queue view to inspect events, diffs, artifacts, and final result JSON."
      />
    );
  }

  return (
    <section className="run-detail-view">
      <div className="section-heading">
        <div>
          <h1>Run Detail</h1>
          <p>{run?.task ?? `Loading ${compactId(runId)}...`}</p>
        </div>
        <div className="detail-actions">
          <span className={`stream-chip stream-${streamState.status}`}>
            <i className={`dot ${streamState.status === "error" ? "blocked" : streamState.status === "live" ? "running" : "future"}`} />
            {streamLabel(streamState)}
          </span>
          <button className="primary-action" type="button" onClick={() => navigate("Queue")}><Rows size={17} /> Queue</button>
          <button className="primary-action" type="button" onClick={() => setReloadToken((value) => value + 1)}><ArrowsClockwise size={17} /> Reload</button>
          <button className="primary-action deny" type="button" disabled={!cancelAllowed} onClick={handleCancelSelectedRun}><X size={17} /> Cancel</button>
        </div>
      </div>
      {state.error ? <div className="api-banner">Run detail failed: {state.error}</div> : null}
      {streamState.status === "error" ? <div className="api-banner">Event stream: {streamState.error}</div> : null}
      {runNotice ? <span className="action-notice">{runNotice}</span> : null}
      <div className="run-detail-grid">
        <ReviewItem label="Run" value={compactId(runId)} />
        <ReviewItem label="Status" value={statusText(run?.status, state.status)} tone={run?.status === "failed" ? "risk" : run?.status === "approval_required" ? "pending" : undefined} />
        <ReviewItem label="Turns" value={result.turns ?? "unknown"} />
        <ReviewItem label="Scope" value={runScopeLabel(run?.scope ?? result.run_scope)} />
      </div>
      <StreamStatusPanel streamState={streamState} eventCount={events.length} cursor={streamCursor} run={run} />
      <section className="review-surface">
        <div className="table-heading">
          <div>
            <h2>Result</h2>
            <p>Structured final result returned by the run.</p>
          </div>
          <StatusPill tone={lifecycleTone(result.status ?? run?.status)}>{statusText(result.status ?? run?.status, state.status)}</StatusPill>
        </div>
        <p>{result.summary ?? result.error ?? "No result summary has been recorded yet."}</p>
      </section>
      <section className="run-detail-layout">
        <RunEvents events={events} />
        <RunDiff diff={detail?.diff} changes={changes} />
      </section>
      <RunArtifacts artifacts={artifacts} />
    </section>
  );
}

function StreamStatusPanel({ streamState, eventCount, cursor, run }) {
  const status = streamState.status;
  const rows = [
    ["Source", "API events/stream"],
    ["Replay cursor", cursor === null ? "terminal snapshot" : `after seq ${cursor}`],
    ["Durable events", String(eventCount)],
    ["Run state", statusText(run?.status, "loading")],
  ];

  return (
    <section className={`stream-status-panel stream-panel-${status}`}>
      <div>
        <strong>{streamLabel(streamState)}</strong>
        <span>{streamDescription(streamState)}</span>
      </div>
      <div className="stream-facts">
        {rows.map(([label, value]) => (
          <span key={label}>
            <small>{label}</small>
            <b>{value}</b>
          </span>
        ))}
      </div>
    </section>
  );
}

function RunEvents({ events }) {
  return (
    <section className="review-surface">
      <div className="table-heading">
        <div>
          <h2>Events</h2>
          <p>Durable event log for replaying the run.</p>
        </div>
        <strong className="counter-label">{events.length}</strong>
      </div>
      {events.length ? (
        <div className="event-list">
          {events.map((event) => (
            <div className="event-list-row" key={event.event_id ?? `${event.seq}-${event.type}`}>
              <span>{event.seq}</span>
              <i className={`dot ${eventTone(event.type)}`} />
              <strong>{event.type}</strong>
              <p>{eventPayloadSummary(event.payload)}</p>
            </div>
          ))}
        </div>
      ) : (
        <EmptyState title="No events" body="No durable events have been recorded for this run yet." />
      )}
    </section>
  );
}

function RunDiff({ diff, changes }) {
  return (
    <section className="review-surface">
      <div className="table-heading">
        <div>
          <h2>Diff</h2>
          <p>File changes persisted for this run.</p>
        </div>
        <strong className="counter-label">{changes.length}</strong>
      </div>
      {changes.length ? (
        <div className="change-list">
          {changes.map((change) => (
            <div className="change-card" key={change.id}>
              <strong>{change.path}</strong>
              <small>{formatTimestamp(change.created_at)}</small>
              <pre>{change.diff}</pre>
            </div>
          ))}
        </div>
      ) : (
        <div className="diff-box">
          <div><FileText size={18} /> No file changes</div>
          <pre>{diff?.diff || "This run did not persist a file diff."}</pre>
        </div>
      )}
    </section>
  );
}

function RunArtifacts({ artifacts }) {
  return (
    <section className="review-surface">
      <div className="table-heading">
        <div>
          <h2>Artifacts</h2>
          <p>Observation and tool artifacts recorded by the runtime.</p>
        </div>
        <strong className="counter-label">{artifacts.length}</strong>
      </div>
      {artifacts.length ? (
        <div className="artifact-grid">
          {artifacts.map((artifact) => (
            <div className="artifact-card" key={artifact.id}>
              <span>{artifact.kind}</span>
              <strong>{artifact.label}</strong>
              <small>{artifact.mime_type ?? artifact.path ?? compactId(artifact.id)}</small>
              <p>{artifactSummary(artifact)}</p>
            </div>
          ))}
        </div>
      ) : (
        <EmptyState title="No artifacts" body="Read-only file-listing runs often have no artifacts. Cluster, Tekton, Argo, Prometheus, and Loki reads should appear here." />
      )}
    </section>
  );
}

function ToolApprovalsView({
  dashboard,
  selectedId,
  toolApprovalState,
  setToolApprovalState,
  actionNotice,
  setActionNotice,
  openRun,
}) {
  const allApprovals = dashboard.data?.approvals ?? [];
  const pendingCount = allApprovals.filter((approval) => approval.status === "pending").length;
  const routeSelected = allApprovals.find((approval) => approval.id === selectedId) ?? null;
  const [approvalFilter, setApprovalFilter] = useState(
    routeSelected && routeSelected.status !== "pending" ? "all" : "pending",
  );
  const approvals =
    approvalFilter === "pending"
      ? allApprovals.filter((approval) => approval.status === "pending")
      : allApprovals;
  const selectedApproval =
    routeSelected ??
    approvals.find((approval) => approval.status === "pending") ??
    approvals[0];

  const decideToolApproval = async (decision) => {
    if (!selectedApproval) {
      return;
    }
    setActionNotice(`Deciding tool approval ${compactId(selectedApproval.id)}...`);
    try {
      await decideApproval(selectedApproval.id, decision);
      setToolApprovalState(decision);
      setActionNotice(`Tool approval ${decision}: ${approvalActionName(selectedApproval)}`);
      await dashboard.refresh();
    } catch (error) {
      setActionNotice(`Tool approval failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const openApprovalRun = () => {
    if (!selectedApproval?.run_id) {
      return;
    }
    openRun(selectedApproval.run_id);
  };

  return (
    <section className="gate-view">
      <div className="section-heading">
        <div>
          <h1>Tool Approvals</h1>
          <p>Execution decisions for proposed tool actions. These authorize or deny a paused run action.</p>
        </div>
        <div className="detail-actions">
          <StatusPill tone={pendingCount ? "pending" : "healthy"}>{pendingCount} pending</StatusPill>
          <div className="filter-row" role="tablist" aria-label="Approval status filter">
            {["pending", "all"].map((option) => (
              <button
                key={option}
                type="button"
                className={approvalFilter === option ? "selected" : ""}
                onClick={() => setApprovalFilter(option)}
              >
                {option}
              </button>
            ))}
          </div>
          <button className="primary-action" type="button" onClick={dashboard.refresh} disabled={dashboard.status === "refreshing"}>
            <ArrowsClockwise size={17} /> {dashboard.status === "refreshing" ? "Refreshing" : "Refresh"}
          </button>
        </div>
      </div>
      {actionNotice ? <span className="action-notice">{actionNotice}</span> : null}
      {approvals.length ? (
        <div className="gate-layout">
          <div className="approval-stack">
            {approvals.map((approval) => (
              <button
                className={`approval-card ${approval.id === selectedApproval?.id ? "is-active" : ""}`}
                type="button"
                key={approval.id}
                onClick={() => navigate("Approvals", approval.id)}
              >
                <span>{approval.kind} · {statusText(approval.status)}</span>
                <strong>{approval.summary}</strong>
                <small>{compactId(approval.id)} · {compactId(String(approval.run_id))}</small>
                <b>{approval.risk_level}</b>
              </button>
            ))}
          </div>
          <div className="review-surface">
            <h2>Approve {approvalActionName(selectedApproval)}</h2>
            <p>This decision resumes or blocks the current run action. It is not a release gate and does not satisfy governance state.</p>
            <div className="review-grid">
              <ReviewItem label="Tool action" value={approvalActionName(selectedApproval)} />
              <ReviewItem label="Run" value={compactId(String(selectedApproval?.run_id))} />
              <ReviewItem label="Risk" value={statusText(selectedApproval?.risk_level, "Unknown")} tone={riskTone(selectedApproval?.risk_level) === "high" ? "risk" : "pending"} />
              <ReviewItem label="Status" value={statusText(selectedApproval?.status)} tone={selectedApproval?.status === "pending" ? "pending" : undefined} />
              <ReviewItem label="Requested" value={formatTimestamp(selectedApproval?.requested_at)} />
              {selectedApproval?.decided_at ? (
                <ReviewItem label="Decided" value={`${selectedApproval?.decided_by ?? "unknown"} · ${formatTimestamp(selectedApproval?.decided_at)}`} />
              ) : null}
              {selectedApproval?.decision_reason ? (
                <ReviewItem label="Reason" value={selectedApproval.decision_reason} />
              ) : null}
            </div>
            <div className="diff-box">
              <div><FileText size={18} /> {approvalPreviewPath(selectedApproval)}</div>
              <pre>{approvalPreviewDiff(selectedApproval)}</pre>
            </div>
            <div className="decision-row">
              <button className="approve" type="button" disabled={selectedApproval?.status !== "pending"} onClick={() => decideToolApproval("approved")}><CheckCircle size={18} /> Approve</button>
              <button className="deny" type="button" disabled={selectedApproval?.status !== "pending"} onClick={() => decideToolApproval("denied")}><X size={18} /> Deny</button>
              <button type="button" disabled={!selectedApproval?.run_id} onClick={openApprovalRun}><FileText size={18} /> Open run</button>
            </div>
          </div>
        </div>
      ) : approvalFilter === "pending" && allApprovals.length ? (
        <EmptyState title="No pending tool approvals" body="Decided approvals are available under the all filter." />
      ) : (
        <EmptyState title="No tool approvals pending" body="Paused write, shell, and network actions will appear here when a run requests human review." />
      )}
    </section>
  );
}

function ApprovalGatesView({ dashboard, selectedId, gateState, setGateState, actionNotice, setActionNotice }) {
  const allGates = dashboard.data?.approvalGates ?? [];
  const pendingGateCount = allGates.filter((gate) => gate.status === "pending").length;
  const routeSelected = allGates.find((gate) => gate.id === selectedId) ?? null;
  const [gateFilter, setGateFilter] = useState(
    routeSelected && routeSelected.status !== "pending" ? "all" : "pending",
  );
  const gates =
    gateFilter === "pending"
      ? allGates.filter((gate) => gate.status === "pending")
      : allGates;
  const gateGroups = gates.reduce((groups, gate) => {
    const key = gate.remediation_plan_id ?? "ungrouped";
    (groups[key] = groups[key] ?? []).push(gate);
    return groups;
  }, {});
  const selectedGate =
    routeSelected ??
    gates.find((gate) => gate.status === "pending") ??
    gates[0];

  const decideGate = async (decision) => {
    if (!selectedGate) {
      return;
    }
    setActionNotice(`Deciding approval gate ${compactId(selectedGate.id)}...`);
    try {
      await decideApprovalGate(selectedGate.id, decision);
      setGateState(decision);
      setActionNotice(`Approval gate ${decision}: ${selectedGate.title}`);
      await dashboard.refresh();
    } catch (error) {
      setActionNotice(`Approval gate decision failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  return (
    <section className="gate-view">
      <div className="section-heading">
        <div>
          <h1>Approval Gates</h1>
          <p>Governance and release-state review. Gates do not authorize tool execution by themselves.</p>
        </div>
        <div className="detail-actions">
          <StatusPill tone={pendingGateCount ? "pending" : "healthy"}>{pendingGateCount} pending</StatusPill>
          <div className="filter-row" role="tablist" aria-label="Gate status filter">
            {["pending", "all"].map((option) => (
              <button
                key={option}
                type="button"
                className={gateFilter === option ? "selected" : ""}
                onClick={() => setGateFilter(option)}
              >
                {option}
              </button>
            ))}
          </div>
          <button className="primary-action" type="button" onClick={dashboard.refresh} disabled={dashboard.status === "refreshing"}>
            <ArrowsClockwise size={17} /> {dashboard.status === "refreshing" ? "Refreshing" : "Refresh"}
          </button>
        </div>
      </div>
      {actionNotice ? <span className="action-notice">{actionNotice}</span> : null}
      {gates.length ? (
        <div className="gate-layout">
          <div className="approval-stack">
            {Object.entries(gateGroups).map(([planId, planGates]) => (
              <div className="gate-group" key={planId}>
                <button
                  className="gate-group-title"
                  type="button"
                  title={planId}
                  onClick={() => planId !== "ungrouped" && navigate("Remediation Plans", planId)}
                >
                  plan {compactId(planId)} · {planGates.length} gate{planGates.length === 1 ? "" : "s"}
                </button>
                {planGates.map((gate) => (
                  <button
                    className={`approval-card ${gate.id === selectedGate?.id ? "is-active" : ""}`}
                    type="button"
                    key={gate.id}
                    onClick={() => navigate("Approval Gates", gate.id)}
                  >
                    <span>{gate.gate_kind} · {statusText(gate.status)}</span>
                    <strong>{gate.title}</strong>
                    <small>{resourceLabel(gate)} · {compactId(gate.id)}</small>
                    <b>{gate.risk_level}</b>
                  </button>
                ))}
              </div>
            ))}
          </div>
          <div className="review-surface">
            <h2>{selectedGate?.title ?? "Approval gate"}</h2>
            <p>{selectedGate?.summary ?? "Governance state for the selected SDLC resource. Satisfy, waive, or reject this gate after evidence review."}</p>
            <div className="review-grid">
              <ReviewItem label="Status" value={statusText(selectedGate?.status)} tone={selectedGate?.status === "pending" ? "pending" : undefined} />
              <ReviewItem label="Risk" value={statusText(selectedGate?.risk_level, "Unknown")} tone={riskTone(selectedGate?.risk_level) === "high" ? "risk" : "pending"} />
              <ReviewItem label="Gate kind" value={selectedGate?.gate_kind ?? "unknown"} />
              <ReviewItem label="Gate order" value={selectedGate?.gate_order ?? "unknown"} />
              <ReviewItem label="Resource" value={resourceLabel(selectedGate)} />
              <ReviewItem label="Requested" value={formatTimestamp(selectedGate?.created_at)} />
              <ReviewItem
                label="Remediation plan"
                value={
                  <button className="link-text" type="button" onClick={() => navigate("Remediation Plans", selectedGate?.remediation_plan_id)}>
                    {compactId(selectedGate?.remediation_plan_id)}
                  </button>
                }
              />
              <ReviewItem
                label="Incident"
                value={
                  <button className="link-text" type="button" onClick={() => navigate("Incidents", selectedGate?.incident_id)}>
                    {compactId(selectedGate?.incident_id)}
                  </button>
                }
              />
              {selectedGate?.decided_at ? (
                <ReviewItem label="Decided" value={`${selectedGate?.decided_by ?? "unknown"} · ${formatTimestamp(selectedGate?.decided_at)}`} />
              ) : null}
              {selectedGate?.decision_reason ? (
                <ReviewItem label="Reason" value={selectedGate.decision_reason} />
              ) : null}
              {selectedGate?.stale_at ? (
                <ReviewItem label="Stale" value={`${selectedGate?.stale_reason ?? "superseded"} · ${formatTimestamp(selectedGate?.stale_at)}`} tone="pending" />
              ) : null}
            </div>
            <div className="diff-box">
              <div><FileText size={18} /> gate payload · plan {compactId(selectedGate?.remediation_plan_id)}</div>
              <pre>{JSON.stringify(selectedGate?.gate_json ?? {}, null, 2)}</pre>
            </div>
            <div className="decision-row">
              <button className="approve" type="button" disabled={selectedGate?.status !== "pending"} onClick={() => decideGate("satisfied")}><CheckCircle size={18} /> Satisfy</button>
              <button className="waive" type="button" disabled={selectedGate?.status !== "pending"} onClick={() => decideGate("waived")}><ShieldWarning size={18} /> Waive</button>
              <button className="deny" type="button" disabled={selectedGate?.status !== "pending"} onClick={() => decideGate("rejected")}><X size={18} /> Reject</button>
            </div>
          </div>
        </div>
      ) : gateFilter === "pending" && allGates.length ? (
        <EmptyState title="No pending approval gates" body="Decided and stale gates are available under the all filter." />
      ) : (
        <EmptyState title="No approval gates" body="Release, deployment, and remediation gates will appear here when governance state exists." />
      )}
    </section>
  );
}

function navTargetForResource(resourceKind, resourceId) {
  const targets = {
    run: ["Run Detail", String(resourceId)],
    approval: ["Approvals", resourceId],
    approval_gate: ["Approval Gates", resourceId],
    remediation_plan: ["Remediation Plans", resourceId],
    incident: ["Incidents", resourceId],
    observation: ["Observations", resourceId],
    work_plan: ["Flow", { kind: "work_plan", id: resourceId }],
    change_set: ["Flow", { kind: "change_set", id: resourceId }],
  };
  return targets[resourceKind] ?? null;
}

function AuditView({ dashboard, openRun, selectedSearch, scope }) {
  const emptyFilters = { search: selectedSearch ?? "", kind: "", actor: "", resourceKind: "", resourceId: "", runId: "" };
  const [draftFilters, setDraftFilters] = useState(emptyFilters);
  const [filters, setFilters] = useState(emptyFilters);
  const [state, setState] = useState({ status: "loading", events: [], error: null });
  const [reloadToken, setReloadToken] = useState(0);

  useEffect(() => {
    const search = selectedSearch ?? "";
    setDraftFilters((current) => ({ ...current, search }));
    setFilters((current) => ({ ...current, search }));
  }, [selectedSearch]);

  useEffect(() => {
    let active = true;
    setState((current) => ({ ...current, status: current.events.length ? "refreshing" : "loading", error: null }));
    loadAuditEvents(filters, scope)
      .then((events) => {
        if (active) {
          setState({ status: "ready", events, error: null });
        }
      })
      .catch((error) => {
        if (active) {
          setState((current) => ({ ...current, status: "error", error: error instanceof Error ? error.message : String(error) }));
        }
      });
    return () => {
      active = false;
    };
  }, [filters, scope, reloadToken]);

  const events = state.events;
  const latest = events[0];
  const resourceKinds = new Set(events.map((event) => event.resource_kind).filter(Boolean));
  const kindOptions = [...new Set((dashboard.data?.auditEvents ?? []).map((event) => event.kind).filter(Boolean))].sort();
  const actorOptions = [...new Set((dashboard.data?.auditEvents ?? []).map((event) => event.actor).filter(Boolean))].sort();
  const resourceKindOptions = [...new Set((dashboard.data?.auditEvents ?? []).map((event) => event.resource_kind).filter(Boolean))].sort();
  const runLinked = events.filter((event) => event.run_id).length;
  const metrics = [
    ["Events", String(events.length), "latest page"],
    ["Kinds", String(resourceKinds.size), "resource classes"],
    ["Run-linked", String(runLinked), "execution context"],
    ["Latest", latest ? formatTimestamp(latest.created_at) : "none", "audit time"],
  ];

  return (
    <section className="audit-view">
      <div className="section-heading">
        <div>
          <h1>Audit</h1>
          <p>Durable control-plane events from policy, approvals, grants, evidence, and SDLC state changes.</p>
        </div>
        <button className="primary-action" type="button" onClick={() => setReloadToken((value) => value + 1)} disabled={state.status === "refreshing"}>
          <ArrowsClockwise size={17} /> {state.status === "refreshing" ? "Refreshing" : "Refresh"}
        </button>
      </div>
      <div className="summary-grid">
        {metrics.map(([label, value, note]) => (
          <div className="metric" key={label}>
            <span>{label}</span>
            <strong>{value}</strong>
            <small>{note}</small>
          </div>
        ))}
      </div>
      <form
        className="audit-filters"
        onSubmit={(event) => {
          event.preventDefault();
          setFilters({ ...draftFilters });
        }}
      >
        <label className="audit-search-field">
          <span>Search</span>
          <div><MagnifyingGlass size={16} /><input value={draftFilters.search} onChange={(event) => setDraftFilters((current) => ({ ...current, search: event.target.value }))} placeholder="Event, actor, resource, payload..." /></div>
        </label>
        <AuditFilterSelect label="Kind" value={draftFilters.kind} options={kindOptions} onChange={(kind) => setDraftFilters((current) => ({ ...current, kind }))} />
        <AuditFilterSelect label="Resource" value={draftFilters.resourceKind} options={resourceKindOptions} onChange={(resourceKind) => setDraftFilters((current) => ({ ...current, resourceKind }))} />
        <AuditFilterSelect label="Actor" value={draftFilters.actor} options={actorOptions} onChange={(actor) => setDraftFilters((current) => ({ ...current, actor }))} />
        <label>
          <span>Run ID</span>
          <input value={draftFilters.runId} onChange={(event) => setDraftFilters((current) => ({ ...current, runId: event.target.value }))} placeholder="run_..." />
        </label>
        <div className="audit-filter-actions">
          <button className="primary-action" type="submit"><MagnifyingGlass size={16} /> Apply</button>
          <button
            type="button"
            onClick={() => {
              const cleared = { search: "", kind: "", actor: "", resourceKind: "", resourceId: "", runId: "" };
              setDraftFilters(cleared);
              setFilters(cleared);
              if (selectedSearch) {
                navigate("Audit");
              }
            }}
          >
            Clear
          </button>
        </div>
      </form>
      {state.error ? <div className="api-banner">Audit query failed: {state.error}</div> : null}
      {events.length ? (
        <div className="audit-list">
          <div className="audit-head">
            <span>Event</span>
            <span>Resource</span>
            <span>Actor</span>
            <span>Run</span>
            <span>Payload</span>
            <span>Time</span>
          </div>
          {events.map((event) => {
            const target = navTargetForResource(event.resource_kind, event.resource_id);
            return (
              <div className="audit-row" key={event.id}>
                <span>
                  <i className={`dot ${eventTone(event.kind)}`} />
                  <strong title={event.kind}>{event.kind}</strong>
                </span>
                <span title={`${event.resource_kind}/${event.resource_id}`}>
                  {target ? (
                    <button className="link-text" type="button" onClick={() => navigate(target[0], target[1])}>
                      {event.resource_kind}/{compactId(event.resource_id)}
                    </button>
                  ) : (
                    <>{event.resource_kind}/{compactId(event.resource_id)}</>
                  )}
                </span>
                <span>{event.actor ?? "system"}</span>
                <span>
                  {event.run_id ? (
                    <button className="link-text" type="button" onClick={() => openRun(event.run_id)}>
                      {compactId(String(event.run_id))}
                    </button>
                  ) : (
                    "none"
                  )}
                </span>
                <details className="audit-payload">
                  <summary title={JSON.stringify(event.payload ?? {})}>{eventPayloadSummary(event.payload)}</summary>
                  <pre>{JSON.stringify(event.payload ?? {}, null, 2)}</pre>
                </details>
                <span>{formatTimestamp(event.created_at)}</span>
              </div>
            );
          })}
        </div>
      ) : (
        <EmptyState title="No matching audit events" body="Clear filters or generate control-plane activity, then run the query again." />
      )}
    </section>
  );
}

function AuditFilterSelect({ label, value, options, onChange }) {
  return (
    <label>
      <span>{label}</span>
      <select value={value} onChange={(event) => onChange(event.target.value)}>
        <option value="">All</option>
        {options.map((option) => <option key={option} value={option}>{option}</option>)}
      </select>
    </label>
  );
}

function IncidentsView({ dashboard, selectedId, openRun }) {
  const incidents = dashboard.data?.incidents ?? [];
  const plans = dashboard.data?.remediationPlans ?? [];
  const selected = incidents.find((incident) => incident.id === selectedId) ?? incidents[0] ?? null;
  const linkedPlan = selected ? plans.find((plan) => plan.incident_id === selected.id) : null;
  const highSeverity = incidents.filter((incident) => ["high", "critical"].includes(incident.severity)).length;
  const candidates = incidents.filter((incident) => incident.status === "candidate").length;
  const reasons = selected?.data_json?.reasons;
  const metrics = [
    ["Incidents", String(incidents.length), "latest page"],
    ["Candidates", String(candidates), "awaiting triage"],
    ["High severity", String(highSeverity), "operator attention"],
    ["Run-linked", String(incidents.filter((incident) => incident.run_id).length), "execution context"],
  ];

  return (
    <section className="gate-view">
      <div className="section-heading">
        <div>
          <h1>Incidents</h1>
          <p>Read-only incident candidates derived from Tekton and Release observability evidence.</p>
        </div>
        <button className="primary-action" type="button" onClick={dashboard.refresh} disabled={dashboard.status === "refreshing"}>
          <ArrowsClockwise size={17} /> {dashboard.status === "refreshing" ? "Refreshing" : "Refresh"}
        </button>
      </div>
      <div className="summary-grid">
        {metrics.map(([label, value, note]) => (
          <div className="metric" key={label}>
            <span>{label}</span>
            <strong>{value}</strong>
            <small>{note}</small>
          </div>
        ))}
      </div>
      {incidents.length ? (
        <div className="gate-layout">
          <div className="approval-stack">
            {incidents.map((incident) => (
              <button
                className={`approval-card ${incident.id === selected?.id ? "is-active" : ""}`}
                type="button"
                key={incident.id}
                onClick={() => navigate("Incidents", incident.id)}
              >
                <span>{incident.severity} · {statusText(incident.status)}</span>
                <strong>{incident.title}</strong>
                <small>{resourceLabel(incident)} · {compactId(incident.id)}</small>
                <b>{incident.severity}</b>
              </button>
            ))}
          </div>
          <div className="review-surface">
            <h2>{selected?.title ?? "Incident"}</h2>
            <p>{selected?.summary ?? "Select an incident to review its evidence."}</p>
            <div className="review-grid">
              <ReviewItem label="Status" value={statusText(selected?.status)} tone={selected?.status === "candidate" ? "pending" : undefined} />
              <ReviewItem label="Severity" value={statusText(selected?.severity, "Unknown")} tone={riskTone(selected?.severity) === "high" ? "risk" : "pending"} />
              <ReviewItem label="Resource" value={resourceLabel(selected)} />
              <ReviewItem label="Created" value={formatTimestamp(selected?.created_at)} />
              <ReviewItem
                label="Observation"
                value={
                  <button className="link-text" type="button" onClick={() => navigate("Observations", selected?.observation_id)}>
                    {compactId(selected?.observation_id)}
                  </button>
                }
              />
              {linkedPlan ? (
                <ReviewItem
                  label="Remediation plan"
                  value={
                    <button className="link-text" type="button" onClick={() => navigate("Remediation Plans", linkedPlan.id)}>
                      {compactId(linkedPlan.id)}
                    </button>
                  }
                />
              ) : null}
              {selected?.run_id ? (
                <ReviewItem
                  label="Run"
                  value={
                    <button className="link-text" type="button" onClick={() => openRun(selected.run_id)}>
                      {compactId(String(selected.run_id))}
                    </button>
                  }
                />
              ) : null}
            </div>
            <div className="diff-box">
              <div><Siren size={18} /> incident evidence</div>
              <pre>{JSON.stringify(Array.isArray(reasons) ? { reasons } : selected?.data_json ?? {}, null, 2)}</pre>
            </div>
          </div>
        </div>
      ) : (
        <EmptyState title="No incidents" body="Incident candidates appear when attached Tekton or Release observability evidence needs attention." />
      )}
    </section>
  );
}

function RemediationPlansView({ dashboard, selectedId }) {
  const plans = dashboard.data?.remediationPlans ?? [];
  const gates = dashboard.data?.approvalGates ?? [];
  const selected = plans.find((plan) => plan.id === selectedId) ?? plans[0] ?? null;
  const linkedGates = selected ? gates.filter((gate) => gate.remediation_plan_id === selected.id) : [];
  const steps = Array.isArray(selected?.plan_json?.steps) ? selected.plan_json.steps : [];
  const metrics = [
    ["Plans", String(plans.length), "latest page"],
    ["Drafts", String(plans.filter((plan) => plan.status === "draft").length), "awaiting review"],
    ["Require approval", String(plans.filter((plan) => plan.requires_approval).length), "gated execution"],
    ["High risk", String(plans.filter((plan) => ["high", "critical"].includes(plan.risk_level)).length), "operator attention"],
  ];

  return (
    <section className="gate-view">
      <div className="section-heading">
        <div>
          <h1>Remediation Plans</h1>
          <p>Read-only remediation drafts. Execution stays behind approval gates; no mutation runs from this view.</p>
        </div>
        <button className="primary-action" type="button" onClick={dashboard.refresh} disabled={dashboard.status === "refreshing"}>
          <ArrowsClockwise size={17} /> {dashboard.status === "refreshing" ? "Refreshing" : "Refresh"}
        </button>
      </div>
      <div className="summary-grid">
        {metrics.map(([label, value, note]) => (
          <div className="metric" key={label}>
            <span>{label}</span>
            <strong>{value}</strong>
            <small>{note}</small>
          </div>
        ))}
      </div>
      {plans.length ? (
        <div className="gate-layout">
          <div className="approval-stack">
            {plans.map((plan) => (
              <button
                className={`approval-card ${plan.id === selected?.id ? "is-active" : ""}`}
                type="button"
                key={plan.id}
                onClick={() => navigate("Remediation Plans", plan.id)}
              >
                <span>{plan.risk_level} · {statusText(plan.status)}</span>
                <strong>{plan.title}</strong>
                <small>{resourceLabel(plan)} · {compactId(plan.id)}</small>
                <b>{plan.requires_approval ? "gated" : "ungated"}</b>
              </button>
            ))}
          </div>
          <div className="review-surface">
            <h2>{selected?.title ?? "Remediation plan"}</h2>
            <p>{selected?.summary ?? "Select a plan to review its steps and gates."}</p>
            <div className="review-grid">
              <ReviewItem label="Status" value={statusText(selected?.status)} tone={selected?.status === "draft" ? "pending" : undefined} />
              <ReviewItem label="Risk" value={statusText(selected?.risk_level, "Unknown")} tone={riskTone(selected?.risk_level) === "high" ? "risk" : "pending"} />
              <ReviewItem label="Requires approval" value={String(selected?.requires_approval ?? false)} />
              <ReviewItem label="Resource" value={resourceLabel(selected)} />
              <ReviewItem
                label="Incident"
                value={
                  <button className="link-text" type="button" onClick={() => navigate("Incidents", selected?.incident_id)}>
                    {compactId(selected?.incident_id)}
                  </button>
                }
              />
            </div>
            {steps.length ? (
              <div className="plan-steps">
                {steps.map((step) => (
                  <div className="plan-step" key={`${step.order}-${step.capability}`}>
                    <b>{step.order}</b>
                    <span>{step.kind}</span>
                    <strong>{step.capability}</strong>
                    <p>{step.summary}</p>
                  </div>
                ))}
              </div>
            ) : null}
            {linkedGates.length ? (
              <div className="resource-chips">
                {linkedGates.map((gate) => (
                  <button
                    className={`chip-${gate.status === "pending" ? "pending" : "settled"}`}
                    type="button"
                    key={gate.id}
                    onClick={() => navigate("Approval Gates", gate.id)}
                  >
                    {gate.gate_kind} · {statusText(gate.status)}
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        </div>
      ) : (
        <EmptyState title="No remediation plans" body="Draft remediation plans appear when incident candidates are created from observability evidence." />
      )}
    </section>
  );
}

function ObservationsView({ dashboard, selectedId, openRun }) {
  const observations = dashboard.data?.observations ?? [];
  const [sourceFilter, setSourceFilter] = useState("all");
  const sources = ["all", ...new Set(observations.map((observation) => observation.source))];
  const filtered =
    sourceFilter === "all"
      ? observations
      : observations.filter((observation) => observation.source === sourceFilter);
  const selected =
    observations.find((observation) => observation.id === selectedId) ??
    filtered[0] ??
    null;
  const metrics = [
    ["Observations", String(observations.length), "latest page"],
    ["Sources", String(new Set(observations.map((observation) => observation.source)).size), "evidence origins"],
    ["Run-linked", String(observations.filter((observation) => observation.run_id).length), "execution context"],
    ["With artifacts", String(observations.filter((observation) => observation.artifact_id).length), "durable payloads"],
  ];

  return (
    <section className="gate-view">
      <div className="section-heading">
        <div>
          <h1>Observations</h1>
          <p>Normalized read-only facts persisted from typed cluster reads and control-plane activity.</p>
        </div>
        <div className="detail-actions">
          <div className="filter-row" role="tablist" aria-label="Observation source filter">
            {sources.map((option) => (
              <button
                key={option}
                type="button"
                className={sourceFilter === option ? "selected" : ""}
                onClick={() => setSourceFilter(option)}
              >
                {option}
              </button>
            ))}
          </div>
          <button className="primary-action" type="button" onClick={dashboard.refresh} disabled={dashboard.status === "refreshing"}>
            <ArrowsClockwise size={17} /> {dashboard.status === "refreshing" ? "Refreshing" : "Refresh"}
          </button>
        </div>
      </div>
      <div className="summary-grid">
        {metrics.map(([label, value, note]) => (
          <div className="metric" key={label}>
            <span>{label}</span>
            <strong>{value}</strong>
            <small>{note}</small>
          </div>
        ))}
      </div>
      {filtered.length ? (
        <div className="gate-layout">
          <div className="approval-stack">
            {filtered.map((observation) => (
              <button
                className={`approval-card ${observation.id === selected?.id ? "is-active" : ""}`}
                type="button"
                key={observation.id}
                onClick={() => navigate("Observations", observation.id)}
              >
                <span>{observation.source} · {observation.kind}</span>
                <strong>{observation.subject}</strong>
                <small>{resourceLabel(observation)} · {compactId(observation.id)}</small>
                <b>{observation.artifact_id ? "artifact" : "inline"}</b>
              </button>
            ))}
          </div>
          <div className="review-surface">
            <h2>{selected?.subject ?? "Observation"}</h2>
            <p>{selected?.summary ?? "Select an observation to review its normalized data."}</p>
            <div className="review-grid">
              <ReviewItem label="Source" value={selected?.source ?? "unknown"} />
              <ReviewItem label="Kind" value={selected?.kind ?? "unknown"} />
              <ReviewItem label="Resource" value={resourceLabel(selected)} />
              <ReviewItem label="Artifact" value={selected?.artifact_id ? compactId(selected.artifact_id) : "none"} />
              {selected?.run_id ? (
                <ReviewItem
                  label="Run"
                  value={
                    <button className="link-text" type="button" onClick={() => openRun(selected.run_id)}>
                      {compactId(String(selected.run_id))}
                    </button>
                  }
                />
              ) : null}
            </div>
            <div className="diff-box">
              <div><ChartLineUp size={18} /> normalized observation data</div>
              <pre>{JSON.stringify(selected?.data_json ?? {}, null, 2)}</pre>
            </div>
          </div>
        </div>
      ) : (
        <EmptyState title="No observations" body="Typed cluster reads and control-plane activity persist observations here." />
      )}
    </section>
  );
}

function ReviewItem({ label, value, tone }) {
  return (
    <div className={`review-item ${tone ? `tone-${tone}` : ""}`}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function EmptyState({ title, body }) {
  return (
    <div className="empty-state">
      <strong>{title}</strong>
      <p>{body}</p>
    </div>
  );
}

function Inspector({
  selectedNode,
  topologyNodes,
  flow,
  pendingToolApprovals,
  actionNotice,
}) {
  const node = useMemo(
    () => topologyNodes.find((item) => item.id === selectedNode) ?? topologyNodes[0] ?? null,
    [selectedNode, topologyNodes],
  );
  const detailRows = useMemo(() => detailRowsForNode(selectedNode, flow), [selectedNode, flow]);
  const readiness = flow?.readiness;
  const selectedResource = selectedResourceForNode(selectedNode, flow);
  const gateCounts = approvalGateCounts(readiness);
  const auditEvents = flow?.audit_events?.slice(-5) ?? [];

  if (!flow || !node) {
    return (
      <aside className="inspector">
        <div className="inspector-header">
          <div>
            <h2>No Flow Selected</h2>
            <StatusPill tone="future">No live data</StatusPill>
          </div>
        </div>
        <EmptyState title="Inspector waiting for live data" body="The inspector only renders API-backed WorkPlan or ChangeSet flow resources." />
      </aside>
    );
  }

  return (
    <aside className="inspector">
      <div className="inspector-header">
        <div>
          <h2>{node.label}</h2>
          <StatusPill tone={node.status}>{statusLabels[node.status] ?? node.status}</StatusPill>
        </div>
        <IconButton label="Close inspector"><X size={18} /></IconButton>
      </div>
      <dl className="detail-list">
        {detailRows.map((row) => (
          <div key={row.label}>
            <dt>{row.label}</dt>
            <dd className={row.danger ? "danger" : ""}>{row.value}</dd>
          </div>
        ))}
      </dl>
      <section className="state-axes" aria-label="State axes">
        <ReviewItem label="Readiness" value={readiness?.ready ? "Ready" : "Blocked"} tone={readiness?.ready ? undefined : "risk"} />
        <ReviewItem label="Pipeline" value={statusText(flow.pipeline_intent?.status, "Missing")} tone={lifecycleTone(flow.pipeline_intent?.status)} />
        <ReviewItem label="Gates" value={`${gateCounts.pending} pending`} tone={gateCounts.pending ? "pending" : undefined} />
      </section>
      <section className="action-panel">
        <div>
          <h3>Resource actions</h3>
          <p>Direct resource mutations are not implemented in this UI. Use Queue, Tool Approvals, and Approval Gates for live API-backed actions.</p>
        </div>
        <div className="capability-tags">
          <span>read-only detail</span>
          <span>no cluster mutation</span>
          <span>no release execution</span>
        </div>
        {actionNotice ? <span className="action-notice">{actionNotice}</span> : null}
      </section>
      <Disclosure title="Readiness Evaluation" badge={readiness?.ready ? "Ready" : "Blocked"} defaultOpen>
        <div className="policy-grid">
          <ReviewItem label="Overall" value={readiness?.ready ? "Pass" : "Blocked"} tone={readiness?.ready ? undefined : "risk"} />
          <ReviewItem label="Risk" value={selectedResource?.risk_level ?? "unknown"} />
          <ReviewItem label="Blockers" value={readiness?.blockers?.length ?? 0} tone={readiness?.blockers?.length ? "risk" : undefined} />
          <ReviewItem label="Warnings" value={readiness?.warnings?.length ?? 0} tone={readiness?.warnings?.length ? "pending" : undefined} />
        </div>
        <ReadinessFacts readiness={readiness} />
      </Disclosure>
      <Disclosure title="Resource Scope" badge={selectedResource?.resource_namespace ?? "unscoped"}>
        <div className="radius-list">
          <div><span>Namespace</span><strong>{selectedResource?.resource_namespace ?? selectedResource?.target_namespace ?? "not scoped"}</strong></div>
          <div><span>Kind</span><strong>{selectedResource?.resource_kind ?? "unknown"}</strong></div>
          <div><span>Name</span><strong>{selectedResource?.resource_name ?? selectedResource?.argo_application ?? "unknown"}</strong></div>
          <div><span>Production-impacting</span><strong>{String(selectedResource?.production_impacting ?? false)}</strong></div>
        </div>
        <div className="resource-chips">
          {resourceChips(flow).map((resource) => <span key={resource}>{resource}</span>)}
        </div>
      </Disclosure>
      <Disclosure title="Approval Gates (this flow)" badge={`${gateCounts.pending} pending`}>
        <p className="compact-copy">Gates scoped to the selected flow root. Governance gates are decided in the Approval Gates tab; the global count lives in the navigation badge.</p>
        <div className="radius-list">
          <div><span>Pending</span><strong>{gateCounts.pending}</strong></div>
          <div><span>Stale</span><strong>{gateCounts.stale}</strong></div>
          <div><span>Rejected</span><strong>{gateCounts.rejected}</strong></div>
        </div>
      </Disclosure>
      <Disclosure title="Tool Approvals" badge={`${pendingToolApprovals ?? 0} pending`}>
        <p className="compact-copy">Tool approvals are live in the Approvals tab when a run pauses for write, shell, network, or destructive actions.</p>
      </Disclosure>
      <Disclosure title="Audit Events" badge={auditEvents.length ? "latest" : "none"}>
        {auditEvents.length ? (
          <div className="tool-event-list">
            {auditEvents.map((event, index) => (
              <div key={`${event.id ?? event.kind}-${index}`}>
                <span className={`dot ${eventTone(event.kind)}`} />
                <strong>{event.kind}</strong>
                <small>{formatTimestamp(event.created_at)}</small>
              </div>
            ))}
          </div>
        ) : (
          <p className="compact-copy">No resource-scoped audit events are attached to this flow yet.</p>
        )}
      </Disclosure>
    </aside>
  );
}

function detailRowsForNode(nodeId, flow) {
  if (!flow) {
    return [];
  }

  const resource = selectedResourceForNode(nodeId, flow);

  return [
    { label: "Flow root", value: `${flow.resource_kind}/${flow.resource_id}` },
    { label: "ID", value: resource?.id ?? "not created" },
    { label: "Status", value: statusText(resource?.status, "Missing") },
    { label: "Risk", value: resource?.risk_level ?? "unknown" },
    { label: "Namespace", value: resource?.resource_namespace ?? resource?.target_namespace ?? "not scoped" },
    { label: "Production-impacting", value: String(resource?.production_impacting ?? false), danger: Boolean(resource?.production_impacting) },
  ];
}

function selectedResourceForNode(nodeId, flow) {
  const byNode = {
    "work-plan": flow?.work_plan,
    "change-set": flow?.change_set,
    "pipeline-intent": flow?.pipeline_intent,
    "pipeline-analysis": flow?.pipeline_intent,
    "deployment-intent": flow?.deployment_intent,
    release: flow?.release,
    "registry-evidence": flow?.registry_evidence,
  };
  return byNode[nodeId] ?? flow?.work_plan;
}

function approvalGateCounts(readiness) {
  const gates = readiness?.approval_gates ?? {};
  return {
    pending: gates.pending?.length ?? 0,
    stale: gates.stale?.length ?? 0,
    rejected: gates.rejected?.length ?? 0,
  };
}

function resourceChips(flow) {
  return [
    flow.work_plan && `WorkPlan/${compactId(flow.work_plan.id)}`,
    flow.change_set && `ChangeSet/${compactId(flow.change_set.id)}`,
    flow.pipeline_intent && `PipelineIntent/${compactId(flow.pipeline_intent.id)}`,
    flow.deployment_intent && `DeploymentIntent/${compactId(flow.deployment_intent.id)}`,
    flow.release && `Release/${compactId(flow.release.id)}`,
    flow.registry_evidence && `RegistryEvidence/${compactId(flow.registry_evidence.id)}`,
  ].filter(Boolean);
}

function eventTone(kind) {
  if (kind?.includes("approval") || kind?.includes("gate") || kind?.includes("stale")) {
    return "policy";
  }
  if (kind?.includes("run") || kind?.includes("tool")) {
    return "tool";
  }
  return "audit";
}

function eventPayloadSummary(payload) {
  if (!payload || typeof payload !== "object") {
    return "no payload";
  }
  if (typeof payload.summary === "string") {
    return payload.summary;
  }
  if (typeof payload.error === "string") {
    return payload.error;
  }
  if (typeof payload.action === "string") {
    return payload.reason ? `${payload.action}: ${payload.reason}` : payload.action;
  }
  if (typeof payload.raw_provider_id === "string") {
    return compactId(payload.raw_provider_id);
  }
  const keys = Object.keys(payload);
  return keys.length ? keys.slice(0, 4).join(", ") : "empty payload";
}

function artifactSummary(artifact) {
  if (typeof artifact.content_text === "string" && artifact.content_text.trim()) {
    return artifact.content_text.trim().slice(0, 180);
  }
  if (artifact.content_json && typeof artifact.content_json === "object") {
    return summarizeJson(artifact.content_json, Object.keys(artifact.content_json).slice(0, 4).join(", "));
  }
  return artifact.path ?? "metadata only";
}

function mergeRunEvent(detail, runId, event) {
  const base = detail ?? {
    run: null,
    events: [],
    diff: { run_id: runId, changes: [], diff: "" },
    artifacts: [],
  };
  const eventKey = event.event_id ?? `${event.seq}-${event.type}`;
  const existing = new Set(base.events.map((item) => item.event_id ?? `${item.seq}-${item.type}`));
  if (existing.has(eventKey)) {
    return base;
  }
  return {
    ...base,
    events: [...base.events, event].sort((left, right) => Number(left.seq ?? 0) - Number(right.seq ?? 0)),
  };
}

function latestEventSeq(events) {
  return (events ?? []).reduce((latest, event) => {
    const seq = Number(event.seq ?? 0);
    return Number.isFinite(seq) && seq > latest ? seq : latest;
  }, 0);
}

function isTerminalEvent(event) {
  return ["run.finished", "run.failed", "run.cancelled"].includes(event?.type);
}

function isTerminalStatus(status) {
  return ["completed", "failed", "cancelled"].includes(status);
}

function eventShouldRefreshRunDetail(event) {
  return [
    "run.finished",
    "run.failed",
    "run.cancelled",
    "approval.required",
    "approval.decided",
    "tool.finished",
  ].includes(event?.type);
}

function streamLabel(streamState) {
  if (streamState.status === "connecting") {
    return "Connecting";
  }
  if (streamState.status === "live") {
    return "Live events";
  }
  if (streamState.status === "closed") {
    return "Stream closed";
  }
  if (streamState.status === "error") {
    return "Stream disconnected";
  }
  return "Stream idle";
}

function streamDescription(streamState) {
  if (streamState.status === "connecting") {
    return "Opening the API-backed event stream from the latest durable event cursor.";
  }
  if (streamState.status === "live") {
    return "Receiving new durable events from the API stream.";
  }
  if (streamState.status === "closed") {
    return "Run is terminal or paused at an approval boundary; the event log is a durable snapshot.";
  }
  if (streamState.status === "error") {
    return streamState.error ?? "The event stream disconnected.";
  }
  return "Waiting for a selected run and its durable event cursor.";
}

function ReadinessFacts({ readiness }) {
  const facts = [...(readiness?.blockers ?? []), ...(readiness?.warnings ?? [])];
  if (!facts.length) {
    return <p className="compact-copy">No blockers or warnings are currently reported for this resource.</p>;
  }
  return (
    <div className="fact-list">
      {facts.slice(0, 6).map((fact, index) => (
        <div key={`${fact.code}-${index}`}>
          <span className={`dot ${index < (readiness?.blockers?.length ?? 0) ? "blocked" : "pending"}`} />
          <strong>{fact.code}</strong>
          <p>{fact.message}</p>
        </div>
      ))}
    </div>
  );
}

function Disclosure({ title, badge, children, defaultOpen = false }) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <section className={`disclosure ${open ? "is-open" : ""}`}>
      <button type="button" onClick={() => setOpen((value) => !value)}>
        <span>{title}</span>
        <b>{badge}</b>
      </button>
      {open ? <div className="disclosure-body">{children}</div> : null}
    </section>
  );
}

export function App() {
  const [route, setRoute] = useState(parseHash);
  const [lastRunId, setLastRunId] = useState(null);
  const [theme, setTheme] = useState("dark");
  const [selectedNode, setSelectedNode] = useState("pipeline-analysis");
  const [gateState, setGateState] = useState("pending");
  const [toolApprovalState, setToolApprovalState] = useState("pending");
  const [actionNotice, setActionNotice] = useState("");
  const [scope, setScope] = useState(EMPTY_SCOPE);

  useEffect(() => {
    const onHashChange = () => setRoute(parseHash());
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  const routeRunId = route.view === "Run Detail" && typeof route.param === "string" ? route.param : null;
  useEffect(() => {
    if (routeRunId) {
      setLastRunId(routeRunId);
    }
  }, [routeRunId]);

  const flowRoot = route.view === "Flow" && route.param?.kind ? route.param : null;
  const dashboard = usePharnessDashboard(flowRoot, scope);

  return (
    <AppShell
      route={route}
      selectedRunId={routeRunId ?? lastRunId}
      theme={theme}
      setTheme={setTheme}
      selectedNode={selectedNode}
      setSelectedNode={setSelectedNode}
      gateState={gateState}
      setGateState={setGateState}
      toolApprovalState={toolApprovalState}
      setToolApprovalState={setToolApprovalState}
      actionNotice={actionNotice}
      setActionNotice={setActionNotice}
      dashboard={dashboard}
      scope={scope}
      setScope={setScope}
    />
  );
}
