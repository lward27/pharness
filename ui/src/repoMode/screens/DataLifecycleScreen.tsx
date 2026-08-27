import { useMemo, useState } from "react";
import { Archive, Broom, Database, LockKey, Receipt } from "@phosphor-icons/react";
import { sendJson } from "../api";
import { Empty, ResourceState, Status } from "../components";
import { formatMoment, humanize } from "../presentation";
import { useResource } from "../useResource";

export function DataLifecycleScreen({ operatorName }: { operatorName:string }) {
  const inventory = useResource<any>("/api/system/data-inventory", { pollMs:30_000 });
  const archives = useResource<any>("/api/system/archives", { pollMs:30_000 });
  const previews = useResource<any>("/api/system/retention/previews", { pollMs:30_000 });
  const receipts = useResource<any>("/api/system/retention/receipts", { pollMs:30_000 });
  const [reason,setReason] = useState("Review normal PHarness retention eligibility");
  const [busy,setBusy] = useState(false);
  const [error,setError] = useState("");
  const [confirmation,setConfirmation] = useState("");
  const [archiveConfirmation,setArchiveConfirmation] = useState("");
  const [selectedArchive,setSelectedArchive] = useState<any>(null);
  const readyPreview = useMemo(() => (previews.data?.previews || []).find((value:any) => value.status === "ready"),[previews.data]);
  const refresh = () => { inventory.refresh(); archives.refresh(); previews.refresh(); receipts.refresh(); };
  const createPreview = async () => {
    setBusy(true); setError("");
    try { await sendJson("/api/system/retention/previews","POST",{actor:operatorName,reason}); refresh(); }
    catch(caught) { setError(caught instanceof Error ? caught.message : String(caught)); }
    finally { setBusy(false); }
  };
  const executePreview = async () => {
    if(!readyPreview) return;
    setBusy(true); setError("");
    try {
      await sendJson(`/api/system/retention/previews/${encodeURIComponent(readyPreview.id)}/execute`,"POST",{
        actor:operatorName, reason, state_hash:readyPreview.state_hash, confirmation,
      });
      setConfirmation(""); refresh();
    } catch(caught) {
      const value = caught as Error & {status?:number};
      if(value.status === 409) refresh();
      setError(value.message);
    } finally { setBusy(false); }
  };
  const deleteArchive = async () => {
    if(!selectedArchive) return;
    setBusy(true); setError("");
    try {
      await sendJson(`/api/system/archives/${encodeURIComponent(selectedArchive.id)}/delete`,"POST",{
        actor:operatorName, reason, state_hash:selectedArchive.deletion_action.state_hash, confirmation:archiveConfirmation,
      });
      setArchiveConfirmation(""); setSelectedArchive(null); refresh();
    } catch(caught) {
      const value = caught as Error & {status?:number};
      if(value.status === 409) { setSelectedArchive(null); archives.refresh(); }
      setError(value.message);
    } finally { setBusy(false); }
  };
  const data = inventory.data?.inventory;
  const retainedBytes = data?.retained_bytes || {};
  const totalBytes = Object.values(retainedBytes).reduce((sum:number,value:any) => sum + Number(value || 0),0);
  return <ResourceState status={inventory.status} error={inventory.error}>
    <div className="repo-two-columns">
      <section className="repo-panel"><header><span><Database size={20}/><h2>Active database generation</h2></span><Status value={data?.database_generation ? "aligned" : "unavailable"}/></header><dl className="repo-bindings"><div><dt>Generation</dt><dd className="repo-mono">{data?.database_generation?.id || "Not initialized"}</dd></div><div><dt>Schema</dt><dd>{data?.database_generation?.schema_version || "Unavailable"}</dd></div><div><dt>Purpose</dt><dd>{data?.database_generation?.purpose || "Unavailable"}</dd></div><div><dt>Initialized by revision</dt><dd className="repo-mono">{data?.database_generation?.initializing_revision || "Unavailable"}</dd></div></dl></section>
      <section className="repo-panel"><header><span><Broom size={20}/><h2>Retention policy</h2></span><Status value={inventory.data?.policy?.automatic_execution ? "automatic" : "preview only"}/></header><dl className="repo-bindings"><div><dt>Ephemeral workspace</dt><dd>{inventory.data?.policy?.workspace_days} days</dd></div><div><dt>Raw Run payload</dt><dd>{inventory.data?.policy?.raw_run_payload_days} days</dd></div><div><dt>Sealed evidence</dt><dd>{humanize(inventory.data?.policy?.evidence_retention)}</dd></div><div><dt>Stored payload estimate</dt><dd>{formatBytes(totalBytes)}</dd></div></dl></section>
      <section className="repo-panel repo-span-2"><header><span><Archive size={20}/><h2>Retained archives</h2></span><span className="repo-count">{archives.data?.count || 0}</span></header><div className="repo-list">{(archives.data?.archives || []).map((archive:any) => <article className="repo-list-row" key={archive.id}><div><strong>{archive.archived_generation_id}</strong><span>Database claim {archive.database_claim} · archive claim {archive.archive_claim}</span><small>Deletion eligible {formatMoment(archive.deletion_eligible_at)} · explicit confirmation remains required</small>{(archive.deletion_action?.blockers || []).map((blocker:any) => <small key={blocker.code}>{blocker.summary}</small>)}</div><div><Status value={archive.status}/>{archive.deletion_action?.status === "ready" ? <button className="deny" type="button" onClick={() => {setSelectedArchive(archive);setArchiveConfirmation("");}}>Review deletion</button> : null}</div></article>)}{!archives.data?.count ? <p className="repo-muted">No prior database generation is recorded in this generation.</p> : null}</div>{selectedArchive ? <div className="repo-effect is-external"><Archive size={20}/><div><strong>Destructive archive deletion</strong><p>{selectedArchive.deletion_action.external_effect_summary}</p><label>Exact confirmation<input aria-label="Archive deletion confirmation" placeholder={selectedArchive.deletion_action.confirmation} value={archiveConfirmation} onChange={event => setArchiveConfirmation(event.target.value)}/></label><div><button className="deny" type="button" disabled={busy || archiveConfirmation !== selectedArchive.deletion_action.confirmation} onClick={deleteArchive}>Delete exact retained PVCs</button><button type="button" onClick={() => setSelectedArchive(null)}>Cancel</button></div></div></div> : null}</section>
      <section className="repo-panel"><header><span><LockKey size={20}/><h2>Active holds</h2></span><span className="repo-count">{data?.active_holds || 0}</span></header><div className="repo-list">{(inventory.data?.holds || []).filter((hold:any) => !hold.released_at).map((hold:any) => <article className="repo-list-row" key={hold.id}><div><strong>{hold.subject_kind} · {hold.subject_id}</strong><span>{hold.reason}</span><small>{hold.expires_at ? `Expires ${formatMoment(hold.expires_at)}` : "No automatic expiry"}</small></div><Status value="held"/></article>)}{!data?.active_holds ? <p className="repo-muted">No active retention holds.</p> : null}</div></section>
      <section className="repo-panel"><header><span><Receipt size={20}/><h2>Inventory by record</h2></span></header><div className="repo-metric-grid">{Object.entries(data?.table_counts || {}).map(([name,value]) => <div key={name}><span>{humanize(name)}</span><strong>{String(value)}</strong></div>)}</div></section>
      <section className="repo-panel repo-span-2"><header><div><span className="repo-eyebrow">State-hashed execution</span><h2>Cleanup review</h2></div><Status value={readyPreview ? "ready" : "not previewed"}/></header><p>PHarness computes exact eligible IDs for this database generation. Active, held, externally waiting, mounted, or evidence-protected aggregates remain excluded.</p><div className="repo-inline-form"><label className="repo-span-2">Operator reason<input value={reason} onChange={event => setReason(event.target.value)}/></label><button className="repo-primary" type="button" disabled={busy || !reason.trim()} onClick={createPreview}>Create 15-minute preview</button>{readyPreview ? <><label className="repo-span-2">Exact confirmation<input aria-label="Retention confirmation" placeholder={`EXECUTE RETENTION ${readyPreview.id}`} value={confirmation} onChange={event => setConfirmation(event.target.value)}/></label><button type="button" disabled={busy || confirmation !== `EXECUTE RETENTION ${readyPreview.id}`} onClick={executePreview}>Execute exact preview</button></> : null}</div>{readyPreview ? <pre className="repo-code">{JSON.stringify(readyPreview.preview,null,2)}</pre> : null}{error ? <div className="repo-error" role="alert">{error}</div> : null}</section>
      <section className="repo-panel repo-span-2"><header><h2>Immutable cleanup receipts</h2><span className="repo-count">{receipts.data?.count || 0}</span></header><div className="repo-list">{(receipts.data?.receipts || []).map((item:any) => <article className="repo-list-row" key={item.id}><div><strong>{item.id}</strong><span>{item.status} · {item.policy_version}</span><small>{formatMoment(item.created_at)} · {item.content_hash}</small></div><Status value={item.status}/></article>)}{!receipts.data?.count ? <Empty title="No cleanup has executed" message="Preview-only operation is the rollout default."/> : null}</div></section>
    </div>
  </ResourceState>;
}

function formatBytes(value:number) {
  if(value < 1024) return `${value} B`;
  if(value < 1024*1024) return `${(value/1024).toFixed(1)} KiB`;
  return `${(value/1024/1024).toFixed(1)} MiB`;
}
