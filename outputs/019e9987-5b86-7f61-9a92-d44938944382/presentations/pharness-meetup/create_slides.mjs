import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const workspace = path.dirname(fileURLToPath(import.meta.url));
const slidesDir = path.join(workspace, "slides");

const utils = String.raw`
import path from "node:path";

export const C = {
  bg: "#071013",
  panel: "#0B171C",
  panel2: "#10242C",
  line: "#28424A",
  text: "#F2F7F8",
  soft: "#C6D2D6",
  muted: "#8DA1AA",
  cyan: "#32D7DF",
  green: "#36D86B",
  amber: "#F2BC2E",
  red: "#FF584F",
  blue: "#45A8FF",
  purple: "#A985FF",
  white: "#FFFFFF",
};

export function screenshotPath() {
  return path.resolve(process.cwd(), "../pharness-ui/pharness-prototype-flow.png");
}

export function addBase(presentation, ctx, opts = {}) {
  const slide = presentation.slides.add();
  ctx.addShape(slide, { x: 0, y: 0, w: ctx.W, h: ctx.H, fill: C.bg });
  ctx.addShape(slide, { x: 0, y: 0, w: ctx.W, h: 92, fill: "#08161B" });
  ctx.addShape(slide, { x: 0, y: 91, w: ctx.W, h: 1, fill: C.line });
  ctx.addText(slide, {
    text: "PHarness",
    x: 54,
    y: 26,
    w: 220,
    h: 24,
    fontSize: 24,
    bold: true,
    color: C.text,
    typeface: ctx.fonts.title,
  });
  ctx.addText(slide, {
    text: opts.kicker || "agent SDLC control plane",
    x: 54,
    y: 62,
    w: 360,
    h: 16,
    fontSize: 11,
    color: C.muted,
  });
  ctx.addText(slide, {
    text: String(ctx.slideNumber).padStart(2, "0"),
    x: 1160,
    y: 34,
    w: 60,
    h: 24,
    fontSize: 13,
    color: C.muted,
    align: "right",
  });
  return slide;
}

export function title(slide, ctx, text, subtitle) {
  ctx.addText(slide, {
    text,
    x: 78,
    y: 144,
    w: 940,
    h: 72,
    fontSize: 43,
    bold: true,
    color: C.text,
    typeface: ctx.fonts.title,
  });
  if (subtitle) {
    ctx.addText(slide, {
      text: subtitle,
      x: 82,
      y: 216,
      w: 860,
      h: 44,
      fontSize: 19,
      color: C.soft,
    });
  }
}

export function sectionLabel(slide, ctx, text, x = 78, y = 112) {
  ctx.addText(slide, {
    text: text.toUpperCase(),
    x,
    y,
    w: 320,
    h: 20,
    fontSize: 11,
    bold: true,
    color: C.cyan,
  });
}

export function paragraph(slide, ctx, text, x, y, w, size = 24, color = C.soft) {
  ctx.addText(slide, {
    text,
    x,
    y,
    w,
    h: 96,
    fontSize: size,
    color,
    insets: { left: 0, right: 0, top: 0, bottom: 0 },
  });
}

export function bullets(slide, ctx, items, x, y, w, opts = {}) {
  const gap = opts.gap || 48;
  const size = opts.size || 23;
  const bulletColor = opts.bulletColor || C.cyan;
  items.forEach((item, index) => {
    const yy = y + index * gap;
    ctx.addShape(slide, { x, y: yy + 11, w: 9, h: 9, fill: bulletColor, geometry: "ellipse" });
    ctx.addText(slide, {
      text: item,
      x: x + 25,
      y: yy,
      w,
      h: Math.max(28, gap - 10),
      fontSize: size,
      color: opts.color || C.soft,
    });
  });
}

export function card(slide, ctx, x, y, w, h, heading, body, tone = C.cyan) {
  ctx.addShape(slide, { x, y, w, h, fill: C.panel, line: ctx.line(C.line, 1) });
  ctx.addShape(slide, { x, y, w: 6, h, fill: tone });
  ctx.addText(slide, {
    text: heading,
    x: x + 22,
    y: y + 18,
    w: w - 44,
    h: 28,
    fontSize: 20,
    bold: true,
    color: C.text,
  });
  ctx.addText(slide, {
    text: body,
    x: x + 22,
    y: y + 54,
    w: w - 44,
    h: h - 70,
    fontSize: 16,
    color: C.soft,
  });
}

export function pill(slide, ctx, text, x, y, w, color = C.cyan) {
  ctx.addShape(slide, { x, y, w, h: 34, fill: "#102A31", line: ctx.line(color, 1) });
  ctx.addText(slide, {
    text,
    x: x + 12,
    y: y + 7,
    w: w - 24,
    h: 20,
    fontSize: 13,
    bold: true,
    color,
    align: "center",
  });
}

export function node(slide, ctx, label, x, y, w, h, color = C.cyan, sub = "") {
  ctx.addShape(slide, { x, y, w, h, fill: C.panel2, line: ctx.line(color, 1.2) });
  ctx.addText(slide, {
    text: label,
    x: x + 14,
    y: y + 16,
    w: w - 28,
    h: 26,
    fontSize: 18,
    bold: true,
    color: C.text,
    align: "center",
  });
  if (sub) {
    ctx.addText(slide, {
      text: sub,
      x: x + 12,
      y: y + 47,
      w: w - 24,
      h: 20,
      fontSize: 12,
      color: C.muted,
      align: "center",
    });
  }
}

export function flowArrow(slide, ctx, x, y, w = 38) {
  ctx.addShape(slide, { x, y: y + 11, w, h: 2, fill: C.line });
  ctx.addShape(slide, { x: x + w - 7, y: y + 6, w: 12, h: 12, fill: C.line, geometry: "triangle" });
}

export function bigNumber(slide, ctx, n, label, x, y, color = C.cyan) {
  ctx.addText(slide, {
    text: n,
    x,
    y,
    w: 135,
    h: 70,
    fontSize: 55,
    bold: true,
    color,
    align: "center",
  });
  ctx.addText(slide, {
    text: label,
    x: x - 8,
    y: y + 72,
    w: 150,
    h: 38,
    fontSize: 14,
    color: C.muted,
    align: "center",
  });
}
`;

const slides = [
  {
    title: "PHarness",
    subtitle: "An agent harness as an SDLC control plane",
    kind: "cover",
  },
  {
    title: "The claim",
    subtitle: "The hard part is not making a model call a tool.",
    bullets: ["preserve operator intent", "enforce policy boundaries", "make actions replayable", "attach durable evidence", "keep rollback context visible"],
  },
  {
    title: "Why build this?",
    subtitle: "Agent tools blur trust domains that should stay separate.",
    cards: [
      ["Chat", "Useful for exploration, weak as an audit boundary.", "#A985FF"],
      ["Local automation", "Fast feedback against a repo, shell, git, and tests.", "#32D7DF"],
      ["Delivery operations", "Needs policy, evidence, approvals, and rollback.", "#F2BC2E"],
    ],
  },
  {
    title: "Philosophy",
    subtitle: "Small runtime, explicit boundaries, durable state.",
    bullets: ["small surface area beats plugin sprawl", "typed actions beat opaque shell commands", "policy belongs in the runtime, not the prompt", "autonomy should be bounded by explicit envelopes", "chat is secondary to runs, evidence, and audit"],
  },
  {
    title: "What PHarness is",
    subtitle: "A local-first Rust agent harness designed to grow into a Kubernetes-native delivery runtime.",
    bullets: ["Fireworks-first model provider", "one-action-per-turn agent loop", "file, shell, git, patch, and typed read capabilities", "conservative safety policy and approval resume", "durable SQLite-backed sessions, events, artifacts, diffs, observations, gates, grants, and audit"],
  },
  {
    title: "What it is not",
    subtitle: "The non-goals are as important as the goals.",
    bullets: ["not a plugin marketplace", "not an MCP-first runtime", "not a chat UI with tools bolted on", "not a hidden permission bypass", "not an autonomous production deployer"],
  },
  {
    title: "Current state",
    subtitle: "The control-plane slice exists now.",
    bullets: ["run API, worker execution, and SSE event streaming", "Fireworks native tool calling as default", "approval queues and exact action resume", "typed read-only Kubernetes, Argo, Tekton, Prometheus, and Loki capabilities", "durable observations, incidents, remediation plans, work plans, change sets, approval gates, permission grants, and audit events"],
  },
  {
    title: "Local-first, cluster-native",
    subtitle: "V1 runs locally, but the nouns are shaped for the cluster.",
    pills: ["ExecutionTarget", "ResourceRef", "ArtifactRef", "CapabilityKind", "RunScope"],
    note: "The point is to avoid painting V1 into a corner while still keeping the first useful implementation small.",
  },
  {
    title: "What the operator sees",
    subtitle: "Flow, Queue, Approvals, and Approval Gates are lenses over the same resources.",
    image: true,
  },
  {
    title: "The SDLC shape",
    subtitle: "Future CRDs, current database resources.",
    kind: "sdlc",
  },
  {
    title: "Runtime architecture",
    subtitle: "The model proposes. The runtime decides. The store remembers.",
    kind: "runtime",
  },
  {
    title: "One action per turn",
    subtitle: "A narrow loop makes policy, replay, and approval resume tractable.",
    steps: ["model request", "one proposed AgentAction", "shape validation", "policy evaluation", "execute / pause / deny", "append result"],
  },
  {
    title: "Policy is a runtime contract",
    subtitle: "A resumed run should not inherit config drift by accident.",
    bullets: ["read-only actions allowed", "file writes ask by default", "network and destructive shell commands ask", "privileged and secret-shaped actions deny", "typed cluster reads allow unless secret-shaped", "grants cannot override denials"],
  },
  {
    title: "Two approval surfaces",
    subtitle: "Tool approvals and governance gates are different resources.",
    compare: [
      ["Tool approval", "May this paused run execute this exact action?", "approve / deny", "#F2BC2E"],
      ["Approval gate", "Has this governance checkpoint been satisfied?", "satisfy / waive / reject", "#32D7DF"],
    ],
  },
  {
    title: "Typed capabilities",
    subtitle: "Production delivery should not become a pile of shell wrappers.",
    bullets: ["kubernetes_get", "argo_get_app", "tekton_get_pipeline_runs", "tekton_get_task_runs", "tekton_analyze_pipeline_run", "prometheus_query / prometheus_inventory", "loki_log_summary"],
  },
  {
    title: "Tekton analysis example",
    subtitle: "Turn scattered cluster facts into structured SDLC evidence.",
    bullets: ["PipelineRun status, reason, and timing", "TaskRun status counts", "repo URL, commit SHA, image reference, and digest", "deployment target and rollout health", "Argo sync and health", "registry-aware image alignment"],
  },
  {
    title: "Trusted autonomy",
    subtitle: "The goal is bounded autonomy, not fewer prompts by default.",
    bullets: ["scoped to environment, repo, branch, and namespace", "scoped to WorkPlan or ChangeSet", "scoped to capability kind and action", "time-bounded and auditable", "invalidated when material plans change", "today: local file writes only"],
  },
  {
    title: "Technical challenges",
    subtitle: "The interesting parts are mostly boring operational details.",
    cards: [
      ["Model protocols", "Tool support, malformed arguments, streaming assembly, and one-action discipline.", "#A985FF"],
      ["Approval resume", "Persist and resume the exact reviewed action, not a later model interpretation.", "#F2BC2E"],
      ["Cluster output", "Parse before redaction, compact the payload, and deny secret-shaped reads early.", "#32D7DF"],
      ["Drift and aliases", "Registry host aliases should reduce false drift without hiding real mismatches.", "#45A8FF"],
    ],
  },
  {
    title: "What I would demo",
    subtitle: "Show that the runtime knows what happened.",
    steps: ["run read-only task", "show event stream", "show policy decision", "trigger write approval", "inspect preview diff", "approve and resume", "show final diff and audit trail", "show typed cluster read"],
  },
  {
    title: "Roadmap",
    subtitle: "Keep V1 useful while moving toward governed cluster execution.",
    cards: [
      ["Near term", "WorkPlan and ChangeSet readiness, scoped grant negative smokes, stronger evidence retrieval, more dogfooding.", "#32D7DF"],
      ["V2", "Kubernetes worker pods, per-run workspace sandboxes, API/UI services, optional Postgres.", "#F2BC2E"],
      ["V3", "Tekton intents, Argo deployment intents, database operator capabilities, LGTM loops, CRD-backed resources.", "#36D86B"],
    ],
  },
  {
    title: "The risk",
    subtitle: "The obvious failure mode is overbuilding.",
    bullets: ["keep the policy model readable", "make every action explainable", "add cluster capabilities one at a time", "keep V1 locally useful", "delete abstractions that do not make production workflows safer or clearer"],
  },
  {
    title: "Takeaways",
    subtitle: "Useful agent autonomy needs a control plane.",
    bullets: ["typed actions", "scoped trust", "durable evidence", "explicit approvals", "replayable runs", "cluster-native resource vocabulary"],
    closing: "The model is the planner and actor. PHarness is the system that keeps it inside a governed operational story.",
  },
];

function safeText(value) {
  return String(value).replaceAll("`", "\\`").replaceAll("${", "\\${");
}

function moduleFor(slide, index) {
  const n = String(index + 1).padStart(2, "0");
  return `import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide${n}(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  ${bodyFor(slide)}
  return slide;
}
`;
}

function bodyFor(s) {
  if (s.kind === "cover") {
    return `
  ctx.addShape(slide, { x: 78, y: 142, w: 104, h: 104, fill: "#0D2C33", line: ctx.line(C.cyan, 2) });
  ctx.addText(slide, { text: "P", x: 104, y: 158, w: 54, h: 66, fontSize: 56, bold: true, color: C.cyan, align: "center" });
  ctx.addText(slide, { text: "${safeText(s.title)}", x: 208, y: 138, w: 820, h: 84, fontSize: 62, bold: true, color: C.text, typeface: ctx.fonts.title });
  ctx.addText(slide, { text: "${safeText(s.subtitle)}", x: 214, y: 232, w: 850, h: 44, fontSize: 26, color: C.soft });
  ctx.addText(slide, { text: "Local-first today. Cluster-native by design.", x: 218, y: 304, w: 760, h: 32, fontSize: 21, color: C.cyan });
  card(slide, ctx, 78, 430, 1080, 118, "Meetup framing", "A casual technical story about why PHarness exists, what has been built, and where the hard engineering problems actually are.", C.cyan);
`;
  }
  if (s.cards) {
    const cols = s.cards.length === 4 ? [78, 362, 646, 930] : s.cards.length === 3 ? [78, 442, 806] : [78, 670];
    const width = s.cards.length === 4 ? 250 : s.cards.length === 3 ? 320 : 510;
    const y = s.cards.length === 4 ? 312 : 322;
    return `
  sectionLabel(slide, ctx, "story");
  title(slide, ctx, "${safeText(s.title)}", "${safeText(s.subtitle)}");
  ${s.cards.map((c, i) => `card(slide, ctx, ${cols[i]}, ${y}, ${width}, ${s.cards.length === 4 ? 210 : 190}, "${safeText(c[0])}", "${safeText(c[1])}", "${c[2]}");`).join("\n  ")}
`;
  }
  if (s.pills) {
    return `
  sectionLabel(slide, ctx, "architecture");
  title(slide, ctx, "${safeText(s.title)}", "${safeText(s.subtitle)}");
  ${s.pills.map((p, i) => `pill(slide, ctx, "${safeText(p)}", ${96 + (i % 3) * 330}, ${322 + Math.floor(i / 3) * 68}, 270, ${i % 2 ? "C.amber" : "C.cyan"});`).join("\n  ")}
  paragraph(slide, ctx, "${safeText(s.note)}", 102, 500, 920, 24, C.soft);
`;
  }
  if (s.image) {
    return `
  sectionLabel(slide, ctx, "product");
  title(slide, ctx, "${safeText(s.title)}", "${safeText(s.subtitle)}");
  await ctx.addImage(slide, { path: screenshotPath(), x: 84, y: 260, w: 1090, h: 390, fit: "contain", alt: "PHarness prototype flow UI" });
`;
  }
  if (s.kind === "sdlc") {
    return `
  sectionLabel(slide, ctx, "resource model");
  title(slide, ctx, "${safeText(s.title)}", "${safeText(s.subtitle)}");
  const labels = ["WorkItem", "WorkPlan", "ChangeSet", "PipelineIntent", "PipelineRunAnalysis", "DeploymentIntent", "Release"];
  labels.forEach((label, i) => { node(slide, ctx, label, 66 + i * 166, 300, 132, 86, i < 3 ? C.cyan : i < 5 ? C.amber : C.green); if (i < labels.length - 1) flowArrow(slide, ctx, 200 + i * 166, 330, 30); });
  node(slide, ctx, "Observation", 142, 500, 150, 70, C.blue, "read-only fact");
  flowArrow(slide, ctx, 294, 525, 50);
  node(slide, ctx, "Incident", 348, 500, 150, 70, C.red, "candidate");
  flowArrow(slide, ctx, 500, 525, 50);
  node(slide, ctx, "RemediationPlan", 554, 500, 170, 70, C.purple, "draft");
  flowArrow(slide, ctx, 728, 525, 56);
  node(slide, ctx, "WorkPlan", 790, 500, 150, 70, C.cyan, "handoff");
  pill(slide, ctx, "PermissionGrant bounds autonomy", 166, 626, 310, C.green);
  pill(slide, ctx, "ApprovalGate gates risky transitions", 502, 626, 330, C.amber);
  pill(slide, ctx, "AuditEvent records decisions", 858, 626, 280, C.blue);
`;
  }
  if (s.kind === "runtime") {
    return `
  sectionLabel(slide, ctx, "runtime");
  title(slide, ctx, "${safeText(s.title)}", "${safeText(s.subtitle)}");
  node(slide, ctx, "CLI / API / UI", 88, 310, 160, 78, C.cyan);
  flowArrow(slide, ctx, 250, 335, 50);
  node(slide, ctx, "Run Queue", 304, 310, 150, 78, C.cyan);
  flowArrow(slide, ctx, 456, 335, 50);
  node(slide, ctx, "Worker", 510, 310, 150, 78, C.amber);
  flowArrow(slide, ctx, 662, 335, 50);
  node(slide, ctx, "Fireworks", 716, 310, 150, 78, C.purple);
  flowArrow(slide, ctx, 868, 335, 50);
  node(slide, ctx, "AgentAction", 922, 310, 170, 78, C.green);
  node(slide, ctx, "SafetyPolicy", 510, 458, 170, 78, C.red);
  node(slide, ctx, "ToolExecutor", 738, 458, 170, 78, C.cyan);
  node(slide, ctx, "Store: events, artifacts, audit", 430, 604, 420, 62, C.blue);
  ctx.addShape(slide, { x: 590, y: 390, w: 2, h: 68, fill: C.line });
  ctx.addShape(slide, { x: 680, y: 496, w: 58, h: 2, fill: C.line });
  ctx.addShape(slide, { x: 590, y: 536, w: 2, h: 68, fill: C.line });
`;
  }
  if (s.steps) {
    return `
  sectionLabel(slide, ctx, "flow");
  title(slide, ctx, "${safeText(s.title)}", "${safeText(s.subtitle)}");
  ${s.steps.map((step, i) => `bigNumber(slide, ctx, "${i + 1}", "${safeText(step)}", ${90 + (i % 4) * 275}, ${318 + Math.floor(i / 4) * 160}, ${i % 2 ? "C.amber" : "C.cyan"});`).join("\n  ")}
`;
  }
  if (s.compare) {
    return `
  sectionLabel(slide, ctx, "governance");
  title(slide, ctx, "${safeText(s.title)}", "${safeText(s.subtitle)}");
  ${s.compare.map((c, i) => `card(slide, ctx, ${120 + i * 550}, 322, 460, 210, "${safeText(c[0])}", "${safeText(c[1])}\\n\\nDecision verbs: ${safeText(c[2])}", "${c[3]}");`).join("\n  ")}
`;
  }
  return `
  sectionLabel(slide, ctx, "talk track");
  title(slide, ctx, "${safeText(s.title)}", "${safeText(s.subtitle || "")}");
  bullets(slide, ctx, ${JSON.stringify(s.bullets || [])}, 112, 296, 900, { size: ${(s.bullets || []).length > 6 ? 20 : 23}, gap: ${(s.bullets || []).length > 6 ? 42 : 49} });
  ${s.closing ? `paragraph(slide, ctx, "${safeText(s.closing)}", 112, 602, 900, 22, C.cyan);` : ""}
`;
}

await fs.mkdir(slidesDir, { recursive: true });
await fs.writeFile(path.join(slidesDir, "deck-utils.mjs"), utils, "utf8");
await Promise.all(slides.map((slide, index) => fs.writeFile(
  path.join(slidesDir, `slide-${String(index + 1).padStart(2, "0")}.mjs`),
  moduleFor(slide, index),
  "utf8",
)));
await fs.writeFile(
  path.join(workspace, "profile-plan.txt"),
  [
    "task mode: create",
    "primary deck-profile: engineering-platform",
    "required proof objects: SDLC resource model, runtime loop, UI screenshot, policy/approval surfaces, typed capability examples",
    "source requirements: local pharness and pharness-ui artifacts already reviewed",
    "QA gates: editable PPTX export, preview render, contact sheet check",
  ].join("\n") + "\n",
  "utf8",
);

console.log(`Wrote ${slides.length} slide modules to ${slidesDir}`);
