import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import {
  CampusApiError,
  CampusAssignment,
  CampusLabSession,
  CampusMe,
  CampusMember,
  CampusPortalApi,
  LIVE_STATUSES,
  SIGN_IN_PATH,
  campusPortalApi,
} from "./campusApi";
import { CampusTerminalApp } from "./CampusTerminalApp";
import {
  courseLabel,
  formatClock,
  formatDuration,
  formatTime,
  limitRows,
  newIdempotencyKey,
  sessionState,
} from "./campusFormat";
import type { TerminalWidget } from "./xtermWidget";

// ---------------------------------------------------------------- routing

export type CampusRoute =
  | { name: "home" }
  | { name: "course"; courseId: string }
  | { name: "assignment"; courseId: string; assignmentId: string }
  | { name: "lab"; sessionId: string }
  | { name: "live"; courseId: string }
  | { name: "profiles" };

const ID = "([A-Za-z0-9._:-]{1,128})";

export function parseRoute(hash: string): CampusRoute {
  const path = hash.replace(/^#/, "") || "/";
  let match: RegExpMatchArray | null;
  if ((match = path.match(new RegExp(`^/lab-sessions/${ID}/?$`)))) return { name: "lab", sessionId: match[1] };
  if ((match = path.match(new RegExp(`^/courses/${ID}/assignments/${ID}/?$`))))
    return { name: "assignment", courseId: match[1], assignmentId: match[2] };
  if ((match = path.match(new RegExp(`^/courses/${ID}/live/?$`)))) return { name: "live", courseId: match[1] };
  if ((match = path.match(new RegExp(`^/courses/${ID}/?$`)))) return { name: "course", courseId: match[1] };
  if (/^\/profiles\/?$/.test(path)) return { name: "profiles" };
  return { name: "home" };
}

export const href = {
  home: () => "#/",
  course: (courseId: string) => `#/courses/${encodeURIComponent(courseId)}`,
  assignment: (courseId: string, assignmentId: string) =>
    `#/courses/${encodeURIComponent(courseId)}/assignments/${encodeURIComponent(assignmentId)}`,
  lab: (sessionId: string) => `#/lab-sessions/${encodeURIComponent(sessionId)}`,
  live: (courseId: string) => `#/courses/${encodeURIComponent(courseId)}/live`,
  profiles: () => "#/profiles",
};

// ---------------------------------------------------------------- context

type CampusContextValue = {
  me: CampusMe;
  api: CampusPortalApi;
  institution: string;
  createWidget: () => TerminalWidget;
  navigate: (hash: string) => void;
  staffCourses: string[];
  studentCourses: string[];
};

const CampusContext = createContext<CampusContextValue | null>(null);

function useCampus(): CampusContextValue {
  const value = useContext(CampusContext);
  if (!value) throw new Error("Campus context is missing.");
  return value;
}

function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now() / 1000), intervalMs);
    return () => clearInterval(timer);
  }, [intervalMs]);
  return now;
}

/** Load once, then refresh every `pollMs` while `keepPolling` says so. */
function usePolled<T>(load: () => Promise<T>, deps: unknown[], pollMs: number, keepPolling: (value: T) => boolean = () => true) {
  const [value, setValue] = useState<T | null>(null);
  const [error, setError] = useState<unknown>(null);
  const loadRef = useRef(load);
  loadRef.current = load;
  const keepRef = useRef(keepPolling);
  keepRef.current = keepPolling;
  const [tick, setTick] = useState(0);
  const refresh = useCallback(() => setTick(t => t + 1), []);

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const run = async () => {
      try {
        const next = await loadRef.current();
        if (!alive) return;
        setValue(next);
        setError(null);
        if (keepRef.current(next)) timer = setTimeout(run, pollMs);
      } catch (caught) {
        if (!alive) return;
        setError(caught);
        timer = setTimeout(run, pollMs * 2);
      }
    };
    void run();
    return () => {
      alive = false;
      if (timer) clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, tick, pollMs]);

  return { value, error, refresh };
}

// ---------------------------------------------------------------- shared pieces

function Brand({ institution }: { institution: string }) {
  return (
    <a className="cp-brand" href={href.home()}>
      <svg width="30" height="30" viewBox="0 0 30 30" aria-hidden="true">
        <circle cx="15" cy="15" r="14" fill="currentColor" />
        <path d="M8 20V11l7-3 7 3v9M12 20v-6h6v6" fill="none" stroke="#FFFFFF" strokeWidth="1.6" />
      </svg>
      <span className="cp-brand__text">
        <span className="cp-brand__name">PHarness Campus</span>
        <span className="cp-brand__org">{institution}</span>
      </span>
    </a>
  );
}

function StateBadge({ session }: { session: Pick<CampusLabSession, "status" | "stop_requested"> }) {
  const { label, tone } = sessionState(session);
  return <span className={`cp-badge cp-badge--${tone}`}>{label}</span>;
}

function Notice({ tone = "info", title, children, role }: { tone?: "info" | "warn" | "bad" | "good"; title: string; children?: React.ReactNode; role?: string }) {
  return (
    <div className={`cp-notice cp-notice--${tone}`} role={role}>
      <strong>{title}</strong>
      {children && <span>{children}</span>}
    </div>
  );
}

function describeError(error: unknown): string {
  if (error instanceof CampusApiError) {
    if (error.status === 401) return "Your sign-in ended. Sign in again to continue.";
    if (error.status === 403) return "You do not have access to this.";
    if (error.status === 404) return "This was not found, or you do not have access to it.";
    return error.message;
  }
  return "Campus is unavailable right now. Try again shortly.";
}

function Header({ onSignOut }: { onSignOut: () => void }) {
  const { me, institution, staffCourses, studentCourses } = useCampus();
  const who = me.identity.display_name || me.identity.email || "Signed in";
  const role = me.is_tenant_admin ? "Institution admin" : staffCourses.length ? "Instructor" : studentCourses.length ? "Student" : "Member";
  const items: { label: string; to: string }[] = [{ label: "My courses", to: href.home() }];
  for (const courseId of staffCourses) items.push({ label: `${courseLabel(courseId)} live cohort`, to: href.live(courseId) });
  if (me.is_tenant_admin || staffCourses.length) items.push({ label: "Lab profiles", to: href.profiles() });
  const current = typeof window === "undefined" ? "" : window.location.hash || "#/";
  return (
    <header className="cp-header">
      <Brand institution={institution} />
      <nav aria-label="Campus" className="cp-header__nav">
        {items.map(item => (
          <a key={item.to} href={item.to} aria-current={current === item.to ? "page" : undefined}>
            {item.label}
          </a>
        ))}
      </nav>
      <div className="cp-header__who">
        <span className="cp-header__identity">
          <span className="cp-header__name">{who}</span>
          <span className="cp-header__role">{role}</span>
        </span>
        <button type="button" className="cp-button cp-button--quiet" onClick={onSignOut}>
          Sign out
        </button>
      </div>
    </header>
  );
}

// ---------------------------------------------------------------- sign-in

export function SignInPage({ institution, failed, signedOut }: { institution: string; failed?: boolean; signedOut?: boolean }) {
  return (
    <main className="cp-signin">
      <div className="cp-signin__card">
        <Brand institution={institution} />
        <div className="cp-stack-8">
          <h1 className="cp-display">Sign in to PHarness Campus</h1>
          <p className="cp-lede">
            Your AI lab sessions run in private, short-lived sandboxes on campus infrastructure. Use the same account you use
            for other college services.
          </p>
        </div>
        {signedOut && <Notice tone="good" title="You signed out." role="status" />}
        {failed && (
          <Notice tone="bad" title="Campus is unavailable right now." role="alert">
            You are not signed in. Try again in a moment.
          </Notice>
        )}
        <a className="cp-button cp-button--primary cp-button--wide" href={SIGN_IN_PATH}>
          Continue with college sign-in
        </a>
        <p className="cp-fine">You will be sent to the college sign-in page, then back here.</p>
      </div>
    </main>
  );
}

// ---------------------------------------------------------------- student: courses

function AssignmentRow({ courseId, assignment }: { courseId: string; assignment: CampusAssignment }) {
  const live = assignment.session && LIVE_STATUSES.has(assignment.session.status) ? assignment.session : null;
  return (
    <li className={`cp-assignment${live ? " cp-assignment--live" : ""}`}>
      <div className="cp-assignment__body">
        <div className="cp-row-wrap">
          <span className="cp-assignment__title">{assignment.title}</span>
          <span className="cp-tag">{assignment.profile.kind === "coding" ? "Coding lab" : "Research lab"}</span>
          {live && <StateBadge session={live} />}
        </div>
        <span className="cp-muted">
          Up to {formatDuration(assignment.session_seconds)}. Profile {assignment.profile.id} {assignment.profile.version}.
        </span>
      </div>
      {live ? (
        <a className="cp-button cp-button--primary" href={href.lab(live.session_id)}>
          Open your lab
        </a>
      ) : (
        <a className="cp-button cp-button--primary" href={href.assignment(courseId, assignment.assignment_id)}>
          View lab
        </a>
      )}
    </li>
  );
}

function CourseSection({ courseId, role }: { courseId: string; role: "student" | "instructor" | "admin" }) {
  const { api } = useCampus();
  const { value, error } = usePolled(() => api.assignments(courseId), [courseId], 5000);
  return (
    <section className="cp-card cp-stack-16" aria-labelledby={`course-${courseId}`}>
      <div className="cp-row-between">
        <div className="cp-stack-4">
          <h2 id={`course-${courseId}`} className="cp-h2">
            {courseLabel(courseId)}
          </h2>
          <span className="cp-muted">{role === "student" ? "You are enrolled as a student." : role === "instructor" ? "You teach this course." : "Institution view."}</span>
        </div>
        {role !== "student" && (
          <a className="cp-button" href={href.live(courseId)}>
            Open live cohort
          </a>
        )}
      </div>
      <h3 className="cp-h3">Assignments</h3>
      {error && !value ? <Notice tone="bad" title={describeError(error)} /> : null}
      {!value && !error ? <p className="cp-muted" role="status">Loading assignments.</p> : null}
      {value && value.assignments.length === 0 && <p className="cp-muted">No labs are published for this course yet.</p>}
      {value && value.assignments.length > 0 && (
        <ul className="cp-list">
          {value.assignments.map(assignment => (
            <AssignmentRow key={assignment.assignment_id} courseId={courseId} assignment={assignment} />
          ))}
        </ul>
      )}
    </section>
  );
}

function AdminCoursePicker() {
  const { navigate } = useCampus();
  const [courseId, setCourseId] = useState("");
  return (
    <form
      className="cp-card cp-stack-12"
      onSubmit={event => {
        event.preventDefault();
        if (courseId.trim()) navigate(href.course(courseId.trim()));
      }}
    >
      <h2 className="cp-h2">Open a course</h2>
      <p className="cp-muted">As an institution admin you can open any course in this campus by its id.</p>
      <div className="cp-row-wrap">
        <label className="cp-field">
          <span>Course id</span>
          <input value={courseId} onChange={event => setCourseId(event.target.value)} placeholder="cs101" pattern="[A-Za-z0-9._-]{1,128}" />
        </label>
        <button className="cp-button cp-button--primary" type="submit">
          Open course
        </button>
      </div>
    </form>
  );
}

function HomePage() {
  const { me } = useCampus();
  const memberships = me.memberships ?? [];
  return (
    <main className="cp-main cp-stack-24">
      <h1 className="cp-display">My courses</h1>
      {memberships.length === 0 && !me.is_tenant_admin && (
        <Notice tone="info" title="You are not enrolled in a course yet.">
          Your instructor or the campus admin adds you to a course. Then its labs show here.
        </Notice>
      )}
      {memberships.map(membership => (
        <CourseSection key={membership.course_id} courseId={membership.course_id} role={membership.role} />
      ))}
      {me.is_tenant_admin && <AdminCoursePicker />}
    </main>
  );
}

function CoursePage({ courseId }: { courseId: string }) {
  const { me } = useCampus();
  const role = me.memberships?.find(m => m.course_id === courseId)?.role ?? (me.is_tenant_admin ? "admin" : "student");
  return (
    <main className="cp-main cp-stack-24">
      <a className="cp-back" href={href.home()}>
        Back to My courses
      </a>
      <CourseSection courseId={courseId} role={role} />
    </main>
  );
}

// ---------------------------------------------------------------- student: assignment and launch

function LimitsList({ assignment }: { assignment: CampusAssignment }) {
  if (!assignment.limits) {
    return (
      <dl className="cp-dl">
        <dt>Time</dt>
        <dd>Up to {formatDuration(assignment.session_seconds)}</dd>
      </dl>
    );
  }
  return (
    <dl className="cp-dl">
      {limitRows(assignment.limits).map(row => (
        <React.Fragment key={row.label}>
          <dt>{row.label}</dt>
          <dd>
            {row.value} {row.source && <span className="cp-muted">({row.source})</span>}
          </dd>
        </React.Fragment>
      ))}
    </dl>
  );
}

function AssignmentPage({ courseId, assignmentId }: { courseId: string; assignmentId: string }) {
  const { api, me, navigate } = useCampus();
  const { value, error } = usePolled(() => api.assignments(courseId), [courseId], 5000);
  const [launching, setLaunching] = useState(false);
  const [launchError, setLaunchError] = useState<{ title: string; body?: string; sessionId?: string | null } | null>(null);
  const idempotencyKey = useRef(newIdempotencyKey());
  const assignment = value?.assignments.find(a => a.assignment_id === assignmentId) ?? null;
  const live = assignment?.session && LIVE_STATUSES.has(assignment.session.status) ? assignment.session : null;

  async function launch() {
    setLaunching(true);
    setLaunchError(null);
    try {
      const result = await api.launch(courseId, assignmentId, me.csrf_token, idempotencyKey.current);
      navigate(href.lab(result.session.session_id));
    } catch (caught) {
      idempotencyKey.current = newIdempotencyKey();
      if (caught instanceof CampusApiError && caught.code === "active_session_exists") {
        setLaunchError({ title: "You already have a lab running.", body: "Open it, or end it before launching another.", sessionId: caught.sessionId });
      } else if (caught instanceof CampusApiError && (caught.code === "no_slots_available" || caught.code === "course_capacity_reached")) {
        setLaunchError({
          title: "All lab slots are in use right now.",
          body: "Your time has not started and this attempt is not counted. Try again in a few minutes, as other sessions end.",
        });
      } else if (caught instanceof CampusApiError && caught.code === "labs_unavailable") {
        setLaunchError({ title: "Labs cannot be launched right now.", body: "The campus lab service is paused or unavailable." });
      } else {
        setLaunchError({ title: describeError(caught) });
      }
    } finally {
      setLaunching(false);
    }
  }

  return (
    <main className="cp-main cp-stack-24">
      <a className="cp-back" href={href.course(courseId)}>
        Back to {courseLabel(courseId)}
      </a>
      {error && !value ? <Notice tone="bad" title={describeError(error)} role="alert" /> : null}
      {!value && !error ? <p className="cp-muted" role="status">Loading the lab.</p> : null}
      {value && !assignment && <Notice tone="bad" title="This lab was not found in the course." role="alert" />}
      {assignment && (
        <>
          <div className="cp-stack-8">
            <span className="cp-muted">{courseLabel(courseId)}</span>
            <div className="cp-row-wrap">
              <h1 className="cp-display">{assignment.title}</h1>
              <span className="cp-tag">{assignment.profile.kind === "coding" ? "Coding lab" : "Research lab"}</span>
            </div>
          </div>
          <div className="cp-split">
            <section className="cp-card cp-stack-12 cp-split__main" aria-labelledby="allows">
              <h2 id="allows" className="cp-h3">
                What this lab allows
              </h2>
              <LimitsList assignment={assignment} />
              <p className="cp-fine">
                The lab runs in an isolated sandbox with no access to college systems. When it ends, the workspace is deleted
                and its AI access is revoked.
              </p>
            </section>
            <section className="cp-card cp-card--accent cp-stack-16 cp-split__side" aria-labelledby="start">
              <h2 id="start" className="cp-h3">
                {live ? "Your lab is running" : "Start your lab"}
              </h2>
              {live ? (
                <>
                  <p className="cp-body">
                    Started {live.created_at ? formatTime(live.created_at) : "earlier"}, ends by {formatTime(live.expires_at)}.
                  </p>
                  <a className="cp-button cp-button--primary cp-button--wide" href={href.lab(live.session_id)}>
                    Open your lab
                  </a>
                </>
              ) : (
                <>
                  <p className="cp-body">
                    Profile <strong>{assignment.profile.id}</strong> {assignment.profile.version}. Up to{" "}
                    {formatDuration(assignment.session_seconds)}.
                  </p>
                  {launchError && (
                    <Notice tone="warn" title={launchError.title} role="alert">
                      {launchError.body}
                      {launchError.sessionId && (
                        <>
                          {" "}
                          <a href={href.lab(launchError.sessionId)}>Open your running lab</a>
                        </>
                      )}
                    </Notice>
                  )}
                  <button
                    type="button"
                    className="cp-button cp-button--primary cp-button--wide"
                    disabled={!assignment.launchable || launching}
                    onClick={() => void launch()}
                  >
                    {launching ? "Launching" : "Launch lab"}
                  </button>
                  {!assignment.launchable && <p className="cp-fine">Only enrolled students can launch this lab.</p>}
                </>
              )}
            </section>
          </div>
        </>
      )}
    </main>
  );
}

// ---------------------------------------------------------------- student: lab session

const STEPS = [
  { key: "queued", label: "Admitted", help: "A lab slot was reserved for you" },
  { key: "provisioning", label: "Provisioning", help: "Preparing your private sandbox" },
  { key: "ready", label: "Ready", help: "Your terminal is open" },
];

function LaunchProgress({ session }: { session: CampusLabSession }) {
  const stage = session.status === "ready" ? 2 : 1;
  return (
    <ol aria-label="Launch progress" className="cp-steps">
      {STEPS.map((step, index) => {
        const done = index < stage;
        const now = index === stage;
        return (
          <li key={step.key} aria-current={now ? "step" : undefined} className={now ? "cp-steps__now" : undefined}>
            <span className="cp-stack-2">
              <span className="cp-steps__label">{step.label}</span>
              <span className="cp-muted">{step.help}</span>
            </span>
            <span className={`cp-steps__status${done ? " cp-steps__status--done" : now ? " cp-steps__status--now" : ""}`}>
              {done ? "Done" : now ? "In progress" : "Not started"}
            </span>
          </li>
        );
      })}
    </ol>
  );
}

function Countdown({ expiresAt }: { expiresAt: number }) {
  const now = useNow();
  const left = expiresAt - now;
  const tone = left < 120 ? "bad" : left < 600 ? "warn" : "ok";
  return (
    <div className={`cp-countdown cp-countdown--${tone}`}>
      <span className="cp-countdown__label">Time left</span>
      <span className="cp-countdown__value" aria-live="off">
        {formatClock(left)}
      </span>
      <span className="cp-muted">Ends at {formatTime(expiresAt)}. The workspace is deleted then.</span>
    </div>
  );
}

function LabEnded({ session }: { session: CampusLabSession }) {
  const failed = session.status === "failed";
  const blocked = session.status === "cleanup_blocked";
  return (
    <section className="cp-card cp-stack-16" aria-labelledby="ended">
      <h2 id="ended" className="cp-h2">
        {failed ? "Your lab could not start" : blocked ? "Your lab ended; cleanup is pending" : "Your lab has ended"}
      </h2>
      {failed ? (
        <p className="cp-body">{session.error?.message ?? "The sandbox failed to start."} This attempt does not count against you.</p>
      ) : (
        <dl className="cp-dl">
          <dt>Started</dt>
          <dd>{session.created_at ? formatTime(session.created_at) : "Unknown"}</dd>
          <dt>Ended</dt>
          <dd>{session.terminated_at ? formatTime(session.terminated_at) : "Ending now"}</dd>
          <dt>Workspace</dt>
          <dd>{blocked ? "Deletion is waiting on the campus operator. Its AI access is already revoked." : "Deleted. Its AI access was revoked."}</dd>
        </dl>
      )}
      <div className="cp-row-wrap">
        <a className="cp-button cp-button--primary" href={href.assignment(session.course_id, session.assignment_id)}>
          Back to the lab
        </a>
        <a className="cp-button" href={href.course(session.course_id)}>
          Back to {courseLabel(session.course_id)}
        </a>
      </div>
    </section>
  );
}

function LabPage({ sessionId }: { sessionId: string }) {
  const { api, createWidget } = useCampus();
  const { value, error } = usePolled(
    () => api.session(sessionId),
    [sessionId],
    2500,
    ({ session }) => session.status !== "stopped" && session.status !== "failed",
  );
  const session = value?.session ?? null;
  const ended = session && (session.status === "stopped" || session.status === "failed");
  const showTerminal = session && !ended && (session.status === "ready" || session.status === "stopping" || session.stop_requested);
  const [everReady, setEverReady] = useState(false);
  useEffect(() => {
    if (session?.status === "ready") setEverReady(true);
  }, [session?.status]);

  return (
    <main className="cp-main cp-main--wide cp-stack-20">
      <div className="cp-row-between">
        <div className="cp-stack-4">
          <span className="cp-muted">{session ? courseLabel(session.course_id) : "Lab session"}</span>
          <h1 className="cp-display cp-display--sm">
            {!session ? "Your lab" : ended ? "Lab ended" : session.status === "provisioning" ? "Starting your lab" : "Your lab"}
          </h1>
        </div>
        {session && <StateBadge session={session} />}
      </div>
      {error && !value ? <Notice tone="bad" title={describeError(error)} role="alert" /> : null}
      {!value && !error ? <p className="cp-muted" role="status">Loading your lab.</p> : null}
      {session && session.status === "provisioning" && !session.stop_requested && (
        <div className="cp-narrow cp-stack-16">
          <LaunchProgress session={session} />
          <Notice tone="info" title="Nothing is needed from you yet." role="status">
            Your sandbox is being created with its own network rules, AI allowance and time limit. This usually takes under a
            minute.
          </Notice>
        </div>
      )}
      {showTerminal && (
        <div className="cp-workspace">
          <section className="cp-workspace__terminal" aria-label="Lab terminal">
            <CampusTerminalApp key={sessionId} sessionId={sessionId} createWidget={createWidget} embedded />
          </section>
          <aside className="cp-workspace__side cp-stack-16" aria-label="Session status">
            <Countdown expiresAt={session.expires_at} />
            <div className="cp-stack-6">
              <span className="cp-side-label">Sandbox</span>
              <span className="cp-body">
                Profile {session.profile?.id ?? "unknown"} {session.profile?.version ?? ""}
                {session.generation && session.generation > 1 ? `, generation ${session.generation}` : ""}
              </span>
              <span className="cp-muted">Isolated, short-lived, and deleted when it ends.</span>
            </div>
            <div className="cp-stack-6">
              <span className="cp-side-label">Try in the terminal</span>
              <code className="cp-code">codex --version</code>
              <code className="cp-code">pwd &amp;&amp; ls -la</code>
              <code className="cp-code">curl -sI https://example.com</code>
              <span className="cp-muted">The last one shows the network policy at work.</span>
            </div>
            <a className="cp-button" href={href.assignment(session.course_id, session.assignment_id)}>
              Lab details
            </a>
          </aside>
        </div>
      )}
      {session && ended && <LabEnded session={session} />}
      {session && !everReady && session.stop_requested && session.status === "provisioning" && (
        <Notice tone="warn" title="This lab is stopping." role="status" />
      )}
    </main>
  );
}

// ---------------------------------------------------------------- instructor: live cohort

function memberName(members: CampusMember[] | undefined, owner: CampusLabSession["owner"]): string {
  if (!owner) return "Unknown student";
  const member = members?.find(m => m.subject === owner.subject && m.issuer === owner.issuer);
  return member?.display_name || member?.email || `Student ${owner.subject.slice(0, 8)}`;
}

function LivePage({ courseId }: { courseId: string }) {
  const { api, me } = useCampus();
  const now = useNow();
  const sessions = usePolled(() => api.courseSessions(courseId), [courseId], 3000);
  const members = usePolled(() => api.members(courseId), [courseId], 30000);
  const assignments = usePolled(() => api.assignments(courseId), [courseId], 60000);
  const [confirm, setConfirm] = useState<{ ids: string[]; title: string } | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const list = (sessions.value?.sessions ?? []).filter(s => LIVE_STATUSES.has(s.status));
  const titleOf = (assignmentId: string) =>
    assignments.value?.assignments.find(a => a.assignment_id === assignmentId)?.title ?? assignmentId;
  const running = list.filter(s => s.status === "ready" && !s.stop_requested).length;
  const starting = list.filter(s => s.status === "provisioning").length;
  const ending = list.filter(s => s.status === "stopping" || s.stop_requested).length;
  const blocked = list.filter(s => s.status === "cleanup_blocked").length;
  const students = (members.value?.members ?? []).filter(m => m.role === "student").length;

  async function stopAll(ids: string[]) {
    setConfirm(null);
    let stopped = 0;
    for (const id of ids) {
      try {
        await api.stop(id, me.csrf_token);
        stopped += 1;
      } catch {
        // Already ended or no longer visible; the next poll shows the truth.
      }
    }
    setMessage(stopped === 1 ? "Ending 1 lab. Its sandbox and AI access are being removed." : `Ending ${stopped} labs. Their sandboxes and AI access are being removed.`);
    sessions.refresh();
  }

  return (
    <main className="cp-main cp-main--wide cp-stack-20">
      <div className="cp-row-between">
        <div className="cp-stack-4">
          <h1 className="cp-display">{courseLabel(courseId)} live cohort</h1>
          <p className="cp-body" role="status">
            {sessions.value
              ? `${running} running, ${starting} starting, ${ending} ending${blocked ? `, ${blocked} cleanup blocked` : ""}. ${students} students enrolled.`
              : "Loading live sessions."}
          </p>
        </div>
        <button
          type="button"
          className="cp-button cp-button--danger"
          disabled={list.filter(s => !s.stop_requested).length === 0}
          onClick={() =>
            setConfirm({ ids: list.filter(s => !s.stop_requested).map(s => s.session_id), title: "End every running lab in this course?" })
          }
        >
          End lab for everyone
        </button>
      </div>
      {sessions.error && !sessions.value ? <Notice tone="bad" title={describeError(sessions.error)} role="alert" /> : null}
      {message && <Notice tone="good" title={message} />}
      {confirm && (
        <div className="cp-confirm" role="group" aria-label="Confirm ending labs">
          <div className="cp-stack-4">
            <strong>{confirm.title}</strong>
            <span>
              {confirm.ids.length === 1 ? "The student's" : `${confirm.ids.length} students'`} terminal closes at once, the
              sandbox is deleted, and its AI access is revoked. This is recorded.
            </span>
          </div>
          <div className="cp-row-wrap">
            <button type="button" className="cp-button cp-button--danger" onClick={() => void stopAll(confirm.ids)}>
              End {confirm.ids.length === 1 ? "lab" : `${confirm.ids.length} labs`}
            </button>
            <button type="button" className="cp-button" onClick={() => setConfirm(null)}>
              Cancel
            </button>
          </div>
        </div>
      )}
      <div className="cp-table-wrap">
        <table className="cp-table">
          <caption className="cp-sr">Live lab sessions with student, lab, state, time left and actions</caption>
          <thead>
            <tr>
              <th scope="col">Student</th>
              <th scope="col">Lab</th>
              <th scope="col">State</th>
              <th scope="col">Started</th>
              <th scope="col">Time left</th>
              <th scope="col">
                <span className="cp-sr">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {list.length === 0 && (
              <tr>
                <td colSpan={6} className="cp-muted">
                  {sessions.value ? "No labs are running in this course right now." : "Loading."}
                </td>
              </tr>
            )}
            {list.map(session => {
              const name = memberName(members.value?.members, session.owner);
              return (
                <tr key={session.session_id}>
                  <td className="cp-strong">{name}</td>
                  <td>{titleOf(session.assignment_id)}</td>
                  <td>
                    <StateBadge session={session} />
                  </td>
                  <td>{session.created_at ? formatTime(session.created_at) : ""}</td>
                  <td className="cp-mono">{session.status === "ready" ? formatClock(session.expires_at - now) : ""}</td>
                  <td className="cp-actions">
                    <button
                      type="button"
                      className="cp-button cp-button--small"
                      disabled={session.stop_requested || session.status === "stopping"}
                      aria-label={`End ${name}'s lab`}
                      onClick={() => setConfirm({ ids: [session.session_id], title: `End ${name}'s lab?` })}
                    >
                      End lab
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <p className="cp-fine">Refreshes every few seconds. Instructors can end a lab but cannot open a student's terminal.</p>
    </main>
  );
}

// ---------------------------------------------------------------- staff: lab profiles

function ProfilesPage() {
  const { api, me, staffCourses } = useCampus();
  const courses = staffCourses.length ? staffCourses : me.is_tenant_admin ? (me.memberships ?? []).map(m => m.course_id) : [];
  const [extra, setExtra] = useState<string[]>([]);
  const [draft, setDraft] = useState("");
  const all = Array.from(new Set([...courses, ...extra]));
  return (
    <main className="cp-main cp-main--wide cp-stack-24">
      <div className="cp-stack-8">
        <h1 className="cp-display">Lab profiles and limits</h1>
        <p className="cp-lede">
          Every lab runs from an approved, versioned profile: the sandbox image and harness, its isolation, network rules, AI
          models and budget. Institution, instructor and assignment layers can only narrow what the profile allows.
        </p>
      </div>
      {all.map(courseId => (
        <ProfileCourse key={courseId} courseId={courseId} api={api} />
      ))}
      {me.is_tenant_admin && (
        <form
          className="cp-row-wrap"
          onSubmit={event => {
            event.preventDefault();
            if (draft.trim()) setExtra(previous => [...previous, draft.trim()]);
            setDraft("");
          }}
        >
          <label className="cp-field">
            <span>Show a course</span>
            <input value={draft} onChange={event => setDraft(event.target.value)} placeholder="cs101" />
          </label>
          <button className="cp-button" type="submit">
            Show
          </button>
        </form>
      )}
    </main>
  );
}

function ProfileCourse({ courseId, api }: { courseId: string; api: CampusPortalApi }) {
  const { value, error } = usePolled(() => api.assignments(courseId), [courseId], 60000);
  return (
    <section className="cp-stack-12" aria-labelledby={`profiles-${courseId}`}>
      <h2 id={`profiles-${courseId}`} className="cp-h2">
        {courseLabel(courseId)}
      </h2>
      {error && !value ? <Notice tone="bad" title={describeError(error)} /> : null}
      <div className="cp-grid">
        {value?.assignments.map(assignment => (
          <article key={assignment.assignment_id} className="cp-card cp-stack-12">
            <div className="cp-stack-4">
              <h3 className="cp-h3">{assignment.title}</h3>
              <span className="cp-mono cp-muted">
                {assignment.profile.id} {assignment.profile.version} · {assignment.profile.kind}
              </span>
            </div>
            <LimitsList assignment={assignment} />
          </article>
        ))}
      </div>
    </section>
  );
}

// ---------------------------------------------------------------- app

type Load = { phase: "loading" } | { phase: "signed_out"; signedOut?: boolean } | { phase: "unavailable" } | { phase: "ready"; me: CampusMe };

export type CampusAppProps = {
  createWidget: () => TerminalWidget;
  api?: CampusPortalApi;
  institution?: string;
};

export function CampusApp({ createWidget, api = campusPortalApi, institution = "Campus pilot" }: CampusAppProps) {
  const [load, setLoad] = useState<Load>({ phase: "loading" });
  const [route, setRoute] = useState<CampusRoute>(() => parseRoute(typeof window === "undefined" ? "" : window.location.hash));

  useEffect(() => {
    const onHash = () => {
      setRoute(parseRoute(window.location.hash));
      window.scrollTo?.(0, 0);
    };
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  useEffect(() => {
    let alive = true;
    api
      .me()
      .then(me => alive && setLoad({ phase: "ready", me }))
      .catch(error => {
        if (!alive) return;
        setLoad(error instanceof CampusApiError && error.status === 401 ? { phase: "signed_out" } : { phase: "unavailable" });
      });
    return () => {
      alive = false;
    };
  }, [api]);

  const context = useMemo<CampusContextValue | null>(() => {
    if (load.phase !== "ready") return null;
    const memberships = load.me.memberships ?? [];
    return {
      me: load.me,
      api,
      institution,
      createWidget,
      navigate: hash => {
        window.location.hash = hash;
      },
      staffCourses: memberships.filter(m => m.role === "instructor").map(m => m.course_id),
      studentCourses: memberships.filter(m => m.role === "student").map(m => m.course_id),
    };
  }, [load, api, institution, createWidget]);

  if (load.phase === "loading") {
    return (
      <p className="cp-loading" role="status">
        Loading PHarness Campus.
      </p>
    );
  }
  if (load.phase === "signed_out") return <SignInPage institution={institution} signedOut={load.signedOut} />;
  if (load.phase === "unavailable" || !context) return <SignInPage institution={institution} failed />;

  async function signOut() {
    try {
      await api.logout(context!.me.csrf_token);
    } catch {
      // An already ended sign-in still ends here.
    }
    window.location.hash = "#/";
    setLoad({ phase: "signed_out", signedOut: true });
  }

  return (
    <CampusContext.Provider value={context}>
      <div className="cp-app">
        <Header onSignOut={() => void signOut()} />
        {route.name === "home" && <HomePage />}
        {route.name === "course" && <CoursePage courseId={route.courseId} />}
        {route.name === "assignment" && <AssignmentPage courseId={route.courseId} assignmentId={route.assignmentId} />}
        {route.name === "lab" && <LabPage sessionId={route.sessionId} />}
        {route.name === "live" && <LivePage courseId={route.courseId} />}
        {route.name === "profiles" && <ProfilesPage />}
      </div>
    </CampusContext.Provider>
  );
}
