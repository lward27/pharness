import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide08(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "architecture");
  title(slide, ctx, "Local-first, cluster-native", "V1 runs locally, but the nouns are shaped for the cluster.");
  pill(slide, ctx, "ExecutionTarget", 96, 322, 270, C.cyan);
  pill(slide, ctx, "ResourceRef", 426, 322, 270, C.amber);
  pill(slide, ctx, "ArtifactRef", 756, 322, 270, C.cyan);
  pill(slide, ctx, "CapabilityKind", 96, 390, 270, C.amber);
  pill(slide, ctx, "RunScope", 426, 390, 270, C.cyan);
  paragraph(slide, ctx, "The point is to avoid painting V1 into a corner while still keeping the first useful implementation small.", 102, 500, 920, 24, C.soft);

  return slide;
}
