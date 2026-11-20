import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import React from "react";
import { createRoot } from "react-dom/client";
import { CampusApp } from "./CampusApp";
import { CampusTerminalApp } from "./CampusTerminalApp";
import { createXtermWidget } from "./xtermWidget";
import "./campus.css";
import "./portal.css";

const LAB_SESSION_PATTERN = /^[A-Za-z0-9._:-]{1,128}$/;

/** `?session=<id>` opens the standalone terminal page the M1 demo harness drives. */
export function legacySessionId(location: Location): string | null {
  const candidate = new URLSearchParams(location.search).get("session");
  return candidate && LAB_SESSION_PATTERN.test(candidate) ? candidate : null;
}

function institutionName(): string {
  const meta = document.querySelector('meta[name="pharness-campus-institution"]');
  const value = meta?.getAttribute("content")?.trim();
  return value || "Campus pilot";
}

const root = document.getElementById("root");
if (!root) throw new Error("Campus root element is missing.");

const reactRoot = createRoot(root);
const legacy = legacySessionId(window.location);

if (legacy) {
  document.body.classList.add("campus-legacy");
  reactRoot.render(
    <React.StrictMode>
      <CampusTerminalApp key={legacy} sessionId={legacy} createWidget={createXtermWidget} />
    </React.StrictMode>,
  );
} else {
  document.body.classList.add("campus-portal");
  document.title = "PHarness Campus";
  reactRoot.render(
    <React.StrictMode>
      <CampusApp createWidget={createXtermWidget} institution={institutionName()} />
    </React.StrictMode>,
  );
}
