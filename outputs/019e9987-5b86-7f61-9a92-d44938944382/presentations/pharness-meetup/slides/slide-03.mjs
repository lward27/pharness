import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide03(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "story");
  title(slide, ctx, "Why build this?", "Agent tools blur trust domains that should stay separate.");
  card(slide, ctx, 78, 322, 320, 190, "Chat", "Useful for exploration, weak as an audit boundary.", "#A985FF");
  card(slide, ctx, 442, 322, 320, 190, "Local automation", "Fast feedback against a repo, shell, git, and tests.", "#32D7DF");
  card(slide, ctx, 806, 322, 320, 190, "Delivery operations", "Needs policy, evidence, approvals, and rollback.", "#F2BC2E");

  return slide;
}
