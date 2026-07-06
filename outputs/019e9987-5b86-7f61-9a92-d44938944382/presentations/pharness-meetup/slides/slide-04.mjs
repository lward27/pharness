import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide04(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "talk track");
  title(slide, ctx, "Philosophy", "Small runtime, explicit boundaries, durable state.");
  bullets(slide, ctx, ["small surface area beats plugin sprawl","typed actions beat opaque shell commands","policy belongs in the runtime, not the prompt","autonomy should be bounded by explicit envelopes","chat is secondary to runs, evidence, and audit"], 112, 296, 900, { size: 23, gap: 49 });
  

  return slide;
}
