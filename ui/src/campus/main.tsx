import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import React from "react";
import { createRoot } from "react-dom/client";
import { CampusTerminalApp } from "./CampusTerminalApp";
import { createXtermWidget } from "./xtermWidget";
import "./campus.css";

const LAB_SESSION_PATTERN = /^[A-Za-z0-9._:-]{1,128}$/;

/** `#/lab-sessions/<id>` or `?session=<id>`. */
export function sessionIdFromLocation(location: Location): string | null {
  const fromHash = location.hash.match(/^#\/lab-sessions\/([^/?#]+)/)?.[1];
  const candidate = fromHash ? decodeURIComponent(fromHash) : new URLSearchParams(location.search).get("session");
  return candidate && LAB_SESSION_PATTERN.test(candidate) ? candidate : null;
}

const root = document.getElementById("root");
if (!root) throw new Error("Campus terminal root element is missing.");

const reactRoot = createRoot(root);

function render() {
  const sessionId = sessionIdFromLocation(window.location);
  // A new session id is a new page: the key remounts the terminal.
  reactRoot.render(
    <React.StrictMode>
      <CampusTerminalApp key={sessionId ?? ""} sessionId={sessionId} createWidget={createXtermWidget} />
    </React.StrictMode>,
  );
}

window.addEventListener("hashchange", render);
render();
