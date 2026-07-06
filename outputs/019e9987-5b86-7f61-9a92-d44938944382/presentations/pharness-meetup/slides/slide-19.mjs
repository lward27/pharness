import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide19(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "flow");
  title(slide, ctx, "What I would demo", "Show that the runtime knows what happened.");
  bigNumber(slide, ctx, "1", "run read-only task", 90, 318, C.cyan);
  bigNumber(slide, ctx, "2", "show event stream", 365, 318, C.amber);
  bigNumber(slide, ctx, "3", "show policy decision", 640, 318, C.cyan);
  bigNumber(slide, ctx, "4", "trigger write approval", 915, 318, C.amber);
  bigNumber(slide, ctx, "5", "inspect preview diff", 90, 478, C.cyan);
  bigNumber(slide, ctx, "6", "approve and resume", 365, 478, C.amber);
  bigNumber(slide, ctx, "7", "show final diff and audit trail", 640, 478, C.cyan);
  bigNumber(slide, ctx, "8", "show typed cluster read", 915, 478, C.amber);

  return slide;
}
