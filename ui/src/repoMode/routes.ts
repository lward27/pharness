export type RouteName =
  | "overview"
  | "products"
  | "product"
  | "repositories"
  | "repository"
  | "onboarding"
  | "workItems"
  | "newWorkItem"
  | "workItem"
  | "agents"
  | "agentRun"
  | "releases"
  | "insights"
  | "settings"
  | "compatibility";

export type AppRoute = {
  name: RouteName;
  params: Record<string, string>;
  section?: string;
  canonicalHash: string;
};

const decode = (value?: string) => value ? decodeURIComponent(value) : "";

export function route(path: string): string {
  return path.startsWith("#/") ? path : `#/${path.replace(/^\//, "")}`;
}

export function parseRoute(hash = window.location.hash): AppRoute {
  const parts = hash.replace(/^#\/?/, "").split("/").filter(Boolean).map(decode);
  const [root, id, section] = parts;
  if (!root) return { name: "overview", params: {}, canonicalHash: "#/overview" };
  if (root === "overview") return { name: "overview", params: {}, canonicalHash: "#/overview" };
  if (root === "products" && id && section === "work-items" && parts[3] === "new") {
    return { name: "newWorkItem", params: { productId: id }, canonicalHash: `#/products/${encodeURIComponent(id)}/work-items/new` };
  }
  if (root === "products" && id) return { name: "product", params: { productId: id }, section: section || "work-items", canonicalHash: `#/products/${encodeURIComponent(id)}/${encodeURIComponent(section || "work-items")}` };
  if (root === "products") return { name: "products", params: {}, canonicalHash: "#/products" };
  if (root === "repositories" && id) return { name: "repository", params: { repositoryId: id }, section: section || "overview", canonicalHash: `#/repositories/${encodeURIComponent(id)}/${encodeURIComponent(section || "overview")}` };
  if (root === "repositories") return { name: "repositories", params: {}, canonicalHash: "#/repositories" };
  if (root === "repository-onboardings" && id) return { name: "onboarding", params: { onboardingId: id }, canonicalHash: `#/repository-onboardings/${encodeURIComponent(id)}` };
  if (root === "work-items" && id) return { name: "workItem", params: { workItemId: id }, section: section || "overview", canonicalHash: `#/work-items/${encodeURIComponent(id)}/${encodeURIComponent(section || "overview")}` };
  if (root === "work-items") return { name: "workItems", params: {}, canonicalHash: "#/work-items" };
  if (root === "agents" && id === "runs" && section) return { name: "agentRun", params: { runId: section }, canonicalHash: `#/agents/runs/${encodeURIComponent(section)}` };
  if (root === "agents") return { name: "agents", params: {}, canonicalHash: "#/agents" };
  if (root === "releases") return { name: "releases", params: {}, canonicalHash: "#/releases" };
  if (root === "insights") return { name: "insights", params: {}, section: id || "audit", canonicalHash: `#/insights/${encodeURIComponent(id || "audit")}` };
  if (root === "settings") return { name: "settings", params: {}, section: id || "platform", canonicalHash: `#/settings/${encodeURIComponent(id || "platform")}` };

  if (root === "triage") return { name: "overview", params: {}, section: "attention", canonicalHash: "#/overview" };
  if (root === "queue") return { name: "agents", params: {}, canonicalHash: "#/agents" };
  if (root === "runs" && id) return { name: "agentRun", params: { runId: id }, canonicalHash: `#/agents/runs/${encodeURIComponent(id)}` };
  if (["approvals", "gates", "workplans", "flow"].includes(root)) return { name: "compatibility", params: { root, id: id || "", nestedId: section || "" }, canonicalHash: hash || `#/${root}` };
  if (["audit", "observations", "incidents", "remediation-plans"].includes(root)) return { name: "insights", params: {}, section: root === "remediation-plans" ? "remediation" : root, canonicalHash: `#/insights/${root === "remediation-plans" ? "remediation" : root}` };
  if (root === "status") return { name: "settings", params: {}, section: "platform", canonicalHash: "#/settings/platform" };
  return { name: "overview", params: {}, canonicalHash: "#/overview" };
}

export function navigate(path: string) {
  const next = route(path);
  if (window.location.hash !== next) window.location.hash = next;
}

export const primaryRoutes = [
  ["overview", "Overview", "overview"],
  ["products", "Products", "products"],
  ["repositories", "Repositories", "repositories"],
  ["workItems", "WorkItems", "work-items"],
  ["agents", "Agents", "agents"],
  ["releases", "Releases", "releases"],
  ["insights", "Insights", "insights/audit"],
  ["settings", "Settings", "settings/platform"],
] as const;
