type RequestOptions = { signal?: AbortSignal };

export type ApiRequestError = Error & {
  status?: number;
  body?: unknown;
};

async function responseJson(response: Response) {
  if (response.ok) return response.json();
  const body = await response.text();
  let detail = body.trim();
  let structured: unknown;
  try {
    const parsed = JSON.parse(body);
    structured = parsed;
    detail = typeof parsed?.error === "string" ? parsed.error : typeof parsed?.message === "string" ? parsed.message : "";
  } catch {
    if (detail.startsWith("<")) detail = "";
  }
  const fallback = `Request failed (${response.status} ${response.statusText})`.replace(/\s+\)/, ")");
  const error = new Error(detail || fallback) as ApiRequestError;
  error.status = response.status;
  error.body = structured;
  throw error;
}

export function getJson(path: string, options: RequestOptions = {}) {
  return fetch(path, { headers: { accept: "application/json" }, signal: options.signal }).then(responseJson);
}

export function sendJson(path: string, method: "POST" | "PUT" | "PATCH", body: unknown, options: RequestOptions = {}) {
  return fetch(path, {
    method,
    headers: { accept: "application/json", "content-type": "application/json" },
    body: JSON.stringify(body),
    signal: options.signal,
  }).then(responseJson);
}

export function query(path: string, values: Record<string, unknown>) {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(values)) {
    if (value !== undefined && value !== null && value !== "") params.set(key, String(value));
  }
  return params.size ? `${path}?${params}` : path;
}
