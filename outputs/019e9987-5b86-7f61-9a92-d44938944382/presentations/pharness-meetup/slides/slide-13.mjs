import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide13(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "talk track");
  title(slide, ctx, "Policy is a runtime contract", "A resumed run should not inherit config drift by accident.");
  bullets(slide, ctx, ["read-only actions allowed","file writes ask by default","network and destructive shell commands ask","privileged and secret-shaped actions deny","typed cluster reads allow unless secret-shaped","grants cannot override denials"], 112, 296, 900, { size: 23, gap: 49 });
  

  return slide;
}
