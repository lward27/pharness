// Browser calls to the Campus API. Every call is same-origin and carries
// the Campus session cookie; state-changing calls carry the CSRF token
// returned by /api/campus/me.

export class CampusApiError extends Error {
  status: number;
  code: string | null;
  sessionId: string | null;

  constructor(status: number, code: string | null, message: string, sessionId: string | null = null) {
    super(message);
    this.status = status;
    this.code = code;
    this.sessionId = sessionId;
  }
}

type RequestOptions = { method?: string; csrf?: string; body?: unknown; headers?: Record<string, string> };

async function request<T>(path: string, { method = "GET", csrf, body, headers: extra }: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { accept: "application/json", ...extra };
  if (csrf) headers["x-campus-csrf"] = csrf;
  if (body !== undefined) headers["content-type"] = "application/json";
  const response = await fetch(path, {
    method,
    credentials: "same-origin",
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new CampusApiError(
      response.status,
      data?.code ?? null,
      data?.error ?? `request failed with ${response.status}`,
      data?.session_id ?? null,
    );
  }
  return data as T;
}

export type CampusRole = "student" | "instructor";

export type CampusMe = {
  csrf_token: string;
  tenant_id?: string;
  identity: { id?: string; issuer?: string; subject?: string; email?: string | null; display_name?: string | null };
  is_tenant_admin?: boolean;
  memberships?: { course_id: string; role: CampusRole }[];
  session?: { idle_expires_at: number; absolute_expires_at: number };
};

export type CampusLabStatus = "provisioning" | "ready" | "stopping" | "cleanup_blocked" | "stopped" | "failed" | string;

export type CampusLabSession = {
  session_id: string;
  course_id: string;
  assignment_id: string;
  status: CampusLabStatus;
  state?: string;
  stop_requested: boolean;
  current: boolean;
  reset_pending?: boolean;
  generation?: number;
  profile?: { id: string; version: string };
  error?: { code: string; message: string } | null;
  created_at?: number;
  updated_at?: number;
  expires_at: number;
  terminated_at?: number | null;
  owner?: { issuer: string; subject: string };
};

export type PolicySource = "profile" | "platform" | "institution" | "assignment" | "student";

export type CampusLabLimits = {
  cpu_millicores: number;
  memory_mib: number;
  ephemeral_storage_mib: number;
  ttl_seconds: number;
  network: "denied" | "approved_egress";
  egress_hosts: string[];
  model_aliases: string[];
  session_allocation_micro_usd: number;
  max_output_tokens: number;
  set_by: Partial<Record<string, PolicySource>>;
};

export type CampusAssignment = {
  assignment_id: string;
  title: string;
  profile: { id: string; version: string; kind: string };
  session_seconds: number;
  limits?: CampusLabLimits;
  launchable: boolean;
  session: CampusLabSession | null;
};

export type CampusMember = {
  issuer: string;
  subject: string;
  role: CampusRole;
  email?: string | null;
  display_name?: string | null;
};

export type TerminalTicket = { ticket: string; expires_at: number; socket_path: string; protocol: string };

const sessionPath = (id: string) => `/api/campus/lab-sessions/${encodeURIComponent(id)}`;
const coursePath = (id: string) => `/api/campus/courses/${encodeURIComponent(id)}`;

export type CampusApi = {
  me: () => Promise<CampusMe>;
  session: (id: string) => Promise<{ session: CampusLabSession }>;
  ticket: (id: string, csrf: string) => Promise<TerminalTicket>;
  stop: (id: string, csrf: string) => Promise<{ session: CampusLabSession }>;
  logout: (csrf: string) => Promise<unknown>;
};

export const campusApi: CampusApi = {
  me: () => request("/api/campus/me"),
  session: id => request(sessionPath(id)),
  ticket: (id, csrf) => request(`${sessionPath(id)}/terminal-ticket`, { method: "POST", csrf, body: {} }),
  stop: (id, csrf) => request(`${sessionPath(id)}/stop`, { method: "POST", csrf, body: {} }),
  logout: csrf => request("/api/campus/auth/logout", { method: "POST", csrf, body: {} }),
};

/** The calls the Campus pages make beyond the terminal. */
export type CampusPortalApi = CampusApi & {
  assignments: (courseId: string) => Promise<{ course_id: string; assignments: CampusAssignment[] }>;
  launch: (
    courseId: string,
    assignmentId: string,
    csrf: string,
    idempotencyKey: string,
  ) => Promise<{ admission: "created" | "existing" | "replayed"; session: CampusLabSession }>;
  mySessions: () => Promise<{ sessions: CampusLabSession[] }>;
  courseSessions: (courseId: string) => Promise<{ course_id: string; sessions: CampusLabSession[] }>;
  members: (courseId: string) => Promise<{ course_id: string; members: CampusMember[] }>;
};

export const campusPortalApi: CampusPortalApi = {
  ...campusApi,
  assignments: courseId => request(`${coursePath(courseId)}/assignments`),
  launch: (courseId, assignmentId, csrf, idempotencyKey) =>
    request(`${coursePath(courseId)}/assignments/${encodeURIComponent(assignmentId)}/launch`, {
      method: "POST",
      csrf,
      body: {},
      headers: { "idempotency-key": idempotencyKey },
    }),
  mySessions: () => request("/api/campus/lab-sessions"),
  courseSessions: courseId => request(`${coursePath(courseId)}/lab-sessions`),
  members: courseId => request(`${coursePath(courseId)}/members`),
};

export const SIGN_IN_PATH = "/api/campus/auth/login";

export const LIVE_STATUSES = new Set(["provisioning", "ready", "stopping", "cleanup_blocked"]);
