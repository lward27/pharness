import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide22(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "talk track");
  title(slide, ctx, "Takeaways", "Useful agent autonomy needs a control plane.");
  bullets(slide, ctx, ["typed actions","scoped trust","durable evidence","explicit approvals","replayable runs","cluster-native resource vocabulary"], 112, 296, 900, { size: 23, gap: 49 });
  paragraph(slide, ctx, "The model is the planner and actor. PHarness is the system that keeps it inside a governed operational story.", 112, 602, 900, 22, C.cyan);

  return slide;
}
