// Browser calls to the Campus API. Every call is same-origin and carries
// the Campus session cookie; state-changing calls carry the CSRF token
// returned by /api/campus/me.

export class CampusApiError extends Error {
  status: number;
  code: string | null;

  constructor(status: number, code: string | null, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

type RequestOptions = { method?: string; csrf?: string; body?: unknown };

async function request<T>(path: string, { method = "GET", csrf, body }: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { accept: "application/json" };
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
    throw new CampusApiError(response.status, data?.code ?? null, data?.error ?? `request failed with ${response.status}`);
  }
  return data as T;
}

export type CampusMe = { csrf_token: string; identity: { email?: string | null; display_name?: string | null } };
export type CampusLabSession = {
  session_id: string;
  course_id: string;
  assignment_id: string;
  status: string;
  stop_requested: boolean;
  current: boolean;
  expires_at: number;
};
export type TerminalTicket = { ticket: string; expires_at: number; socket_path: string; protocol: string };

const sessionPath = (id: string) => `/api/campus/lab-sessions/${encodeURIComponent(id)}`;

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

export const SIGN_IN_PATH = "/api/campus/auth/login";
