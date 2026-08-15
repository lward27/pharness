import { useEffect, useMemo, useState } from "react";
import { ArrowsClockwise } from "@phosphor-icons/react";
import { EmptyState, StatusPill } from "../components/Operational";
import { PaginationControls } from "../components/PaginationControls";
import { loadWorkItems } from "../api/workItems";
import { compactId, formatTimestamp, lifecycleTone, statusText, timestampTitle } from "../lib/formatters";

const PAGE_SIZE = 25;

type WorkItemsListViewProps = {
  dashboard: any;
  autoRefresh: boolean;
  openWorkItem: (workItemId: string) => void;
};

export function WorkItemsListView({ dashboard, autoRefresh, openWorkItem }: WorkItemsListViewProps) {
  const scopeOptions = dashboard.data?.scopeOptions ?? { environments: [], repositories: [], actors: [], origins: [] };
  const [filters, setFilters] = useState({ status: "", environment: "", repository: "", actor: "", origin: "" });
  const [offset, setOffset] = useState(0);
  const [reload, setReload] = useState(0);
  const [state, setState] = useState<any>({ status: "loading", data: null, error: null });
  const query = useMemo(() => ({ ...filters, limit: PAGE_SIZE, offset }), [filters, offset]);
  const filterKey = JSON.stringify(filters);

  useEffect(() => { setOffset(0); }, [filterKey]);
  useEffect(() => {
    let current = true;
    const refresh = () => {
      setState((value: any) => ({ ...value, status: value.data ? "refreshing" : "loading", error: null }));
      loadWorkItems(query).then((data) => {
        if (current) setState({ status: "ready", data, error: null });
      }).catch((error) => {
        if (current) setState((value: any) => ({ ...value, status: "error", error: error instanceof Error ? error.message : String(error) }));
      });
    };
    refresh();
    if (!autoRefresh) return () => { current = false; };
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") refresh();
    }, 30_000);
    return () => { current = false; window.clearInterval(timer); };
  }, [query, reload, autoRefresh]);

  const response = state.data ?? { work_items: [], operator_state: {}, count: 0, limit: PAGE_SIZE, offset };
  const workItems = Array.isArray(response.work_items) ? response.work_items : [];
  const operatorState = response.operator_state ?? {};
  const count = Number(response.count ?? 0);
  const clearFilters = () => setFilters({ status: "", environment: "", repository: "", actor: "", origin: "" });

  return (
    <section className="work-items-view">
      <div className="section-heading">
        <div><h1>WorkItems</h1><p>Durable autonomous delivery intents and their current controller boundary.</p></div>
        <button className="primary-action" type="button" onClick={() => setReload((value) => value + 1)} disabled={state.status === "refreshing"}><ArrowsClockwise size={17} /> {state.status === "refreshing" ? "Refreshing" : "Refresh"}</button>
      </div>
      <div className="work-item-filters" aria-label="WorkItem filters">
        <label>Status<select value={filters.status} onChange={(event) => setFilters((current) => ({ ...current, status: event.target.value }))}><option value="">All statuses</option>{["blocked", "planning", "executing", "verifying", "completed", "cancelled"].map((value) => <option value={value} key={value}>{statusText(value)}</option>)}</select></label>
        <label>Environment<select value={filters.environment} onChange={(event) => setFilters((current) => ({ ...current, environment: event.target.value }))}><option value="">All environments</option>{scopeOptions.environments.map((value: string) => <option value={value} key={value}>{value}</option>)}</select></label>
        <label>Repository<select value={filters.repository} onChange={(event) => setFilters((current) => ({ ...current, repository: event.target.value }))}><option value="">All repositories</option>{scopeOptions.repositories.map((value: string) => <option value={value} key={value}>{value}</option>)}</select></label>
        <label>Actor<select value={filters.actor} onChange={(event) => setFilters((current) => ({ ...current, actor: event.target.value }))}><option value="">All actors</option>{scopeOptions.actors.map((value: string) => <option value={value} key={value}>{value}</option>)}</select></label>
        <label>Origin<select value={filters.origin} onChange={(event) => setFilters((current) => ({ ...current, origin: event.target.value }))}><option value="">All origins</option>{scopeOptions.origins.map((value: string) => <option value={value} key={value}>{value}</option>)}</select></label>
        <button type="button" onClick={clearFilters}>Clear</button>
      </div>
      <PaginationControls label="WorkItems" count={count} limit={response.limit ?? PAGE_SIZE} offset={response.offset ?? offset} loading={state.status === "loading" || state.status === "refreshing"} onOffsetChange={setOffset} />
      {state.error ? <div className="api-banner">Unable to load WorkItems: {state.error}</div> : null}
      {workItems.length ? <div className="work-item-list">
        {workItems.map((item: any) => {
          const operator = operatorState[item.id] ?? {};
          const wait = operator.active_wait;
          return <button className="work-item-row" type="button" key={item.id} onClick={() => openWorkItem(item.id)}>
            <span><strong>{item.title}</strong><em>{operator.attention_reason ?? operator.current_boundary ?? item.intent}</em>{wait ? <small>Waiting: {statusText(wait.wait_kind)} · next check <time title={timestampTitle(wait.next_check_at)}>{formatTimestamp(wait.next_check_at)}</time></small> : null}</span>
            <StatusPill tone={lifecycleTone(item.status)}>{statusText(item.status)}</StatusPill>
            <span>{item.target_environment}</span>
            <span title={`${item.source_repo} @ ${item.source_ref}`}>{compactId(item.source_repo)} / {item.source_ref}</span>
            <span>{operator.attempts_remaining ?? Math.max(0, Number(item.max_attempts ?? 0) - Number(item.attempt_count ?? 0))} attempts left</span>
            <time title={timestampTitle(item.updated_at)}>{formatTimestamp(item.updated_at)}</time>
          </button>;
        })}
      </div> : state.status === "loading" ? <EmptyState title="Loading WorkItems" body="Reading the durable controller queue." /> : <EmptyState title={count ? "No WorkItems on this page" : "No WorkItems match these filters"} body={count ? "Move to another result page." : "Change or clear filters, or submit a durable delivery intent."} />}
    </section>
  );
}
