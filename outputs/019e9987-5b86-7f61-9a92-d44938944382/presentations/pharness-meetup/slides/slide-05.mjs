import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide05(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "talk track");
  title(slide, ctx, "What PHarness is", "A local-first Rust agent harness designed to grow into a Kubernetes-native delivery runtime.");
  bullets(slide, ctx, ["Fireworks-first model provider","one-action-per-turn agent loop","file, shell, git, patch, and typed read capabilities","conservative safety policy and approval resume","durable SQLite-backed sessions, events, artifacts, diffs, observations, gates, grants, and audit"], 112, 296, 900, { size: 23, gap: 49 });
  

  return slide;
}
