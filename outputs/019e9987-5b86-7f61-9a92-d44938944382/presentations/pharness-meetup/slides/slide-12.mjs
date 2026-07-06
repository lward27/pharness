import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide12(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "flow");
  title(slide, ctx, "One action per turn", "A narrow loop makes policy, replay, and approval resume tractable.");
  bigNumber(slide, ctx, "1", "model request", 90, 318, C.cyan);
  bigNumber(slide, ctx, "2", "one proposed AgentAction", 365, 318, C.amber);
  bigNumber(slide, ctx, "3", "shape validation", 640, 318, C.cyan);
  bigNumber(slide, ctx, "4", "policy evaluation", 915, 318, C.amber);
  bigNumber(slide, ctx, "5", "execute / pause / deny", 90, 478, C.cyan);
  bigNumber(slide, ctx, "6", "append result", 365, 478, C.amber);

  return slide;
}
