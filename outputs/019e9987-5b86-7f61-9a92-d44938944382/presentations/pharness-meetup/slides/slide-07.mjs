import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide07(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "talk track");
  title(slide, ctx, "Current state", "The control-plane slice exists now.");
  bullets(slide, ctx, ["run API, worker execution, and SSE event streaming","Fireworks native tool calling as default","approval queues and exact action resume","typed read-only Kubernetes, Argo, Tekton, Prometheus, and Loki capabilities","durable observations, incidents, remediation plans, work plans, change sets, approval gates, permission grants, and audit events"], 112, 296, 900, { size: 23, gap: 49 });
  

  return slide;
}
