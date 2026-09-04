import { ArrowSquareOut, Robot, WarningCircle } from "@phosphor-icons/react";
import { Empty, LinkButton, Metric, ResourceState, SectionHeader, Status } from "../components";
import { useOrganizationOverview } from "../ConsoleContext";

export function OverviewScreen() {
  const resource = useOrganizationOverview();
  const data = resource.data;
  const counts = data?.work_items || {};
  return <ResourceState status={resource.status} error={resource.error} empty={resource.status === "ready" && !data}>
    <SectionHeader eyebrow="Organization" title={data?.organization?.display_name || "PHarness"} summary="Current Product work, active agents, and exact attention boundaries. This screen is read-only." />
    <section className="repo-metrics" aria-label="Current work summary">
      <Metric label="Current WorkItems" value={counts.current ?? "Unavailable"} detail={`${counts.denominator ?? "Unavailable"} total Repo Mode records`} />
      <Metric label="Waiting" value={counts.waiting ?? "Unavailable"} detail="Human approval or external evidence" />
      <Metric label="Blocked" value={counts.blocked ?? "Unavailable"} detail="Needs an operator decision" />
      <Metric label="Failed" value={counts.failed ?? "Unavailable"} detail="Terminally closed with preserved evidence" />
      <Metric label="Recently completed" value={counts.recently_completed ?? "Unavailable"} detail="Closed after observed merge" />
    </section>

    <div className="repo-overview-grid">
      <section className="repo-panel repo-span-2">
        <header><div><span className="repo-eyebrow">Products</span><h2>Current product work</h2></div><LinkButton to="products">View all</LinkButton></header>
        <div className="repo-card-grid">
          {(data?.product_summaries || []).map((product: any) => <article className="repo-product-card" key={product.id}>
            <div><Status value={product.actionable_waits ? "waiting" : "active"} /><h3>{product.display_name}</h3><p>Owned by {product.owner_principal}</p></div>
            <dl><div><dt>Repositories</dt><dd>{product.repository_count}</dd></div><div><dt>Current work</dt><dd>{product.current_work_items}</dd></div><div><dt>Attention</dt><dd>{product.actionable_waits}</dd></div></dl>
            <LinkButton to={`products/${product.id}/work-items`}>Open Product</LinkButton>
          </article>)}
          {!data?.product_summaries?.length ? <Empty title="No Products registered" message="Create a Product from the Products screen before onboarding a Repository." /> : null}
        </div>
      </section>

      <section className="repo-panel repo-span-2">
        <header><div><span className="repo-eyebrow">Lifecycle</span><h2>Current boundaries</h2></div><span className="repo-count">{counts.current ?? 0}</span></header>
        <div className="repo-stage-counts">{["discover","plan","implement","test","verify","source_delivery"].map(stage => <div key={stage}><span>{stage.replaceAll("_"," ")}</span><strong>{counts.by_lifecycle_boundary?.[stage] || 0}</strong></div>)}</div>
      </section>

      <section className="repo-panel">
        <header><div><span className="repo-eyebrow">Attention</span><h2>Owned boundaries</h2></div><span className="repo-count">{data?.attention?.length || 0}</span></header>
        <div className="repo-list">
          {(data?.attention || []).map((item: any) => <button type="button" className="repo-list-row" key={`${item.resource_kind || item.kind}-${item.resource_id}-${item.action?.id || item.kind}`} onClick={() => { window.location.hash = item.resource_kind === "repository_onboarding" ? `#/repository-onboardings/${item.resource_id}` : `#/work-items/${item.resource_id}/overview`; }}>
            <WarningCircle size={19} /><div><strong>{item.title || item.action?.id?.replaceAll("_", " ") || item.resource_id}</strong><span>{item.action?.external_effect_summary || item.reason || "Operator attention required"}</span><small>{item.kind?.replaceAll("_", " ")} · owner {item.resource_id}</small></div><Status value={item.action?.status || item.status} />
          </button>)}
          {!data?.attention?.length ? <p className="repo-muted">No WorkItem currently needs human attention.</p> : null}
        </div>
      </section>

      <section className="repo-panel">
        <header><div><span className="repo-eyebrow">Agents</span><h2>Active AgentRuns</h2></div><LinkButton to="agents">View all</LinkButton></header>
        <div className="repo-list">
          {(data?.active_agent_runs || []).map((entry: any) => <button type="button" className="repo-list-row" key={entry.run.id} onClick={() => { window.location.hash = `#/agents/runs/${entry.run.id}`; }}>
            <Robot size={19} /><div><strong>{entry.run.profile_id || "Agent"}</strong><span>{entry.work_item_id} · {entry.run.stage_execution_id}</span></div><Status value={entry.run.status} />
          </button>)}
          {!data?.active_agent_runs?.length ? <p className="repo-muted">No agent is currently executing.</p> : null}
        </div>
      </section>

      <section className="repo-panel repo-span-2">
        <header><div><span className="repo-eyebrow">Repository readiness</span><h2>Gaps that prevent new work</h2></div><span className="repo-rate">{data?.repository_readiness_rate?.ready || 0}/{data?.repository_readiness_rate?.total || 0} ready</span></header>
        <div className="repo-list repo-columns">
          {(data?.repository_readiness_gaps || []).map((repository: any) => <button type="button" className="repo-list-row" key={`${repository.product_id}-${repository.repository_id}`} onClick={() => { window.location.hash = `#/repositories/${repository.repository_id}/readiness`; }}>
            <ArrowSquareOut size={18} /><div><strong>{repository.canonical_url}</strong><span>{repository.product_id}</span></div><span><Status value={repository.contract_status} /> <Status value={repository.coding_status} /></span>
          </button>)}
          {!data?.repository_readiness_gaps?.length ? <p className="repo-muted">Every registered Repository has current contract and coding readiness.</p> : null}
        </div>
      </section>

      {(data?.repository_capability_gaps || []).length ? <section className="repo-panel repo-span-2">
        <header><div><span className="repo-eyebrow">Capability availability</span><h2>Stale or unavailable Repository capabilities</h2></div><span className="repo-count">{data.repository_capability_gaps.length}</span></header>
        <div className="repo-list repo-columns">{data.repository_capability_gaps.map((gap:any) => <button type="button" className="repo-list-row" key={`${gap.repository_id}-${gap.capability}`} onClick={() => { window.location.hash = `#/repositories/${gap.repository_id}/overview`; }}><WarningCircle size={18} /><div><strong>{gap.capability.replaceAll("_", " ")}</strong><span>{gap.summary}</span><small>{gap.verified_at ? `Verified ${gap.verified_at}` : "No fresh isolated verification"}</small></div><Status value={gap.status} /></button>)}</div>
      </section> : null}
    </div>
    {data?.unassigned_legacy?.count ? <aside className="repo-legacy-note"><strong>{data.unassigned_legacy.count} legacy WorkItems are unassigned.</strong><span>They remain available through WorkItems history and are not attributed to a Product.</span></aside> : null}
  </ResourceState>;
}
