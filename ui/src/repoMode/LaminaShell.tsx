import { useRef, type ReactNode } from "react";
import { ChartLineUp, Cube, Gear, GitBranch, House, List, MagnifyingGlass, Moon, Package, Pulse, Rows, Sun, X } from "@phosphor-icons/react";
import { navigate, primaryRoutes, type AppRoute } from "./routes";
import { useDialog } from "./useDialog";

const icons = { overview: House, products: Cube, repositories: GitBranch, workItems: Rows, agents: Pulse, releases: Package, insights: ChartLineUp, settings: Gear };
function Navigation({ route, attention, onNavigate }: { route: AppRoute; attention: number; onNavigate: () => void }) {
  return <nav aria-label="Primary navigation">{primaryRoutes.map(([name, label, path]) => {
    const Icon = icons[name];
    const active = route.name === name || (name === "products" && ["product", "newWorkItem"].includes(route.name)) || (name === "repositories" && ["repository", "onboarding"].includes(route.name)) || (name === "workItems" && route.name === "workItem") || (name === "agents" && route.name === "agentRun");
    return <button type="button" key={name} aria-current={active ? "page" : undefined} className={active ? "is-active" : ""} onClick={() => { navigate(path); onNavigate(); }}><Icon size={17} /><span>{label}</span>{name === "overview" && attention > 0 ? <b aria-label={`${attention} attention items`}>{attention}</b> : null}</button>;
  })}</nav>;
}
function NavigationDrawer({ route, attention, onClose }: { route: AppRoute; attention: number; onClose: () => void }) {
  const ref = useRef<HTMLElement>(null);
  useDialog(ref, onClose);
  return <div className="lamina-nav-backdrop" onMouseDown={e => { if (e.currentTarget === e.target) onClose(); }}><section ref={ref} className="lamina-nav-drawer" role="dialog" aria-modal="true" aria-label="Navigation"><header><strong>Explore PHarness</strong><button type="button" aria-label="Close navigation" onClick={onClose}><X size={20} /></button></header><Navigation route={route} attention={attention} onNavigate={onClose} /></section></div>;
}
export function LaminaShell({ theme, setTheme, route, attention, controllerState, operatorName, menuOpen, setMenuOpen, onSearch, children }: {
  theme: string; setTheme: (theme: string) => void; route: AppRoute; attention: number; controllerState: string; operatorName: string;
  menuOpen: boolean; setMenuOpen: (open: boolean) => void; onSearch: () => void; children: ReactNode;
}) {
  return <div className={`repo-app lamina-app theme-${theme}`}>
    <div className="lamina-atmosphere" aria-hidden="true"><div /><div /></div>
    <a href="#lamina-main" className="lamina-skip" onClick={e => { e.preventDefault(); document.getElementById("lamina-main")?.focus(); }}>Skip to content</a>
    <header className="lamina-topbar">
      <button type="button" className="lamina-brand" onClick={() => navigate("overview")} aria-label="PHarness Overview"><span><GitBranch size={22} weight="bold" /></span><strong>PHarness</strong></button>
      <div className="lamina-desktop-nav"><Navigation route={route} attention={attention} onNavigate={() => setMenuOpen(false)} /></div>
      <div className="lamina-topbar-tools"><span className={`lamina-connection is-${controllerState.toLowerCase()}`} title="Freshness of the last API projection"><i />{controllerState}</span><button type="button" aria-label="Search PHarness" title="Search · ⌘K / Ctrl+K" onClick={onSearch}><MagnifyingGlass size={19} /></button><button type="button" aria-label={theme === "dark" ? "Use light theme" : "Use dark theme"} onClick={() => setTheme(theme === "dark" ? "light" : "dark")}>{theme === "dark" ? <Sun size={19} /> : <Moon size={19} />}</button><span className="lamina-avatar" title={`Operator: ${operatorName}`}>{operatorName.slice(0, 2).toUpperCase()}</span><button type="button" className="lamina-menu" aria-label="Open navigation" aria-expanded={menuOpen} onClick={() => setMenuOpen(true)}><List size={22} /></button></div>
    </header>
    <main id="lamina-main" tabIndex={-1} className="repo-main"><div className="repo-content">{children}</div></main>
    {menuOpen ? <NavigationDrawer route={route} attention={attention} onClose={() => setMenuOpen(false)} /> : null}
  </div>;
}
