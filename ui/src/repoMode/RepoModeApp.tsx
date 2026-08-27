import { useEffect, useMemo, useRef, useState, type ComponentType } from "react";
import { ChartLineUp, Cube, Gear, GitBranch, House, List, MagnifyingGlass, Moon, Package, Pulse, Rows, Sun, X } from "@phosphor-icons/react";
import { OverviewScreen } from "./screens/OverviewScreen";
import { ProductScreen, ProductsScreen } from "./screens/ProductsScreen";
import { OnboardingScreen, RepositoriesScreen, RepositoryScreen } from "./screens/RepositoriesScreen";
import { NewWorkItemScreen, WorkItemsScreen, WorkItemScreen } from "./screens/WorkItemsScreen";
import { AgentRunScreen, AgentsScreen, CompatibilityScreen, InsightsScreen, ReleasesScreen, SettingsScreen } from "./screens/PlatformScreens";
import { getJson, query } from "./api";
import { navigate, parseRoute, primaryRoutes, type AppRoute } from "./routes";
import { useResource } from "./useResource";

const icons:Record<string,ComponentType<any>> = { overview:House, products:Cube, repositories:GitBranch, workItems:Rows, agents:Pulse, releases:Package, insights:ChartLineUp, settings:Gear };

export function RepoModeApp() {
  const [route,setRoute] = useState<AppRoute>(() => parseRoute());
  const [theme,setTheme] = useState(() => window.localStorage.getItem("pharness.theme") || "dark");
  const [menuOpen,setMenuOpen] = useState(false);
  const [searchOpen,setSearchOpen] = useState(false);
  const config = useResource<any>("/api/config/effective");
  const overview = useResource<any>("/api/organization/overview",{pollMs:20_000});
  useEffect(() => { const update = () => { const parsed = parseRoute(); setRoute(parsed); setMenuOpen(false); if (parsed.name !== "compatibility" && parsed.canonicalHash !== window.location.hash) window.history.replaceState(null,"",parsed.canonicalHash); }; window.addEventListener("hashchange",update); update(); return () => window.removeEventListener("hashchange",update); },[]);
  useEffect(() => { window.localStorage.setItem("pharness.theme",theme); document.documentElement.style.colorScheme = theme; },[theme]);
  useEffect(() => { const shortcut = (event:KeyboardEvent) => { if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") { event.preventDefault(); setSearchOpen(true); } }; window.addEventListener("keydown",shortcut); return () => window.removeEventListener("keydown",shortcut); },[]);
  const attention = overview.data?.attention?.length || 0;
  const operatorName = config.data?.operator?.name || "operator";
  const controllerState = overview.status === "error" ? "Unavailable" : overview.status === "loading" ? "Connecting" : "Connected";
  return <div className={`repo-app theme-${theme}`}>
    <aside className={`repo-sidebar ${menuOpen ? "is-open" : ""}`}>
      <header className="repo-brand"><div><GitBranch size={24} weight="bold" /></div><span><strong>PHarness</strong><small>Engineering control plane</small></span><button type="button" aria-label="Close navigation" onClick={() => setMenuOpen(false)}><X size={19} /></button></header>
      <nav aria-label="Primary navigation">{primaryRoutes.map(([name,label,path]) => { const Icon = icons[name]; const active = route.name === name || (name === "products" && ["product","newWorkItem"].includes(route.name)) || (name === "repositories" && ["repository","onboarding"].includes(route.name)) || (name === "workItems" && route.name === "workItem") || (name === "agents" && route.name === "agentRun"); const badge = name === "overview" && attention ? attention : null; return <button type="button" className={active ? "is-active" : ""} onClick={() => navigate(path)} key={name}><Icon size={20} /><span>{label}</span>{badge ? <b aria-label={`${badge} attention items`}>{badge}</b> : null}</button>; })}</nav>
      <footer><div><span className={`repo-live-dot is-${controllerState.toLowerCase()}`} /><span><small>Controller</small><strong>{controllerState}</strong></span></div><small className="repo-mono">{overview.data?.as_of || "No current projection"}</small></footer>
    </aside>
    <main className="repo-main">
      <header className="repo-topbar"><button className="repo-menu" type="button" aria-label="Open navigation" onClick={() => setMenuOpen(true)}><List size={21} /></button><button className="repo-search-trigger" type="button" onClick={() => setSearchOpen(true)}><MagnifyingGlass size={19} /><span>Search Products, Repositories, WorkItems, AgentRuns…</span><kbd>⌘K</kbd></button><div className="repo-theme"><button type="button" aria-label="Use dark theme" className={theme === "dark" ? "is-active" : ""} onClick={() => setTheme("dark")}><Moon size={17} /></button><button type="button" aria-label="Use light theme" className={theme === "light" ? "is-active" : ""} onClick={() => setTheme("light")}><Sun size={17} /></button></div><div className="repo-operator"><span>{operatorName.slice(0,2).toUpperCase()}</span><div><strong>{operatorName}</strong><small>Operator</small></div></div></header>
      <div className="repo-content"><RouteContent route={route} operatorName={operatorName} /></div>
    </main>
    {menuOpen ? <button className="repo-nav-scrim" type="button" aria-label="Close navigation" onClick={() => setMenuOpen(false)} /> : null}
    {searchOpen ? <SearchDialog onClose={() => setSearchOpen(false)} /> : null}
  </div>;
}

function RouteContent({ route,operatorName }: { route:AppRoute; operatorName:string }) {
  switch(route.name) {
    case "overview": return <OverviewScreen />;
    case "products": return <ProductsScreen operatorName={operatorName} />;
    case "product": return <ProductScreen productId={route.params.productId} section={route.section || "work-items"} operatorName={operatorName} />;
    case "repositories": return <RepositoriesScreen operatorName={operatorName} />;
    case "repository": return <RepositoryScreen repositoryId={route.params.repositoryId} section={route.section || "overview"} operatorName={operatorName} />;
    case "onboarding": return <OnboardingScreen onboardingId={route.params.onboardingId} operatorName={operatorName} />;
    case "workItems": return <WorkItemsScreen />;
    case "newWorkItem": return <NewWorkItemScreen productId={route.params.productId} operatorName={operatorName} />;
    case "workItem": return <WorkItemScreen workItemId={route.params.workItemId} section={route.section || "overview"} operatorName={operatorName} />;
    case "agents": return <AgentsScreen />;
    case "agentRun": return <AgentRunScreen runId={route.params.runId} operatorName={operatorName} />;
    case "releases": return <ReleasesScreen />;
    case "insights": return <InsightsScreen section={route.section || "audit"} />;
    case "settings": return <SettingsScreen section={route.section || "platform"} operatorName={operatorName} />;
    case "compatibility": return <CompatibilityScreen root={route.params.root} id={route.params.id} nestedId={route.params.nestedId} />;
  }
}

function SearchDialog({ onClose }: { onClose:()=>void }) {
  const [value,setValue] = useState("");
  const [results,setResults] = useState<any[]>([]);
  const [status,setStatus] = useState("idle");
  const dialogRef = useRef<HTMLElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const handleKey = (event:KeyboardEvent) => {
      if(event.key === "Escape") onClose();
      if(event.key === "Tab") {
        const focusable = Array.from(dialogRef.current?.querySelectorAll<HTMLElement>("button,input") || []).filter(element => !element.hasAttribute("disabled"));
        if (!focusable.length) return;
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if(event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
        else if(!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
      }
    };
    window.addEventListener("keydown",handleKey);
    return () => { window.removeEventListener("keydown",handleKey); previous?.focus(); };
  },[onClose]);
  useEffect(() => { if(value.trim().length < 2) { setResults([]);setStatus("idle");return; } const timer = window.setTimeout(async () => { setStatus("loading"); try { const response = await getJson(query("/api/search",{q:value.trim(),limit:20})); setResults(response.results || []);setStatus("ready"); } catch { setStatus("error"); } },200); return () => window.clearTimeout(timer); },[value]);
  const grouped = useMemo(() => results.reduce((groups:any,result:any) => { (groups[result.kind] ||= []).push(result); return groups; },{}),[results]);
  const open = (result:any) => { if(result.kind === "product") navigate(`products/${result.id}/work-items`); else if(result.kind === "repository") navigate(`repositories/${result.id}/overview`); else if(result.kind === "work_item") navigate(`work-items/${result.id}/overview`); else navigate(`agents/runs/${result.id}`); onClose(); };
  return <div className="repo-search-backdrop" onMouseDown={event => { if(event.currentTarget === event.target) onClose(); }}><section ref={dialogRef} className="repo-search-dialog" role="dialog" aria-modal="true" aria-label="Search PHarness"><header><MagnifyingGlass size={21} /><input autoFocus aria-label="Search Products, Repositories, WorkItems, and AgentRuns" value={value} onChange={event => setValue(event.target.value)} placeholder="Search durable resources" /><button type="button" aria-label="Close search" onClick={onClose}><X size={18} /></button></header><div role="status" className="repo-search-status">{status === "loading" ? "Searching…" : status === "error" ? "Search unavailable" : value.length < 2 ? "Type at least two characters" : `${results.length} results`}</div><div className="repo-search-results">{Object.entries(grouped).map(([kind,items]:any) => <section key={kind}><h2>{kind.replace("_"," ")}</h2>{items.map((item:any) => <button type="button" key={item.id} onClick={() => open(item)}><span><strong>{item.label}</strong><small className="repo-mono">{item.id}</small></span><small>{item.status}</small></button>)}</section>)}</div></section></div>;
}
