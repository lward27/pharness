import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide20(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "story");
  title(slide, ctx, "Roadmap", "Keep V1 useful while moving toward governed cluster execution.");
  card(slide, ctx, 78, 322, 320, 190, "Near term", "WorkPlan and ChangeSet readiness, scoped grant negative smokes, stronger evidence retrieval, more dogfooding.", "#32D7DF");
  card(slide, ctx, 442, 322, 320, 190, "V2", "Kubernetes worker pods, per-run workspace sandboxes, API/UI services, optional Postgres.", "#F2BC2E");
  card(slide, ctx, 806, 322, 320, 190, "V3", "Tekton intents, Argo deployment intents, database operator capabilities, LGTM loops, CRD-backed resources.", "#36D86B");

  return slide;
}
