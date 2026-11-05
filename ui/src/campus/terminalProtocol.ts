// The Campus terminal WebSocket protocol, pharness.campus.terminal.v1:
// the browser authenticates with a single-use ticket as its first message,
// then binary frames carry terminal bytes both ways, the browser sends
// resize control messages as text and the server reports `ready` and
// `closed` status messages as text.

export const TERMINAL_PROTOCOL = "pharness.campus.terminal.v1";

export type CloseInfo = {
  reason: string | null;
  code: number | null;
  message: string;
  /** Whether a new connection could succeed without anything else changing. */
  canReconnect: boolean;
  /** True when the connection dropped without the server saying why, such as during a restart. */
  dropped: boolean;
};

type ReasonText = { message: string; reconnect: boolean };

const REASONS: Record<string, ReasonText> = {
  exited: { message: "The terminal session ended.", reconnect: true },
  client_closed: { message: "You disconnected the terminal.", reconnect: true },
  ticket_rejected: { message: "The terminal could not be authorized. Connect again to retry.", reconnect: true },
  auth_timeout: { message: "The terminal was not authorized in time. Connect again to retry.", reconnect: true },
  invalid_message: { message: "The terminal received a message it does not accept.", reconnect: true },
  too_many_terminals: { message: "This lab already has the most terminals it allows. Close another terminal first.", reconnect: true },
  too_many_tickets: { message: "Too many terminal connections were started at once. Wait a moment and connect again.", reconnect: true },
  terminal_unavailable: { message: "The terminal service is unavailable right now. Try again shortly.", reconnect: true },
  workload_not_running: { message: "The lab workspace is not running yet. Try again shortly.", reconnect: true },
  session_not_ready: { message: "The lab is not ready yet. Try again when it is ready.", reconnect: true },
  session_stopped: { message: "The lab was stopped. Launch a new lab to open a terminal.", reconnect: false },
  session_expired: { message: "The lab reached its time limit and was closed.", reconnect: false },
  session_superseded: { message: "This lab was reset or replaced. Open the terminal of the current lab.", reconnect: false },
  generation_changed: { message: "This lab was reset or replaced. Open the terminal of the current lab.", reconnect: false },
  policy_changed: { message: "The lab's policy changed. Launch a new lab to continue.", reconnect: false },
  session_not_found: { message: "This lab session was not found.", reconnect: false },
  logged_out: { message: "You were signed out. Sign in again to continue.", reconnect: false },
  access_revoked: { message: "You no longer have access to this lab.", reconnect: false },
  idle_timeout: { message: "The terminal closed after a period without input.", reconnect: true },
  client_too_slow: { message: "The connection could not keep up with the terminal output.", reconnect: true },
  internal_error: { message: "The terminal closed because of a server error.", reconnect: true },
};

export function describeClose(reason: string | null | undefined, code: number | null = null): CloseInfo {
  if (reason && REASONS[reason]) {
    const known = REASONS[reason];
    return { reason, code, message: known.message, canReconnect: known.reconnect, dropped: false };
  }
  return {
    reason: reason || null,
    code,
    message: "The connection to the terminal was lost.",
    canReconnect: true,
    dropped: !reason,
  };
}

export type ReadyMessage = { type: "ready"; connection_id: string; cols: number; rows: number };

export type TerminalHandlers = {
  onReady: (ready: ReadyMessage) => void;
  onOutput: (bytes: Uint8Array) => void;
  onClose: (info: CloseInfo) => void;
};

export type TerminalConnection = {
  sendInput: (data: string) => void;
  resize: (cols: number, rows: number) => void;
  close: () => void;
};

type SocketLocation = { protocol: string; host: string };

export function socketUrl(path: string, location: SocketLocation): string {
  const scheme = location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${location.host}${path}`;
}

export type OpenTerminalOptions = {
  socketPath: string;
  ticket: string;
  cols: number;
  rows: number;
  handlers: TerminalHandlers;
  location?: SocketLocation;
  WebSocketImpl?: typeof WebSocket;
};

/** Open the socket and authenticate with the ticket. The ticket never appears in the URL. */
export function openTerminal(options: OpenTerminalOptions): TerminalConnection {
  const Impl = options.WebSocketImpl ?? WebSocket;
  const socket = new Impl(socketUrl(options.socketPath, options.location ?? window.location), TERMINAL_PROTOCOL);
  socket.binaryType = "arraybuffer";
  const encoder = new TextEncoder();
  let statusReason: string | null = null;
  let finished = false;
  const open = () => socket.readyState === 1;

  socket.onopen = () => {
    socket.send(JSON.stringify({ type: "auth", ticket: options.ticket, cols: options.cols, rows: options.rows }));
  };
  socket.onmessage = (event: MessageEvent) => {
    if (typeof event.data !== "string") {
      options.handlers.onOutput(new Uint8Array(event.data as ArrayBuffer));
      return;
    }
    let message: { type?: string; reason?: string } = {};
    try {
      message = JSON.parse(event.data);
    } catch {
      return;
    }
    if (message.type === "ready") options.handlers.onReady(message as ReadyMessage);
    if (message.type === "closed" && typeof message.reason === "string") statusReason = message.reason;
  };
  socket.onclose = (event: CloseEvent) => {
    if (finished) return;
    finished = true;
    options.handlers.onClose(describeClose(statusReason ?? (event.reason || null), event.code || null));
  };

  return {
    sendInput(data) {
      if (open()) socket.send(encoder.encode(data));
    },
    resize(cols, rows) {
      if (open()) socket.send(JSON.stringify({ type: "resize", cols, rows }));
    },
    close() {
      if (finished) return;
      statusReason = statusReason ?? "client_closed";
      socket.close(1000, "client_closed");
    },
  };
}
