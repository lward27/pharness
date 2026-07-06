import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide06(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "talk track");
  title(slide, ctx, "What it is not", "The non-goals are as important as the goals.");
  bullets(slide, ctx, ["not a plugin marketplace","not an MCP-first runtime","not a chat UI with tools bolted on","not a hidden permission bypass","not an autonomous production deployer"], 112, 296, 900, { size: 23, gap: 49 });
  

  return slide;
}
