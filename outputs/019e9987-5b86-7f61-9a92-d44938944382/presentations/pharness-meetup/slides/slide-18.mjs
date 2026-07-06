import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide18(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "story");
  title(slide, ctx, "Technical challenges", "The interesting parts are mostly boring operational details.");
  card(slide, ctx, 78, 312, 250, 210, "Model protocols", "Tool support, malformed arguments, streaming assembly, and one-action discipline.", "#A985FF");
  card(slide, ctx, 362, 312, 250, 210, "Approval resume", "Persist and resume the exact reviewed action, not a later model interpretation.", "#F2BC2E");
  card(slide, ctx, 646, 312, 250, 210, "Cluster output", "Parse before redaction, compact the payload, and deny secret-shaped reads early.", "#32D7DF");
  card(slide, ctx, 930, 312, 250, 210, "Drift and aliases", "Registry host aliases should reduce false drift without hiding real mismatches.", "#45A8FF");

  return slide;
}
