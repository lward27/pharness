import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide14(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "governance");
  title(slide, ctx, "Two approval surfaces", "Tool approvals and governance gates are different resources.");
  card(slide, ctx, 120, 322, 460, 210, "Tool approval", "May this paused run execute this exact action?\n\nDecision verbs: approve / deny", "#F2BC2E");
  card(slide, ctx, 670, 322, 460, 210, "Approval gate", "Has this governance checkpoint been satisfied?\n\nDecision verbs: satisfy / waive / reject", "#32D7DF");

  return slide;
}
