import React, { useEffect, useRef, useState } from "react";
import { CampusApi, CampusApiError, CampusLabSession, SIGN_IN_PATH, campusApi } from "./campusApi";
import { CloseInfo, OpenTerminalOptions, TerminalConnection, describeClose, openTerminal } from "./terminalProtocol";
import type { TerminalWidget } from "./xtermWidget";

export const MAX_AUTOMATIC_RECONNECTS = 3;

type Phase = "loading" | "signed_out" | "unavailable" | "not_ready" | "idle" | "connecting" | "connected" | "closed";
type FocusTarget = "terminal" | "connect" | "status" | null;

export type CampusTerminalAppProps = {
  sessionId: string | null;
  createWidget: () => TerminalWidget;
  api?: CampusApi;
  connect?: (options: OpenTerminalOptions) => TerminalConnection;
  reconnectDelayMs?: (attempt: number) => number;
};

const STATUS_LABELS: Record<string, string> = {
  provisioning: "still starting",
  stopping: "stopping",
  stopped: "stopped",
  failed: "failed",
  cleanup_blocked: "waiting for cleanup",
};

function errorInfo(error: unknown): CloseInfo {
  if (error instanceof CampusApiError) {
    if (error.status === 401) return describeClose("logged_out");
    if (error.code) return describeClose(error.code);
    if (error.status === 404) return describeClose("session_not_found");
  }
  return describeClose("terminal_unavailable");
}

/** The Ctrl+] chord that moves focus out of the terminal. */
export function isEscapeChord(event: KeyboardEvent): boolean {
  return (
    event.type === "keydown" &&
    event.ctrlKey &&
    !event.altKey &&
    !event.metaKey &&
    (event.key === "]" || event.code === "BracketRight")
  );
}

export function CampusTerminalApp({
  sessionId,
  createWidget,
  api = campusApi,
  connect = openTerminal,
  reconnectDelayMs = attempt => 1000 * 2 ** (attempt - 1),
}: CampusTerminalAppProps) {
  const [phase, setPhase] = useState<Phase>("loading");
  const [status, setStatus] = useState("Loading your lab.");
  const [session, setSession] = useState<CampusLabSession | null>(null);
  const [closeInfo, setCloseInfo] = useState<CloseInfo | null>(null);
  const [screenReader, setScreenReader] = useState(false);
  const csrf = useRef<string | null>(null);
  const widget = useRef<TerminalWidget | null>(null);
  const connection = useRef<TerminalConnection | null>(null);
  const container = useRef<HTMLDivElement>(null);
  const escapeTarget = useRef<HTMLButtonElement>(null);
  const connectButton = useRef<HTMLButtonElement>(null);
  const statusRef = useRef<HTMLParagraphElement>(null);
  const attempts = useRef(0);
  const reconnectTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mounted = useRef(true);
  const pendingFocus = useRef<FocusTarget>(null);

  useEffect(() => {
    mounted.current = true;
    const terminal = createWidget();
    widget.current = terminal;
    if (container.current) terminal.open(container.current);
    terminal.onData(data => connection.current?.sendInput(data));
    terminal.onResize(({ cols, rows }) => connection.current?.resize(cols, rows));
    terminal.onKey(event => {
      if (!isEscapeChord(event)) return true;
      event.preventDefault();
      escapeTarget.current?.focus();
      return false;
    });
    const refit = () => terminal.fit();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(refit);
    if (observer && container.current) observer.observe(container.current);
    window.addEventListener("resize", refit);
    return () => {
      mounted.current = false;
      observer?.disconnect();
      window.removeEventListener("resize", refit);
      if (reconnectTimer.current) clearTimeout(reconnectTimer.current);
      connection.current?.close();
      connection.current = null;
      terminal.dispose();
      widget.current = null;
    };
  }, [createWidget]);

  useEffect(() => {
    widget.current?.setScreenReaderMode(screenReader);
  }, [screenReader]);

  useEffect(() => {
    const target = pendingFocus.current;
    pendingFocus.current = null;
    if (target === "terminal") widget.current?.focus();
    if (target === "connect") connectButton.current?.focus();
    if (target === "status") statusRef.current?.focus();
  });

  async function load() {
    if (!sessionId) {
      setPhase("unavailable");
      setStatus("No lab session is named in the address.");
      return;
    }
    try {
      csrf.current = (await api.me()).csrf_token;
    } catch (error) {
      const signedOut = error instanceof CampusApiError && error.status === 401;
      setPhase(signedOut ? "signed_out" : "unavailable");
      setStatus(signedOut ? "Sign in to open your lab terminal." : "Campus is unavailable right now.");
      return;
    }
    try {
      const current = (await api.session(sessionId)).session;
      setSession(current);
      if (current.status === "ready" && !current.stop_requested) {
        setPhase("idle");
        setStatus("Your lab is ready. Connect to open its terminal.");
      } else {
        setPhase("not_ready");
        const label = current.stop_requested ? "stopping" : STATUS_LABELS[current.status] ?? current.status;
        setStatus(`Your lab is ${label}. A terminal opens only while the lab is ready.`);
      }
    } catch (error) {
      const info = errorInfo(error);
      setPhase(info.reason === "logged_out" ? "signed_out" : "unavailable");
      setStatus(info.message);
    }
  }

  useEffect(() => {
    void load();
    // Loads once per session id; later loads are explicit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  function finish(info: CloseInfo) {
    if (!mounted.current) return;
    if (info.dropped && attempts.current < MAX_AUTOMATIC_RECONNECTS) {
      attempts.current += 1;
      const attempt = attempts.current;
      setPhase("connecting");
      setStatus(`Connection lost. Reconnecting, attempt ${attempt} of ${MAX_AUTOMATIC_RECONNECTS}.`);
      reconnectTimer.current = setTimeout(() => void connectNow(true), reconnectDelayMs(attempt));
      return;
    }
    attempts.current = 0;
    setCloseInfo(info);
    setPhase(info.reason === "logged_out" ? "signed_out" : "closed");
    setStatus(info.message);
    pendingFocus.current = info.canReconnect && info.reason !== "logged_out" ? "connect" : "status";
  }

  async function connectNow(automatic: boolean) {
    const terminal = widget.current;
    if (!sessionId || !csrf.current || !terminal || connection.current) return;
    reconnectTimer.current = null;
    if (!automatic) attempts.current = 0;
    setPhase("connecting");
    if (!automatic) setStatus("Connecting to the terminal.");
    let ticket;
    try {
      ticket = await api.ticket(sessionId, csrf.current);
    } catch (error) {
      finish(errorInfo(error));
      return;
    }
    if (!mounted.current) return;
    terminal.fit();
    const { cols, rows } = terminal.size();
    connection.current = connect({
      socketPath: ticket.socket_path,
      ticket: ticket.ticket,
      cols,
      rows,
      handlers: {
        onReady: () => {
          if (!mounted.current) return;
          attempts.current = 0;
          terminal.reset();
          setCloseInfo(null);
          setPhase("connected");
          setStatus("Terminal connected. Press Control and right bracket to move focus out of the terminal.");
          pendingFocus.current = "terminal";
        },
        onOutput: bytes => terminal.write(bytes),
        onClose: info => {
          connection.current = null;
          finish(info);
        },
      },
    });
  }

  function disconnect() {
    connection.current?.close();
  }

  async function stopLab() {
    if (!sessionId || !csrf.current) return;
    const wasConnected = connection.current !== null;
    try {
      const stopped = (await api.stop(sessionId, csrf.current)).session;
      setSession(stopped);
      // The server may close the socket before this response arrives; its close reason is the final word.
      if (connection.current) {
        setStatus("Stopping your lab. Its terminal will close.");
      } else if (!wasConnected) {
        setPhase("not_ready");
        setStatus("Stopping your lab.");
        pendingFocus.current = "status";
      }
    } catch (error) {
      setStatus(errorInfo(error).message);
    }
  }

  async function signOut() {
    if (!csrf.current) return;
    const wasConnected = connection.current !== null;
    try {
      await api.logout(csrf.current);
    } catch {
      // Signing out of an already ended session still ends here.
    }
    csrf.current = null;
    if (connection.current) {
      setStatus("Signing out. The terminal will close.");
    } else if (!wasConnected) {
      setPhase("signed_out");
      setStatus("You signed out.");
      pendingFocus.current = "status";
    }
  }

  const hasLab = phase === "idle" || phase === "connecting" || phase === "connected" || phase === "closed";
  const canConnect = phase === "idle" || (phase === "closed" && (closeInfo?.canReconnect ?? true));
  const showConnect = phase === "idle" || phase === "connecting" || canConnect;

  return (
    <main className="campus-terminal" aria-labelledby="campus-terminal-title">
      <header className="campus-terminal__header">
        <h1 id="campus-terminal-title">Lab terminal</h1>
        {session && (
          <p className="campus-terminal__lab">
            {session.course_id} / {session.assignment_id}
          </p>
        )}
      </header>
      <p ref={statusRef} className="campus-terminal__status" role="status" aria-live="polite" tabIndex={-1}>
        {status}
      </p>
      {phase === "signed_out" && (
        <a className="campus-button campus-button--primary" href={SIGN_IN_PATH}>
          Sign in
        </a>
      )}
      {phase === "not_ready" && (
        <button type="button" className="campus-button" onClick={() => void load()}>
          Check again
        </button>
      )}
      {hasLab && (
        <div className="campus-terminal__controls" role="group" aria-label="Terminal controls">
          {showConnect && (
            <button
              ref={connectButton}
              type="button"
              className="campus-button campus-button--primary"
              disabled={!canConnect}
              onClick={() => void connectNow(false)}
            >
              {phase === "closed" ? "Reconnect" : "Connect"}
            </button>
          )}
          <button
            ref={escapeTarget}
            type="button"
            className="campus-button"
            disabled={phase !== "connected"}
            onClick={disconnect}
          >
            Disconnect
          </button>
          <button
            type="button"
            className="campus-button"
            disabled={!session || session.stop_requested}
            onClick={() => void stopLab()}
          >
            Stop lab
          </button>
          <button type="button" className="campus-button" onClick={() => void signOut()}>
            Sign out
          </button>
          <label className="campus-terminal__toggle">
            <input type="checkbox" checked={screenReader} onChange={event => setScreenReader(event.target.checked)} />
            Screen reader mode
          </label>
        </div>
      )}
      <p id="campus-terminal-help" className="campus-terminal__help" hidden={!hasLab}>
        Everything you type in the terminal goes to your lab, including Tab and Escape. Press{" "}
        <kbd>Ctrl</kbd>+<kbd>]</kbd> to move focus out of the terminal to the terminal controls.
      </p>
      <section
        className="campus-terminal__screen"
        aria-label="Terminal"
        aria-describedby="campus-terminal-help"
        hidden={!hasLab}
      >
        <div ref={container} className="campus-terminal__xterm" />
      </section>
    </main>
  );
}
