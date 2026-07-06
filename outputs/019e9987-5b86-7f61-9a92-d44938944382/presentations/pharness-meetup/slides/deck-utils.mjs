
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
