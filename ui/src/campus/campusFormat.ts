// Plain-language wording for Campus limits, states and times.

import type { CampusLabLimits, CampusLabSession, PolicySource } from "./campusApi";

export function formatDuration(totalSeconds: number): string {
  const seconds = Math.max(0, Math.floor(totalSeconds));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours && minutes) return `${hours} h ${minutes} min`;
  if (hours) return hours === 1 ? "1 hour" : `${hours} hours`;
  if (minutes) return `${minutes} min`;
  return `${seconds} s`;
}

export function formatClock(totalSeconds: number): string {
  const seconds = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

export function formatTime(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

export function formatDollars(microUsd: number): string {
  return `$${(microUsd / 1_000_000).toFixed(2)}`;
}

export function formatMemory(mib: number): string {
  return mib >= 1024 && mib % 1024 === 0 ? `${mib / 1024} GB` : mib >= 1024 ? `${(mib / 1024).toFixed(1)} GB` : `${mib} MB`;
}

export function formatCpu(millicores: number): string {
  const cores = millicores / 1000;
  return cores === 1 ? "1 CPU core" : `${Number.isInteger(cores) ? cores : cores.toFixed(1)} CPU cores`;
}

const SOURCE_LABELS: Record<PolicySource, string> = {
  profile: "lab profile",
  platform: "platform",
  institution: "institution",
  assignment: "instructor",
  student: "you",
};

export function setByLabel(limits: CampusLabLimits, ...dimensions: string[]): string | null {
  for (const dimension of dimensions) {
    const source = limits.set_by[dimension];
    if (source) return `set by ${SOURCE_LABELS[source] ?? source}`;
  }
  return null;
}

export type LimitRow = { label: string; value: string; source: string | null };

export function limitRows(limits: CampusLabLimits): LimitRow[] {
  const network =
    limits.network === "denied"
      ? "No internet. The lab reaches only the campus AI gateway."
      : `Approved sites only: ${limits.egress_hosts.join(", ")}`;
  return [
    { label: "Time", value: `Up to ${formatDuration(limits.ttl_seconds)}`, source: setByLabel(limits, "ttl_seconds") },
    {
      label: "Session budget",
      value: `${formatDollars(limits.session_allocation_micro_usd)} of AI usage`,
      source: setByLabel(limits, "session_allocation_micro_usd"),
    },
    { label: "AI models", value: limits.model_aliases.join(", "), source: setByLabel(limits, "model_aliases") },
    {
      label: "Computer",
      value: `${formatCpu(limits.cpu_millicores)}, ${formatMemory(limits.memory_mib)} memory, ${formatMemory(limits.ephemeral_storage_mib)} of files`,
      source: setByLabel(limits, "cpu_millicores", "memory_mib", "ephemeral_storage_mib"),
    },
    { label: "Internet", value: network, source: setByLabel(limits, "egress") },
  ];
}

export type StateTone = "good" | "info" | "warn" | "bad" | "neutral";

export function sessionState(session: Pick<CampusLabSession, "status" | "stop_requested">): { label: string; tone: StateTone } {
  if (session.stop_requested && session.status !== "stopped") return { label: "Stopping", tone: "warn" };
  switch (session.status) {
    case "provisioning":
      return { label: "Starting", tone: "info" };
    case "ready":
      return { label: "Running", tone: "good" };
    case "stopping":
      return { label: "Stopping", tone: "warn" };
    case "cleanup_blocked":
      return { label: "Cleanup blocked", tone: "bad" };
    case "stopped":
      return { label: "Ended", tone: "neutral" };
    case "failed":
      return { label: "Failed", tone: "bad" };
    default:
      return { label: session.status, tone: "neutral" };
  }
}

export function courseLabel(courseId: string): string {
  return courseId.toUpperCase().replace(/^([A-Z]+)(\d+)$/, "$1-$2");
}

export function newIdempotencyKey(): string {
  const random = typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : `${Date.now()}-${Math.random()}`;
  return `launch-${random}`.replace(/[^A-Za-z0-9._:-]/g, "").slice(0, 128);
}
