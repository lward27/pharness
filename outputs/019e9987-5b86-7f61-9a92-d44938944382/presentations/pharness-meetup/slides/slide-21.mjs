import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide21(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "talk track");
  title(slide, ctx, "The risk", "The obvious failure mode is overbuilding.");
  bullets(slide, ctx, ["keep the policy model readable","make every action explainable","add cluster capabilities one at a time","keep V1 locally useful","delete abstractions that do not make production workflows safer or clearer"], 112, 296, 900, { size: 23, gap: 49 });
  

  return slide;
}
