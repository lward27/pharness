import { useEffect, useMemo, useState } from "react";
import {
  ArrowsClockwise,
  ChartLineUp,
  CheckCircle,
  CircleHalf,
  ClipboardText,
  ClockCounterClockwise,
  Cube,
  GitBranch,
  MagnifyingGlass,
  Moon,
  Pulse,
  Rows,
  ShieldCheck,
  ShieldWarning,
  Siren,
  Stack,
  ToggleRight,
} from "@phosphor-icons/react";
import { IconButton } from "./components/Operational.tsx";
import { statusText } from "./lib/formatters.ts";
import { buildEvidenceRows, buildEvents } from "./lib/flowPresentation.ts";
import { QueueView } from "./views/QueueView.tsx";
import { TriageView } from "./views/TriageView.tsx";
import { WorkItemDetailView } from "./views/WorkItemDetailView.tsx";
import { WorkItemsListView } from "./views/WorkItemsListView.tsx";
import { WorkItemNewView } from "./views/WorkItemNewView.tsx";
import { RunDetailView } from "./views/RunDetailView.tsx";
import { ToolApprovalsView as ToolApprovalsPanel } from "./views/ToolApprovalsView.tsx";
import { ApprovalGatesView as ApprovalGatesPanel } from "./views/ApprovalGatesView.tsx";
import { AuditView as AuditPanel } from "./views/AuditView.tsx";
import { WorkPlansView as WorkPlansPanel } from "./views/WorkPlansView.tsx";
import { IncidentsView as IncidentsPanel, ObservationsView as ObservationsPanel, RemediationPlansView as RemediationPlansPanel } from "./views/ResourceViews.tsx";
import { FlowView as FlowPanel } from "./views/FlowView.tsx";
import { StatusView as StatusPanel } from "./views/StatusView.tsx";
import { loadDashboardData, loadTriage, loadTriageSummary } from "./pharnessApi";

const navGroups = [
  { id: "operate", label: "Operate", items: [
    { id: "Triage", label: "Triage", view: "Triage", icon: Rows },
    { id: "WorkItems", label: "WorkItems", view: "WorkItems", icon: ClipboardText },
    { id: "Runs", label: "Runs", view: "Queue", activeViews: ["Queue", "Run Detail"], icon: Pulse },
  ] },
  { id: "govern", label: "Govern", items: [
    { id: "Approvals", label: "Tool approvals", view: "Approvals", icon: ShieldWarning },
    { id: "Approval Gates", label: "Lifecycle gates", view: "Approval Gates", icon: ShieldCheck },
  ] },
  { id: "investigate", label: "Investigate", items: [
    { id: "Observations", label: "Observations", view: "Observations", icon: ChartLineUp },
    { id: "Incidents", label: "Incidents", view: "Incidents", icon: Siren },
    { id: "Remediation Plans", label: "Remediation", view: "Remediation Plans", icon: ClipboardText },
    { id: "Audit", label: "Audit", view: "Audit", icon: ClockCounterClockwise },
  ] },
  { id: "platform", label: "Platform", items: [
    { id: "Status", label: "Status", view: "Status", icon: CircleHalf },
  ] },
];

const navItems = navGroups.flatMap((group) => group.items);

function navItemActive(item, activeView) {
  return item.view === activeView || item.activeViews?.includes(activeView);
}


// Hash routing: #/<segment>[/<id>] with Flow roots as #/flow/<kind>/<id>.
const viewSegments = {
  Triage: "triage",
  WorkItems: "work-items",
  Flow: "flow",
  WorkPlans: "workplans",
  Queue: "queue",
  "Delivery Test": "delivery-test",
  Status: "status",
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
  const view = Object.keys(viewSegments).find((key) => viewSegments[key] === segment) ?? "Triage";
  if (view === "Flow" && first && second) {
    return { view, param: { kind: first, id: second } };
  }
  return { view, param: first ?? null };
}

function hashForRoute(view, param) {
  const segment = viewSegments[view] ?? "triage";
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
  environment: "",
  namespace: "",
  repo: "",
  branch: "",
  productionImpacting: "",
};

function usePharnessDashboard(flowRoot, scope, autoRefresh, activeView, hasSelectedWorkItem) {
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

  const refreshTriage = async () => {
    try {
      const [triage, triageSummary] = await Promise.all([loadTriage(), loadTriageSummary()]);
      setState((current) => current.data ? {
        ...current,
        status: current.status === "error" ? "ready" : current.status,
        data: {
          ...current.data,
          triage: triage ?? current.data.triage,
          triageSummary: triageSummary ?? triage?.summary ?? current.data.triageSummary,
          loadedAt: new Date().toLocaleTimeString(),
          loadedAtAbsolute: new Date().toLocaleString(),
        },
        error: null,
      } : current);
    } catch (error) {
      setState((current) => ({
        ...current,
        error: error instanceof Error ? error.message : String(error),
      }));
    }
  };

  const flowRootKey = flowRoot ? `${flowRoot.kind}:${flowRoot.id}` : "";
  const scopeKey = JSON.stringify(scope);
  useEffect(() => {
    refresh();
    if (!autoRefresh || hasSelectedWorkItem) {
      return undefined;
    }
    const poll = activeView === "Triage" ? refreshTriage : null;
    const interval = 10_000;
    if (!poll) {
      return undefined;
    }
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") {
        poll();
      }
    };
    const timer = window.setInterval(refreshWhenVisible, interval);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [flowRootKey, scopeKey, autoRefresh, activeView, hasSelectedWorkItem]);

  return { ...state, refresh };
}

function badgeForNav(id, data) {
  const triage = data?.triageSummary ?? data?.triage?.summary ?? {};
  if (id === "Triage") {
    return triage.total || null;
  }
  if (id === "Approvals") {
    return triage.pending_tool_approvals || null;
  }
  if (id === "Approval Gates") {
    return triage.pending_approval_gates || null;
  }
  if (id === "Runs") {
    return statusCount(data?.runsSummary?.summary, "running") || null;
  }
  if (id === "WorkItems") {
    return triage.blocked_work_items || null;
  }
  return null;
}

function statusCount(summary, status) {
  const bucket = summary?.by_status?.find((item) => item.value === status);
  return bucket?.count ?? 0;
}

function AppShell({
  route,
  selectedRunId,
  theme,
  setTheme,
  autoRefresh,
  setAutoRefresh,
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
          {navGroups.map((group) => <div className="nav-group" role="group" aria-label={group.label} key={group.id}>
            <span className="nav-group-label">{group.label}</span>
            {group.items.map((item) => {
              const Icon = item.icon;
              const active = navItemActive(item, activeView);
              const badge = badgeForNav(item.id, dashboardData) ?? item.badge;
              return <button
                className={`nav-item ${active ? "is-active" : ""}`}
                key={item.id}
                type="button"
                onClick={() => navigate(item.view)}
                title={`${group.label}: ${item.label}`}
              >
                <Icon size={20} />
                <span>{item.label}</span>
                {badge ? <b>{badge}</b> : null}
              </button>;
            })}
          </div>)}
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
          <MobileNavigation activeView={activeView} dashboardData={dashboardData} />
          <div className="view-context">
            <span>{activeView === "Triage" ? "Operator inbox" : statusText(activeView)}</span>
          </div>
          <div className="live-controls">
            <button className={`refresh-toggle ${autoRefresh ? "is-on" : ""}`} type="button" onClick={() => setAutoRefresh((value) => !value)} title={autoRefresh ? "Pause automatic refresh" : "Enable automatic refresh"}>
              <ToggleRight size={27} weight="fill" />
              {autoRefresh ? "Auto-refresh on" : "Auto-refresh off"}
            </button>
            <span title={dashboardData?.loadedAtAbsolute}>{dashboardData?.loadedAt ?? "not connected"}</span>
            <button className="live-button" type="button" onClick={dashboard.refresh}>
              <span className={dashboard.status === "error" ? "is-error" : ""} /> {dashboard.status === "refreshing" ? "Refreshing" : "Live"}
            </button>
          </div>
        </div>

        <section className="content-shell">
          <section className="primary-panel">
            {activeView === "Triage" ? (
              <TriageView dashboard={dashboard} openResource={navigate} />
            ) : activeView === "WorkItems" ? (
              routeParam === "new" ? <WorkItemNewView operatorName={dashboard.data?.config?.operator?.name} onCancel={() => navigate("WorkItems")} onCreated={(workItemId) => navigate("WorkItems", workItemId)} /> : routeParam ? <WorkItemDetailView workItemId={routeParam} refreshDashboard={dashboard.refresh} autoRefresh={autoRefresh} operatorName={dashboard.data?.config?.operator?.name} onBack={() => navigate("WorkItems")} /> : <WorkItemsListView dashboard={dashboard} autoRefresh={autoRefresh} openWorkItem={(workItemId) => navigate("WorkItems", workItemId)} onNew={() => navigate("WorkItems", "new")} />
            ) : activeView === "Flow" ? (
              <FlowPanel
                dashboard={dashboard}
                evidenceRows={liveEvidenceRows}
                events={liveEvents}
                navigate={navigate}
              />
            ) : activeView === "Queue" ? (
              <QueueView dashboard={dashboard} scope={scope} autoRefresh={autoRefresh} openRun={openRun} />
            ) : activeView === "Status" ? (
              <StatusPanel dashboard={dashboard} navigate={navigate} />
            ) : activeView === "WorkPlans" ? (
              <WorkPlansPanel dashboard={dashboard} selectedId={routeParam} navigate={navigate} />
            ) : activeView === "Run Detail" ? (
              <RunDetailView runId={selectedRunId} refreshDashboard={dashboard.refresh} onOpenQueue={() => navigate("Queue")} />
            ) : activeView === "Approvals" ? (
              <ToolApprovalsPanel
                dashboard={dashboard}
                selectedId={routeParam}
                actionNotice={actionNotice}
                setActionNotice={setActionNotice}
                openRun={openRun}
                navigate={navigate}
              />
            ) : activeView === "Audit" ? (
              <AuditPanel dashboard={dashboard} openRun={openRun} selectedSearch={routeParam} scope={scope} navigate={navigate} />
            ) : activeView === "Incidents" ? (
              <IncidentsPanel dashboard={dashboard} selectedId={routeParam} openRun={openRun} navigate={navigate} />
            ) : activeView === "Remediation Plans" ? (
              <RemediationPlansPanel dashboard={dashboard} selectedId={routeParam} navigate={navigate} />
            ) : activeView === "Observations" ? (
              <ObservationsPanel dashboard={dashboard} selectedId={routeParam} openRun={openRun} navigate={navigate} />
            ) : (
              <ApprovalGatesPanel
                dashboard={dashboard}
                selectedId={routeParam}
                actionNotice={actionNotice}
                setActionNotice={setActionNotice}
                navigate={navigate}
                operatorName={dashboard.data?.config?.operator?.name}
              />
            )}
          </section>
        </section>
      </main>
    </div>
  );
}

function MobileNavigation({ activeView, dashboardData }) {
  const activeItem = navItems.find((item) => navItemActive(item, activeView));
  return <label className="mobile-navigation"><Rows size={18} /><span>Navigate</span><select aria-label="Primary navigation" value={activeItem?.view ?? ""} onChange={(event) => navigate(event.target.value)}>
    {!activeItem ? <option value="">Current workspace</option> : null}
    {navGroups.map((group) => <optgroup label={group.label} key={group.id}>{group.items.map((item) => {
      const badge = badgeForNav(item.id, dashboardData);
      return <option value={item.view} key={item.id}>{item.label}{badge ? ` (${badge})` : ""}</option>;
    })}</optgroup>)}
  </select></label>;
}


function TopBar({ theme, setTheme, dashboard, route, scope, setScope }) {
  const [search, setSearch] = useState(route.view === "Audit" && typeof route.param === "string" ? route.param : "");
  const options = dashboard.data?.scopeOptions ?? {};
  const environmentOptions = options.environments ?? [];
  const namespaceOptions = options.namespaces ?? [];
  const repoOptions = options.repositories ?? [];
  const branchOptions = options.branches ?? [];

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

  const focusedWorkItem = route.view === "WorkItems" && typeof route.param === "string" && route.param !== "new";
  const focusedRun = route.view === "Run Detail" && typeof route.param === "string";
  const focusedResource = focusedWorkItem || focusedRun || route.view === "Flow";

  return (
    <header className={`topbar ${route.view === "Audit" ? "has-search" : "is-contextual"}`}>
      {focusedResource ? <div className="focused-resource-context">
        {focusedWorkItem ? <ClipboardText size={20} /> : focusedRun ? <Pulse size={20} /> : <GitBranch size={20} />}
        <div><span>{focusedWorkItem ? "WorkItem cockpit" : focusedRun ? "Run workspace" : "Delivery evidence"}</span><strong>{focusedWorkItem ? "One supervised lifecycle boundary at a time" : "Durable controller state"}</strong></div>
      </div> : <div className="scope-group">
        <ScopeSelect icon={Stack} label="Environment" value={scope.environment} options={environmentOptions} onChange={(value) => updateScope("environment", value)} />
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
      </div>}
      {route.view === "Audit" ? <form className="search" onSubmit={submitSearch}>
        <MagnifyingGlass size={18} />
        <input aria-label="Search audit events" placeholder="Search audit events..." value={search} onChange={(event) => setSearch(event.target.value)} />
        <button type="submit" aria-label="Run audit search" title="Run audit search"><MagnifyingGlass size={16} /></button>
      </form> : null}
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


export function App() {
  const [route, setRoute] = useState(parseHash);
  const [lastRunId, setLastRunId] = useState(null);
  const [theme, setTheme] = useState("dark");
  const [actionNotice, setActionNotice] = useState("");
  const [scope, setScope] = useState(EMPTY_SCOPE);
  const [autoRefresh, setAutoRefresh] = useState(() => window.localStorage.getItem("pharness.autoRefresh") !== "false");

  useEffect(() => {
    const onHashChange = () => setRoute(parseHash());
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  const routeRunId = route.view === "Run Detail" && typeof route.param === "string" ? route.param : null;
  useEffect(() => {
    if (routeRunId) setLastRunId(routeRunId);
  }, [routeRunId]);
  useEffect(() => {
    window.localStorage.setItem("pharness.autoRefresh", String(autoRefresh));
  }, [autoRefresh]);
  const flowRoot = route.view === "Flow" && route.param?.kind ? route.param : null;
  const dashboard = usePharnessDashboard(flowRoot, scope, autoRefresh, route.view, route.view === "WorkItems" && Boolean(route.param));
  return <AppShell route={route} selectedRunId={routeRunId ?? lastRunId} theme={theme} setTheme={setTheme} autoRefresh={autoRefresh} setAutoRefresh={setAutoRefresh} actionNotice={actionNotice} setActionNotice={setActionNotice} dashboard={dashboard} scope={scope} setScope={setScope} />;
}
