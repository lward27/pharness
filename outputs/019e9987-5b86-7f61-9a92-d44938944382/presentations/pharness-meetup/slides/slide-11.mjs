import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide11(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "runtime");
  title(slide, ctx, "Runtime architecture", "The model proposes. The runtime decides. The store remembers.");
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

  return slide;
}
