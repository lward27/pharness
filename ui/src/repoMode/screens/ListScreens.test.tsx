import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkItemsScreen } from "./WorkItemsScreen";
import { RepositoriesScreen } from "./RepositoriesScreen";
import { AgentsScreen } from "./PlatformScreens";
import { ListPagination } from "../ListPagination";

const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "content-type": "application/json" } });
const workItem = (id: string) => ({ id, title: id, intent: "A bounded change", status: "blocked", mode: "repo", repository_id: "repo-one" });
const cases = [
  { name: "WorkItem", component: WorkItemsScreen, path: "/api/work-items", key: "work_items", limit: 50, item: workItem("work-one") },
  { name: "Repository", component: RepositoriesScreen, path: "/api/repositories", key: "repositories", limit: 25, item: { id: "repo-one", canonical_url: "https://github.com/example/project" } },
  { name: "AgentRun", component: AgentsScreen, path: "/api/runs", key: "runs", limit: 50, item: { id: "run-one", status: "running" } },
];

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe("Lamina list results", () => {
  it.each(cases)("keeps $name navigation usable when the result set shrinks", async ({ name, component: Component, path, key, limit, item }) => {
    let shrunk = false;
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = new URL(String(input), "http://localhost");
      if (url.searchParams.get("mode") === "legacy") return json({ work_items: [], count: 0 });
      if (url.pathname !== path) return json({});
      const offset = Number(url.searchParams.get("offset"));
      if (offset > 0) { shrunk = true; return json({ [key]: [], count: 2 }); }
      return json({ [key]: [item], count: shrunk ? 2 : limit + 1 });
    });
    vi.stubGlobal("fetch", fetchMock);
    render(<Component />);
    const pages = await screen.findByRole("navigation", { name: `${name} pages` });
    fireEvent.click(within(pages).getByRole("button", { name: "Next" }));
    expect(await screen.findByRole("heading", { name: "No results on this page" })).toBeInTheDocument();
    const emptyPages = screen.getByRole("navigation", { name: `${name} pages` });
    expect(within(emptyPages).getByText("No results on this page · 2 matching")).toBeInTheDocument();
    expect(within(emptyPages).getByRole("button", { name: "Next" })).toBeDisabled();
    fireEvent.click(within(emptyPages).getByRole("button", { name: "Previous" }));
    await waitFor(() => expect(screen.queryByRole("heading", { name: "No results on this page" })).not.toBeInTheDocument());
    expect(within(screen.getByRole("navigation", { name: `${name} pages` })).getByText("1–1 of 2")).toBeInTheDocument();
    expect(fetchMock.mock.calls.every(([, init]) => !init || !init.method || init.method === "GET")).toBe(true);
  });

  it.each(cases.slice(0, 2))("keeps $name search focused while a new query loads and names filtered emptiness", async ({ name, component: Component, path, key, item }) => {
    let finishSearch!: (response: Response) => void;
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
      const url = new URL(String(input), "http://localhost");
      if (url.searchParams.get("mode") === "legacy") return json({ work_items: [], count: 0 });
      if (url.pathname !== path) return json({});
      if (url.searchParams.get("search")) return new Promise<Response>(resolve => { finishSearch = resolve; });
      return json({ [key]: [item], count: 1 });
    }));
    render(<Component />);
    await screen.findByRole("navigation", { name: `${name} pages` });
    const plural = name === "Repository" ? "Repositories" : "WorkItems";
    const search = screen.getByRole("textbox", { name: `Search ${plural}` });
    search.focus();
    fireEvent.change(search, { target: { value: "absent" } });
    await waitFor(() => expect(finishSearch).toBeTypeOf("function"));
    expect(screen.getByRole("textbox", { name: `Search ${plural}` })).toBe(search);
    expect(search).toHaveFocus();
    expect(screen.getByText("Loading current state…")).toBeInTheDocument();
    finishSearch(json({ [key]: [], count: 0 }));
    expect(await screen.findByRole("heading", { name: `No matching ${plural}` })).toBeInTheDocument();
    expect(search).toHaveFocus();
  });

  it("pages legacy work independently and resets both lists when the lifecycle changes", async () => {
    const requests: URL[] = [];
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
      const url = new URL(String(input), "http://localhost");
      requests.push(url);
      if (url.pathname !== "/api/work-items") return json({});
      const legacy = url.searchParams.get("mode") === "legacy";
      const offset = Number(url.searchParams.get("offset"));
      return json({ work_items: [workItem(`${legacy ? "legacy" : "current"}-${offset}`)], count: 51 });
    }));
    render(<WorkItemsScreen />);
    const legacyPages = await screen.findByRole("navigation", { name: "Legacy WorkItem pages" });
    fireEvent.click(within(legacyPages).getByRole("button", { name: "Next" }));
    expect(await screen.findByText("legacy-50")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "current-0" })).toBeInTheDocument();
    expect(requests.some(url => url.searchParams.get("mode") === "repo" && url.searchParams.get("offset") === "50")).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "History" }));
    expect(await screen.findByText("legacy-0")).toBeInTheDocument();
    expect(requests.filter(url => url.pathname === "/api/work-items" && url.searchParams.get("lifecycle") === "history").map(url => url.searchParams.get("offset"))).toEqual(["0", "0"]);
  });

  it("shows unavailable legacy records without hiding the primary list or claiming none exist", async () => {
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
      const url = new URL(String(input), "http://localhost");
      if (url.searchParams.get("mode") === "legacy") return new Response(JSON.stringify({ error: "Legacy inventory unavailable" }), { status: 503 });
      return json(url.pathname === "/api/work-items" ? { work_items: [workItem("Visible current work")], count: 1 } : {});
    }));
    render(<WorkItemsScreen />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Legacy inventory unavailable");
    expect(screen.getByRole("heading", { name: "Visible current work" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Legacy WorkItems" })).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Legacy WorkItem pages" })).not.toBeInTheDocument();
  });

  it("keeps the lifecycle switch usable during an unavailable run query", async () => {
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
      const url = new URL(String(input), "http://localhost");
      if (url.pathname === "/api/agent-profiles") return json({ agent_profiles: [] });
      if (url.searchParams.get("lifecycle") === "current") return new Response("{}", { status: 503 });
      return json({ runs: [{ id: "recorded-run", status: "completed" }], count: 1 });
    }));
    render(<AgentsScreen />);
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "Run History" }));
    expect(await screen.findByText("recorded-run")).toBeInTheDocument();
  });

  it("does not invent a total and recovers directly to the last available page", () => {
    const change = vi.fn();
    const view = render(<ListPagination label="Record" visibleCount={3} offset={0} limit={50} onOffsetChange={change} />);
    expect(screen.getByText("3 on this page · Total unavailable")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Next" })).not.toBeInTheDocument();
    view.rerender(<ListPagination label="Record" count={2} visibleCount={0} offset={150} limit={50} onOffsetChange={change} />);
    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(change).toHaveBeenCalledWith(0);
    view.rerender(<ListPagination label="Record" count={0} visibleCount={0} offset={50} limit={50} onOffsetChange={change} />);
    expect(screen.getByRole("button", { name: "Previous" })).toBeEnabled();
    expect(screen.getByText("0 matching")).toBeInTheDocument();
  });
});
