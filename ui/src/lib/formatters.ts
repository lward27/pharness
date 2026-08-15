const COMPLETED = new Set(["completed", "succeeded", "approved", "verified", "satisfied", "ready", "merged"]);
const ACTIVE = new Set(["running", "executing", "in_progress"]);
const BLOCKED = new Set(["blocked", "rejected", "failed", "stale"]);
const PENDING = new Set(["draft", "proposed", "pending", "approval_required"]);

export function lifecycleTone(status?: string | null) {
  if (status && COMPLETED.has(status)) return "healthy";
  if (status && ACTIVE.has(status)) return "running";
  if (status && BLOCKED.has(status)) return "blocked";
  if (status && PENDING.has(status)) return "pending";
  return "future";
}

export function statusText(status?: string | null, fallback = "Future-backed") {
  if (!status) return fallback;
  return status
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

export function compactId(value?: string | null) {
  if (!value) return "not scoped";
  return value.length <= 18 ? value : `${value.slice(0, 10)}...${value.slice(-5)}`;
}

export function formatTimestamp(value?: string | number | null) {
  const millis = Number(value);
  if (!Number.isFinite(millis)) return "unknown";
  const delta = Date.now() - millis;
  if (delta < 0) return new Date(millis).toLocaleString();
  const seconds = Math.floor(delta / 1000);
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months}mo ago`;
  return `${Math.floor(months / 12)}y ago`;
}

export function timestampTitle(value?: string | number | null) {
  const millis = Number(value);
  return Number.isFinite(millis) ? new Date(millis).toLocaleString() : "Unknown time";
}

export function riskTone(risk?: string | null) {
  if (risk === "critical" || risk === "high") return "high";
  return risk === "medium" ? "medium" : "low";
}
