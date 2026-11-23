// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CampusApp, parseRoute, terminalHints } from "./CampusApp";
import { CampusApiError, CampusAssignment, CampusLabSession, CampusMe, CampusPortalApi } from "./campusApi";
import { limitRows } from "./campusFormat";
import type { TerminalWidget } from "./xtermWidget";

const now = Math.floor(Date.now() / 1000);

const student: CampusMe = {
  csrf_token: "csrf-1",
  identity: { display_name: "Student One", issuer: "https://idp", subject: "s1" },
  is_tenant_admin: false,
  memberships: [{ course_id: "cs101", role: "student" }],
};

const instructor: CampusMe = {
  csrf_token: "csrf-2",
  identity: { display_name: "Instructor One", issuer: "https://idp", subject: "i1" },
  is_tenant_admin: false,
  memberships: [{ course_id: "cs101", role: "instructor" }],
};

const limits = {
  cpu_millicores: 1500,
  memory_mib: 1536,
  ephemeral_storage_mib: 1024,
  ttl_seconds: 5400,
  network: "approved_egress" as const,
  egress_hosts: ["github.com", "pypi.org"],
  model_aliases: ["campus-default"],
  session_allocation_micro_usd: 2_000_000,
  max_output_tokens: 8192,
  set_by: { ttl_seconds: "profile" as const, cpu_millicores: "assignment" as const, egress: "profile" as const },
};

const assignment = (session: CampusLabSession | null = null): CampusAssignment => ({
  assignment_id: "coding-lab",
  title: "Coding lab",
  profile: { id: "campus-coding", version: "v1", kind: "coding" },
  session_seconds: 5400,
  limits,
  launchable: true,
  session,
});

const session = (overrides: Partial<CampusLabSession> = {}): CampusLabSession => ({
  session_id: "cls_1",
  course_id: "cs101",
  assignment_id: "coding-lab",
  status: "provisioning",
  stop_requested: false,
  current: true,
  created_at: now,
  expires_at: now + 5400,
  profile: { id: "campus-coding", version: "v1" },
  ...overrides,
});

function fakeApi(me: CampusMe, overrides: Partial<CampusPortalApi> = {}): CampusPortalApi {
  return {
    me: vi.fn(async () => me),
    session: vi.fn(async () => ({ session: session() })),
    ticket: vi.fn(async () => {
      throw new CampusApiError(409, "session_not_ready", "not ready");
    }),
    stop: vi.fn(async () => ({ session: session({ stop_requested: true }) })),
    logout: vi.fn(async () => ({})),
    assignments: vi.fn(async () => ({ course_id: "cs101", assignments: [assignment()] })),
    launch: vi.fn(async () => ({ admission: "created" as const, session: session() })),
    mySessions: vi.fn(async () => ({ sessions: [] })),
    courseSessions: vi.fn(async () => ({ course_id: "cs101", sessions: [] })),
    members: vi.fn(async () => ({ course_id: "cs101", members: [] })),
    ...overrides,
  };
}

const widget = (): TerminalWidget =>
  ({
    open: vi.fn(),
    onData: vi.fn(),
    onResize: vi.fn(),
    onKey: vi.fn(),
    fit: vi.fn(),
    size: () => ({ cols: 80, rows: 24 }),
    write: vi.fn(),
    reset: vi.fn(),
    focus: vi.fn(),
    setScreenReaderMode: vi.fn(),
    dispose: vi.fn(),
  }) as unknown as TerminalWidget;

beforeEach(() => {
  window.location.hash = "#/";
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("parseRoute", () => {
  it("names each Campus page and falls back to home", () => {
    expect(parseRoute("")).toEqual({ name: "home" });
    expect(parseRoute("#/courses/cs101")).toEqual({ name: "course", courseId: "cs101" });
    expect(parseRoute("#/courses/cs101/assignments/coding-lab")).toEqual({ name: "assignment", courseId: "cs101", assignmentId: "coding-lab" });
    expect(parseRoute("#/lab-sessions/cls_1")).toEqual({ name: "lab", sessionId: "cls_1" });
    expect(parseRoute("#/courses/cs101/live")).toEqual({ name: "live", courseId: "cs101" });
    expect(parseRoute("#/profiles")).toEqual({ name: "profiles" });
    expect(parseRoute("#/courses/<script>")).toEqual({ name: "home" });
  });
});

describe("terminalHints", () => {
  it("uses tools each lab image has and shows its network policy", () => {
    const research = { ...assignment(), profile: { id: "campus-research", version: "v1", kind: "research" }, limits: { ...limits, network: "denied" as const, egress_hosts: [] } };
    const researchHints = terminalHints(research).map(h => h.command).join("\n");
    expect(researchHints).toContain("python3");
    expect(researchHints).not.toContain("curl");
    const codingHints = terminalHints(assignment()).map(h => h.command);
    expect(codingHints).toContain("git ls-remote https://github.com/git/git HEAD");
    expect(codingHints.join("\n")).toContain("gitlab.com");
    expect(codingHints.join("\n")).not.toContain("curl");
    expect(terminalHints(null).map(h => h.command)).toEqual(["opencode", "pwd && ls -la"]);
  });
});

describe("limitRows", () => {
  it("says who set each limit in plain words", () => {
    const rows = limitRows(limits);
    expect(rows.find(r => r.label === "Computer")?.value).toBe("1.5 CPU cores, 1.5 GB memory, 1 GB of files");
    expect(rows.find(r => r.label === "Computer")?.source).toBe("set by instructor");
    expect(rows.find(r => r.label === "Internet")?.value).toContain("github.com");
    expect(rows.find(r => r.label === "Session budget")?.value).toBe("$2.00 of AI usage");
  });
});

describe("CampusApp", () => {
  it("offers college sign-in when there is no Campus session", async () => {
    const api = fakeApi(student, { me: vi.fn(async () => Promise.reject(new CampusApiError(401, null, "missing"))) });
    render(<CampusApp api={api} createWidget={widget} />);
    expect(await screen.findByRole("link", { name: "Continue with college sign-in" })).toHaveAttribute("href", "/api/campus/auth/login");
  });

  it("lists a student's labs with their limits and launches one", async () => {
    const api = fakeApi(student);
    render(<CampusApp api={api} createWidget={widget} />);
    expect(await screen.findByText("Coding lab", { selector: ".cp-assignment__title" })).toBeInTheDocument();
    await act(async () => {
      window.location.hash = "#/courses/cs101/assignments/coding-lab";
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    });
    expect(await screen.findByText("What this lab allows")).toBeInTheDocument();
    expect(screen.getByText(/1.5 CPU cores/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Launch lab" }));
    await waitFor(() => expect(api.launch).toHaveBeenCalledWith("cs101", "coding-lab", "csrf-1", expect.stringMatching(/^launch-/)));
    await waitFor(() => expect(window.location.hash).toBe("#/lab-sessions/cls_1"));
  });

  it("explains a launch refused because every slot is busy", async () => {
    const api = fakeApi(student, {
      launch: vi.fn(async () => Promise.reject(new CampusApiError(503, "no_slots_available", "busy"))),
    });
    window.location.hash = "#/courses/cs101/assignments/coding-lab";
    render(<CampusApp api={api} createWidget={widget} />);
    fireEvent.click(await screen.findByRole("button", { name: "Launch lab" }));
    expect(await screen.findByText("All lab slots are in use right now.")).toBeInTheDocument();
  });

  it("shows launch progress while the sandbox is provisioning", async () => {
    window.location.hash = "#/lab-sessions/cls_1";
    render(<CampusApp api={fakeApi(student)} createWidget={widget} />);
    expect(await screen.findByRole("list", { name: "Launch progress" })).toBeInTheDocument();
    expect(screen.getByText("Nothing is needed from you yet.")).toBeInTheDocument();
  });

  it("reports an ended lab and its deleted workspace", async () => {
    window.location.hash = "#/lab-sessions/cls_1";
    const api = fakeApi(student, { session: vi.fn(async () => ({ session: session({ status: "stopped", terminated_at: now }) })) });
    render(<CampusApp api={api} createWidget={widget} />);
    expect(await screen.findByText("Your lab has ended")).toBeInTheDocument();
    expect(screen.getByText(/Deleted. Its AI access was revoked./)).toBeInTheDocument();
  });

  it("lets an instructor end a student's lab after confirming", async () => {
    window.location.hash = "#/courses/cs101/live";
    const live = session({ status: "ready", owner: { issuer: "https://idp", subject: "s1" } });
    const api = fakeApi(instructor, {
      courseSessions: vi.fn(async () => ({ course_id: "cs101", sessions: [live] })),
      members: vi.fn(async () => ({
        course_id: "cs101",
        members: [{ issuer: "https://idp", subject: "s1", role: "student" as const, display_name: "Student One" }],
      })),
    });
    render(<CampusApp api={api} createWidget={widget} />);
    fireEvent.click(await screen.findByRole("button", { name: "End Student One's lab" }));
    fireEvent.click(screen.getByRole("button", { name: "End lab" }));
    await waitFor(() => expect(api.stop).toHaveBeenCalledWith("cls_1", "csrf-2"));
  });
});
