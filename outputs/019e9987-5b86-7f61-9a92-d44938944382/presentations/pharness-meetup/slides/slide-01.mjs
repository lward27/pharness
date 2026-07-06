import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide01(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  ctx.addShape(slide, { x: 78, y: 142, w: 104, h: 104, fill: "#0D2C33", line: ctx.line(C.cyan, 2) });
  ctx.addText(slide, { text: "P", x: 104, y: 158, w: 54, h: 66, fontSize: 56, bold: true, color: C.cyan, align: "center" });
  ctx.addText(slide, { text: "PHarness", x: 208, y: 138, w: 820, h: 84, fontSize: 62, bold: true, color: C.text, typeface: ctx.fonts.title });
  ctx.addText(slide, { text: "An agent harness as an SDLC control plane", x: 214, y: 232, w: 850, h: 44, fontSize: 26, color: C.soft });
  ctx.addText(slide, { text: "Local-first today. Cluster-native by design.", x: 218, y: 304, w: 760, h: 32, fontSize: 21, color: C.cyan });
  card(slide, ctx, 78, 430, 1080, 118, "Meetup framing", "A casual technical story about why PHarness exists, what has been built, and where the hard engineering problems actually are.", C.cyan);

  return slide;
}
