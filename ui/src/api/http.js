export const JSON_HEADERS = { accept: "application/json" };

const WRITE_HEADERS = { ...JSON_HEADERS, "content-type": "application/json" };

export async function fetchJson(path, { optional = false, signal } = {}) {
  const response = await fetch(path, { headers: JSON_HEADERS, ...(signal ? { signal } : {}) });
  if (optional && response.status === 404) return null;
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${response.status} ${response.statusText}${text ? `: ${text}` : ""}`);
  }
  return response.json();
}

export async function postJson(path, body) {
  const response = await fetch(path, {
    method: "POST",
    headers: WRITE_HEADERS,
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${response.status} ${response.statusText}${text ? `: ${text}` : ""}`);
  }
  return response.json();
}

export function withQuery(path, values) {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(values)) {
    if (value !== undefined && value !== null && value !== "") params.set(key, String(value));
  }
  const query = params.toString();
  return query ? `${path}?${query}` : path;
}

export async function firstListItem(path, key) {
  const payload = await fetchJson(`${path}?limit=1`);
  const items = Array.isArray(payload?.[key]) ? payload[key] : [];
  return items[0] ?? null;
}

export function executionScopeQuery(scope = {}) {
  return {
    namespace: scope.namespace,
    repo: scope.repo,
    branch: scope.branch,
    production_impacting: scope.productionImpacting,
  };
}
