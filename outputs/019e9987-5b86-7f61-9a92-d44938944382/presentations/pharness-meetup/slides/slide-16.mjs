import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide16(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "talk track");
  title(slide, ctx, "Tekton analysis example", "Turn scattered cluster facts into structured SDLC evidence.");
  bullets(slide, ctx, ["PipelineRun status, reason, and timing","TaskRun status counts","repo URL, commit SHA, image reference, and digest","deployment target and rollout health","Argo sync and health","registry-aware image alignment"], 112, 296, 900, { size: 23, gap: 49 });
  

  return slide;
}
