import { describe, expect, it } from "vitest";
import { TERMINAL_PROTOCOL, describeClose, openTerminal, socketUrl } from "./terminalProtocol";

class FakeSocket {
  static last: FakeSocket | null = null;
  readyState = 0;
  binaryType = "blob";
  sent: unknown[] = [];
  closedWith: [number, string] | null = null;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onclose: ((event: { code: number; reason: string }) => void) | null = null;

  constructor(
    public url: string,
    public protocol: string,
  ) {
    FakeSocket.last = this;
  }

  send(data: unknown) {
    this.sent.push(data);
  }

  close(code: number, reason: string) {
    this.closedWith = [code, reason];
    this.readyState = 3;
    this.onclose?.({ code, reason });
  }

  serverOpen() {
    this.readyState = 1;
    this.onopen?.();
  }

  serverSend(data: unknown) {
    this.onmessage?.({ data });
  }

  serverClose(code: number, reason = "") {
    this.readyState = 3;
    this.onclose?.({ code, reason });
  }
}

function connect(events: string[]) {
  const connection = openTerminal({
    socketPath: "/api/campus/lab-sessions/cls_1/terminal",
    ticket: "t".repeat(43),
    cols: 100,
    rows: 30,
    location: { protocol: "https:", host: "campus.example.edu" },
    WebSocketImpl: FakeSocket as unknown as typeof WebSocket,
    handlers: {
      onReady: ready => events.push(`ready:${ready.connection_id}`),
      onOutput: bytes => events.push(`output:${new TextDecoder().decode(bytes)}`),
      onClose: info => events.push(`close:${info.reason}:${info.code}:${info.canReconnect}:${info.dropped}`),
    },
  });
  return { connection, socket: FakeSocket.last! };
}

describe("Campus terminal protocol", () => {
  it("authenticates in the first message, never in the URL, and carries bytes both ways", () => {
    const events: string[] = [];
    const { connection, socket } = connect(events);
    expect(socket.url).toBe("wss://campus.example.edu/api/campus/lab-sessions/cls_1/terminal");
    expect(socket.url).not.toContain("ticket");
    expect(socket.protocol).toBe(TERMINAL_PROTOCOL);
    expect(socket.binaryType).toBe("arraybuffer");
    connection.sendInput("before open");
    expect(socket.sent).toEqual([]);
    socket.serverOpen();
    expect(JSON.parse(socket.sent[0] as string)).toEqual({ type: "auth", ticket: "t".repeat(43), cols: 100, rows: 30 });
    socket.serverSend(JSON.stringify({ type: "ready", connection_id: "ctc_1", cols: 100, rows: 30 }));
    socket.serverSend(new TextEncoder().encode("$ ").buffer);
    connection.sendInput("ls\r");
    connection.resize(120, 40);
    expect(new TextDecoder().decode(socket.sent[1] as Uint8Array)).toBe("ls\r");
    expect(JSON.parse(socket.sent[2] as string)).toEqual({ type: "resize", cols: 120, rows: 40 });
    socket.serverSend("not json");
    socket.serverSend(JSON.stringify({ type: "closed", reason: "session_stopped" }));
    socket.serverClose(4010, "session_stopped");
    expect(events).toEqual(["ready:ctc_1", "output:$ ", "close:session_stopped:4010:false:false"]);
  });

  it("reports a drop without a reason as reconnectable and a client close as deliberate", () => {
    const dropped: string[] = [];
    connect(dropped).socket.serverClose(1006);
    expect(dropped).toEqual(["close:null:1006:true:true"]);
    const closed: string[] = [];
    const { connection, socket } = connect(closed);
    socket.serverOpen();
    connection.close();
    connection.close();
    expect(socket.closedWith).toEqual([1000, "client_closed"]);
    expect(closed).toEqual(["close:client_closed:1000:true:false"]);
  });

  it("names every server reason and never offers to revive a stopped, expired or signed-out lab", () => {
    for (const reason of ["session_stopped", "session_expired", "session_superseded", "generation_changed", "logged_out", "access_revoked", "policy_changed"]) {
      const info = describeClose(reason, 4010);
      expect(info.canReconnect, reason).toBe(false);
      expect(info.message).not.toMatch(/lost/);
    }
    for (const reason of ["exited", "idle_timeout", "terminal_unavailable", "ticket_rejected", "too_many_terminals"]) {
      expect(describeClose(reason).canReconnect, reason).toBe(true);
    }
    expect(describeClose("something_new")).toMatchObject({ canReconnect: true, dropped: false });
    expect(socketUrl("/x", { protocol: "http:", host: "localhost:4173" })).toBe("ws://localhost:4173/x");
  });
});
