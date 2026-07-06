import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide09(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "product");
  title(slide, ctx, "What the operator sees", "Flow, Queue, Approvals, and Approval Gates are lenses over the same resources.");
  await ctx.addImage(slide, { path: screenshotPath(), x: 84, y: 260, w: 1090, h: 390, fit: "contain", alt: "PHarness prototype flow UI" });

  return slide;
}
