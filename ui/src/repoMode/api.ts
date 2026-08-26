type RequestOptions = { signal?: AbortSignal };

async function responseJson(response: Response) {
  if (response.ok) return response.json();
  const body = await response.text();
  const error = new Error(`${response.status} ${response.statusText}${body ? `: ${body}` : ""}`) as Error & { status?: number };
  error.status = response.status;
  throw error;
}

export function getJson(path: string, options: RequestOptions = {}) {
  return fetch(path, { headers: { accept: "application/json" }, signal: options.signal }).then(responseJson);
}

export function sendJson(path: string, method: "POST" | "PUT" | "PATCH", body: unknown) {
  return fetch(path, {
    method,
    headers: { accept: "application/json", "content-type": "application/json" },
    body: JSON.stringify(body),
  }).then(responseJson);
}

export function query(path: string, values: Record<string, unknown>) {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(values)) {
    if (value !== undefined && value !== null && value !== "") params.set(key, String(value));
  }
  return params.size ? `${path}?${params}` : path;
}
