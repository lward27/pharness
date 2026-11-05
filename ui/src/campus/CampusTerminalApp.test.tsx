import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CampusTerminalApp, isEscapeChord } from "./CampusTerminalApp";
import { CampusApi, CampusApiError } from "./campusApi";
import type { OpenTerminalOptions, TerminalConnection } from "./terminalProtocol";
import { describeClose } from "./terminalProtocol";
import type { TerminalWidget } from "./xtermWidget";

type FakeWidget = TerminalWidget & {
  written: string[];
  focused: number;
  screenReader: boolean;
  data: ((data: string) => void) | null;
  key: ((event: KeyboardEvent) => boolean) | null;
};

function fakeWidget(): FakeWidget {
  const widget: FakeWidget = {
    written: [],
    focused: 0,
    screenReader: false,
    data: null,
    key: null,
    open: () => {},
    write: bytes => widget.written.push(typeof bytes === "string" ? bytes : new TextDecoder().decode(bytes)),
    onData: listener => {
      widget.data = listener;
    },
    onResize: () => {},
    onKey: handler => {
      widget.key = handler;
    },
    fit: () => {},
    focus: () => {
      widget.focused += 1;
    },
    reset: () => {},
    setScreenReaderMode: enabled => {
      widget.screenReader = enabled;
    },
    size: () => ({ cols: 100, rows: 30 }),
    dispose: () => {},
  };
  return widget;
}

const readySession = { session_id: "cls_1", course_id: "cs101", assignment_id: "lab-1", status: "ready", stop_requested: false, current: true, expires_at: 0 };

function fakeApi(overrides: Partial<CampusApi> = {}): CampusApi {
  let tickets = 0;
  return {
    me: vi.fn(async () => ({ csrf_token: "csrf-1", identity: {} })),
    session: vi.fn(async () => ({ session: readySession })),
    ticket: vi.fn(async () => {
      tickets += 1;
      return { ticket: `ticket-${tickets}`, expires_at: 0, socket_path: "/api/campus/lab-sessions/cls_1/terminal", protocol: "pharness.campus.terminal.v1" };
    }),
    stop: vi.fn(async () => ({ session: { ...readySession, stop_requested: true } })),
    logout: vi.fn(async () => ({})),
    ...overrides,
  };
}

type Opened = { options: OpenTerminalOptions; sent: string[]; closed: boolean };

function fakeConnect() {
  const opened: Opened[] = [];
  const connect = (options: OpenTerminalOptions): TerminalConnection => {
    const entry: Opened = { options, sent: [], closed: false };
    opened.push(entry);
    return {
      sendInput: data => entry.sent.push(data),
      resize: () => {},
      close: () => {
        entry.closed = true;
        options.handlers.onClose(describeClose("client_closed", 1000));
      },
    };
  };
  return { opened, connect };
}

function setup(api = fakeApi()) {
  const widget = fakeWidget();
  const { opened, connect } = fakeConnect();
  render(<CampusTerminalApp sessionId="cls_1" api={api} createWidget={() => widget} connect={connect} reconnectDelayMs={() => 0} />);
  return { api, widget, opened };
}

const status = () => screen.getByRole("status");

afterEach(cleanup);

describe("Campus lab terminal page", () => {
  it("connects with a fresh ticket, announces each state and keeps keyboard focus escapable", async () => {
    const { api, widget, opened } = setup();
    await waitFor(() => expect(status()).toHaveTextContent("Your lab is ready"));
    expect(status()).toHaveAttribute("aria-live", "polite");
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => expect(opened).toHaveLength(1));
    expect(api.ticket).toHaveBeenCalledWith("cls_1", "csrf-1");
    expect(opened[0].options).toMatchObject({ ticket: "ticket-1", cols: 100, rows: 30 });
    act(() => opened[0].options.handlers.onReady({ type: "ready", connection_id: "ctc_1", cols: 100, rows: 30 }));
    expect(status()).toHaveTextContent("Terminal connected");
    expect(widget.focused).toBe(1);

    act(() => opened[0].options.handlers.onOutput(new TextEncoder().encode("codex-cli 1.0\r\n")));
    expect(widget.written).toEqual(["codex-cli 1.0\r\n"]);
    widget.data!("codex --version\r");
    expect(opened[0].sent).toEqual(["codex --version\r"]);

    const tab = new KeyboardEvent("keydown", { key: "Tab" });
    expect(widget.key!(tab)).toBe(true);
    const chord = new KeyboardEvent("keydown", { key: "]", code: "BracketRight", ctrlKey: true, cancelable: true });
    act(() => {
      expect(widget.key!(chord)).toBe(false);
    });
    expect(chord.defaultPrevented).toBe(true);
    expect(screen.getByRole("button", { name: "Disconnect" })).toHaveFocus();
    expect(screen.getByRole("region", { name: "Terminal" })).toHaveAccessibleDescription(/Ctrl\+\] to move focus out of the terminal/);

    fireEvent.click(screen.getByRole("checkbox", { name: "Screen reader mode" }));
    expect(widget.screenReader).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Stop lab" }));
    await waitFor(() => expect(api.stop).toHaveBeenCalledWith("cls_1", "csrf-1"));
    act(() => opened[0].options.handlers.onClose(describeClose("session_stopped", 4010)));
    expect(status()).toHaveTextContent("The lab was stopped");
    expect(screen.queryByRole("button", { name: "Reconnect" })).not.toBeInTheDocument();
    expect(status()).toHaveFocus();
  });

  it("reconnects a dropped terminal with new tickets and gives up on a stopped lab", async () => {
    let calls = 0;
    const api = fakeApi({
      ticket: vi.fn(async () => {
        calls += 1;
        if (calls === 3) throw new CampusApiError(409, "session_stopped", "stopped");
        return { ticket: `ticket-${calls}`, expires_at: 0, socket_path: "/s", protocol: "p" };
      }),
    });
    const { opened } = setup(api);
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
    await waitFor(() => expect(opened).toHaveLength(1));
    act(() => opened[0].options.handlers.onClose(describeClose(null, 1006)));
    await waitFor(() => expect(opened).toHaveLength(2));
    expect(opened[1].options.ticket).toBe("ticket-2");
    act(() => opened[1].options.handlers.onClose(describeClose(null, 1006)));
    await waitFor(() => expect(status()).toHaveTextContent("The lab was stopped"));
    expect(opened).toHaveLength(2);
    expect(screen.queryByRole("button", { name: "Reconnect" })).not.toBeInTheDocument();
  });

  it("offers a focused Reconnect after an idle close and stops retrying after three drops", async () => {
    const { opened } = setup();
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
    await waitFor(() => expect(opened).toHaveLength(1));
    act(() => opened[0].options.handlers.onClose(describeClose("idle_timeout", 4013)));
    expect(status()).toHaveTextContent("period without input");
    const reconnect = screen.getByRole("button", { name: "Reconnect" });
    expect(reconnect).toHaveFocus();
    fireEvent.click(reconnect);
    for (let drop = 1; drop <= 4; drop += 1) {
      await waitFor(() => expect(opened).toHaveLength(drop + 1));
      act(() => opened[drop].options.handlers.onClose(describeClose(null, 1006)));
    }
    await waitFor(() => expect(status()).toHaveTextContent("connection to the terminal was lost"));
    expect(opened).toHaveLength(5);
    expect(screen.getByRole("button", { name: "Reconnect" })).toHaveFocus();
  });

  it("asks a signed-out student to sign in and explains a lab that is not ready", async () => {
    setup(fakeApi({ me: vi.fn(async () => Promise.reject(new CampusApiError(401, null, "no session"))) }));
    expect(await screen.findByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/api/campus/auth/login");
    expect(screen.queryByRole("button", { name: "Connect" })).not.toBeInTheDocument();
  });

  it("does not offer a terminal for a lab that is still starting", async () => {
    setup(fakeApi({ session: vi.fn(async () => ({ session: { ...readySession, status: "provisioning" } })) }));
    await waitFor(() => expect(status()).toHaveTextContent("Your lab is still starting"));
    expect(screen.getByRole("button", { name: "Check again" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Connect" })).not.toBeInTheDocument();
  });

  it("recognises only Ctrl+] as the escape chord", () => {
    expect(isEscapeChord(new KeyboardEvent("keydown", { key: "]", ctrlKey: true }))).toBe(true);
    expect(isEscapeChord(new KeyboardEvent("keyup", { key: "]", ctrlKey: true }))).toBe(false);
    expect(isEscapeChord(new KeyboardEvent("keydown", { key: "]" }))).toBe(false);
    expect(isEscapeChord(new KeyboardEvent("keydown", { key: "Escape" }))).toBe(false);
  });
});
