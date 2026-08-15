import { useState, type ReactNode } from "react";
import { Copy, MagnifyingGlass } from "@phosphor-icons/react";
import { compactId } from "../lib/formatters";

type Tone = string | undefined;

type StatusPillProps = {
  tone: Tone;
  children: ReactNode;
};

export function StatusPill({ tone, children }: StatusPillProps) {
  return <span className={`pill pill-${tone ?? "future"}`}>{children}</span>;
}

type IconButtonProps = {
  label: string;
  children: ReactNode;
  onClick?: () => void;
  active?: boolean;
};

export function IconButton({ label, children, onClick, active = false }: IconButtonProps) {
  return (
    <button className={`icon-button ${active ? "is-active" : ""}`} type="button" aria-label={label} title={label} onClick={onClick}>
      {children}
    </button>
  );
}

type ReviewItemProps = {
  label: string;
  value: ReactNode;
  tone?: string;
};

export function ReviewItem({ label, value, tone }: ReviewItemProps) {
  return (
    <div className={`review-item ${tone ? `tone-${tone}` : ""}`}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

export function CopyIdentifier({ value, label }: { value?: string | null; label?: string }) {
  const [copied, setCopied] = useState(false);
  if (!value) return <span>not scoped</span>;
  const copy = async () => {
    try { await navigator.clipboard.writeText(value); setCopied(true); }
    catch { setCopied(false); }
  };
  return <button className="copy-identifier" type="button" title={value} aria-label={`Copy full ${label ?? "identifier"}`} onClick={copy}><span>{label ? `${label} · ` : ""}{compactId(value)}</span><Copy size={14} />{copied ? <small>Copied</small> : null}</button>;
}

type EmptyStateProps = {
  title: string;
  body: string;
};

export function EmptyState({ title, body }: EmptyStateProps) {
  return (
    <div className="empty-state">
      <strong>{title}</strong>
      <p>{body}</p>
    </div>
  );
}

export function ReadinessFacts({ readiness }: { readiness: any }) {
  const facts = [...(readiness?.blockers ?? []), ...(readiness?.warnings ?? [])];
  if (!facts.length) {
    return <p className="compact-copy">No blockers or warnings are currently reported for this resource.</p>;
  }
  return <div className="fact-list">
    {facts.slice(0, 6).map((fact: any, index: number) => <div key={`${fact.code}-${index}`}>
      <span className={`dot ${index < (readiness?.blockers?.length ?? 0) ? "blocked" : "pending"}`} />
      <strong>{fact.code}</strong><p>{fact.message}</p>
    </div>)}
  </div>;
}

export type OperatorListFiltersValue = {
  search: string;
  status: string;
  actor: string;
  origin: string;
};

type OperatorListFiltersProps = {
  value: OperatorListFiltersValue;
  statuses: string[];
  actors: string[];
  origins: string[];
  onChange: (value: OperatorListFiltersValue) => void;
};

export function OperatorListFilters({ value, statuses, actors, origins, onChange }: OperatorListFiltersProps) {
  const set = (key: keyof OperatorListFiltersValue, next: string) => onChange({ ...value, [key]: next });
  const clear = () => onChange({ search: "", status: "", actor: "", origin: "" });
  return (
    <div className="operator-list-filters" aria-label="Operational list filters">
      <label className="operator-list-search"><span>Search</span><div><MagnifyingGlass size={15} /><input value={value.search} onChange={(event) => set("search", event.target.value)} placeholder="Title, resource, summary..." /></div></label>
      <label>Status<select value={value.status} onChange={(event) => set("status", event.target.value)}><option value="">All statuses</option>{statuses.map((item) => <option value={item} key={item}>{item}</option>)}</select></label>
      <label>Actor<select value={value.actor} onChange={(event) => set("actor", event.target.value)}><option value="">All actors</option>{actors.map((item) => <option value={item} key={item}>{item}</option>)}</select></label>
      <label>Origin<select value={value.origin} onChange={(event) => set("origin", event.target.value)}><option value="">All origins</option>{origins.map((item) => <option value={item} key={item}>{item}</option>)}</select></label>
      <button type="button" onClick={clear}>Clear</button>
    </div>
  );
}
