import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide17(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "talk track");
  title(slide, ctx, "Trusted autonomy", "The goal is bounded autonomy, not fewer prompts by default.");
  bullets(slide, ctx, ["scoped to environment, repo, branch, and namespace","scoped to WorkPlan or ChangeSet","scoped to capability kind and action","time-bounded and auditable","invalidated when material plans change","today: local file writes only"], 112, 296, 900, { size: 23, gap: 49 });
  

  return slide;
}
