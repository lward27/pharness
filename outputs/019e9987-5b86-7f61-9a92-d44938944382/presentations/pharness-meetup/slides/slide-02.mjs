import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide02(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "talk track");
  title(slide, ctx, "The claim", "The hard part is not making a model call a tool.");
  bullets(slide, ctx, ["preserve operator intent","enforce policy boundaries","make actions replayable","attach durable evidence","keep rollback context visible"], 112, 296, 900, { size: 23, gap: 49 });
  

  return slide;
}
