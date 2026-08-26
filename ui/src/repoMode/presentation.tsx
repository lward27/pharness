import { ArrowSquareOut, Clock, FileText, LinkSimple, WarningCircle } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import { Status } from "./components";

export function humanize(value?: string | null) {
  return String(value || "unavailable").replaceAll("_", " ").replaceAll("-", " ");
}

export function compactId(value?: string | null, length = 14) {
  if (!value) return "Unavailable";
  return value.length > length ? `${value.slice(0, length)}…` : value;
}

export function formatMoment(value?: string | null) {
  if (!value) return "Unavailable";
  const numeric = Number(value);
  const date = Number.isFinite(numeric) && numeric > 1_000_000_000_000
    ? new Date(numeric)
    : new Date(value);
  if (Number.isNaN(date.valueOf())) return value;
  return date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

export function formatAge(value?: string | null) {
  if (!value) return "age unavailable";
  const numeric = Number(value);
  const date = Number.isFinite(numeric) && numeric > 1_000_000_000_000
    ? new Date(numeric)
    : new Date(value);
  if (Number.isNaN(date.valueOf())) return "age unavailable";
  const elapsedSeconds = Math.max(0, Math.floor((Date.now() - date.valueOf()) / 1_000));
  if (elapsedSeconds < 60) return "just now";
  const minutes = Math.floor(elapsedSeconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function FactGrid({ facts, className = "" }: { facts: Array<{ label: string; value: ReactNode; mono?: boolean }>; className?: string }) {
  return <dl className={`repo-bindings ${className}`}>{facts.map(fact => <div key={fact.label}><dt>{fact.label}</dt><dd className={fact.mono ? "repo-mono" : undefined}>{fact.value ?? "Unavailable"}</dd></div>)}</dl>;
}

export function RecordList({ values, empty = "No records.", tone }: { values?: unknown[] | null; empty?: string; tone?: "warning" | "error" }) {
  if (!values?.length) return <p className="repo-muted">{empty}</p>;
  return <div className={`repo-structured-list ${tone ? `is-${tone}` : ""}`}>{values.map((value, index) => <StructuredRecord value={value} key={recordKey(value, index)} />)}</div>;
}

export function StructuredRecord({ value }: { value: unknown }) {
  if (value === null || value === undefined) return <div className="repo-structured-record"><span>Unavailable</span></div>;
  if (typeof value !== "object") return <div className="repo-structured-record"><span>{String(value)}</span></div>;
  if (Array.isArray(value)) return <div className="repo-structured-record"><span>{value.map(entry => primitive(entry)).join(" · ")}</span></div>;
  const entries = Object.entries(value as Record<string, unknown>);
  const headline = entries.find(([key]) => ["statement", "summary", "name", "kind", "key", "command", "status", "next"].includes(key));
  return <div className="repo-structured-record">
    <strong>{headline ? primitive(headline[1]) : humanize(entries[0]?.[0] || "record")}</strong>
    <dl>{entries.filter(([key]) => key !== headline?.[0]).map(([key, entry]) => <div key={key}><dt>{humanize(key)}</dt><dd>{primitive(entry)}</dd></div>)}</dl>
  </div>;
}

export function EvidenceReferences({ values, workItemId }: { values?: unknown[] | null; workItemId?: string }) {
  if (!values?.length) return <p className="repo-muted">No evidence references were attached.</p>;
  return <div className="repo-evidence-links">{values.map((value, index) => {
    const record = typeof value === "object" && value ? value as Record<string, unknown> : { id: String(value) };
    const kind = String(record.kind || "evidence");
    const id = String(record.id || record.hash || `reference-${index + 1}`);
    const href = evidenceHref(kind, id, workItemId);
    const content = <><LinkSimple size={16} /><span><strong>{humanize(kind)}</strong><small className="repo-mono">{id}</small></span>{href ? <ArrowSquareOut size={15} /> : null}</>;
    return href ? <a href={href} key={`${kind}:${id}`} target={href.startsWith("/api/") ? "_blank" : undefined} rel="noreferrer">{content}</a> : <div key={`${kind}:${id}`}>{content}</div>;
  })}</div>;
}

export function RawRecord({ label = "Raw immutable record", value, open = false }: { label?: string; value: unknown; open?: boolean }) {
  return <details className="repo-raw-record" open={open}><summary><FileText size={16} />{label}</summary><pre className="repo-code">{JSON.stringify(value ?? {}, null, 2)}</pre></details>;
}

export function Freshness({ status, observedAt, expiresAt, reasons = [] }: { status?: string; observedAt?: string | null; expiresAt?: string | null; reasons?: string[] }) {
  const expiry = expiresAt ? momentMillis(expiresAt) : null;
  const expired = expiry !== null && expiry <= Date.now();
  const effectiveStatus = expired ? "stale" : status || (reasons.length ? "stale" : "current");
  const effectiveReasons = expired && !reasons.includes("observation_expired") ? ["observation_expired", ...reasons] : reasons;
  return <div className="repo-freshness"><div><Clock size={17} /><span><strong>Evidence freshness</strong><small>{observedAt ? `Observed ${formatMoment(observedAt)}` : "Observation time unavailable"}{expiresAt ? ` · expires ${formatMoment(expiresAt)}` : ""}</small></span><Status value={effectiveStatus} /></div>{effectiveReasons.map(reason => <p key={reason}><WarningCircle size={15} />{humanize(reason)}</p>)}</div>;
}

function momentMillis(value:string) {
  const numeric = Number(value);
  const millis = Number.isFinite(numeric) && numeric > 1_000_000_000_000 ? numeric : new Date(value).valueOf();
  return Number.isFinite(millis) ? millis : null;
}

function primitive(value: unknown): string {
  if (value === null || value === undefined || value === "") return "Unavailable";
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") return String(value);
  if (Array.isArray(value)) return value.map(primitive).join(" · ") || "None";
  const record = value as Record<string, unknown>;
  return String(record.statement || record.summary || record.name || record.kind || record.id || JSON.stringify(record));
}

function recordKey(value: unknown, index: number) {
  if (typeof value === "object" && value) {
    const record = value as Record<string, unknown>;
    return String(record.id || record.hash || record.statement || record.summary || `${record.kind || "record"}-${index}`);
  }
  return `${String(value)}-${index}`;
}

function evidenceHref(kind: string, id: string, workItemId?: string) {
  if (kind.includes("artifact")) return `/api/artifacts/${encodeURIComponent(id)}`;
  if (kind.includes("observation") && kind !== "provider_check_set_observation") return `/api/observations/${encodeURIComponent(id)}`;
  if (kind.includes("evidence_validation")) return `/api/evidence-validations/${encodeURIComponent(id)}`;
  if (kind.includes("stage_execution")) return `/api/stage-executions/${encodeURIComponent(id)}`;
  if (workItemId) return `#/work-items/${encodeURIComponent(workItemId)}/evidence`;
  return null;
}
