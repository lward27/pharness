import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide10(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "resource model");
  title(slide, ctx, "The SDLC shape", "Future CRDs, current database resources.");
  const labels = ["WorkItem", "WorkPlan", "ChangeSet", "PipelineIntent", "PipelineRunAnalysis", "DeploymentIntent", "Release"];
  labels.forEach((label, i) => { node(slide, ctx, label, 66 + i * 166, 300, 132, 86, i < 3 ? C.cyan : i < 5 ? C.amber : C.green); if (i < labels.length - 1) flowArrow(slide, ctx, 200 + i * 166, 330, 30); });
  node(slide, ctx, "Observation", 142, 500, 150, 70, C.blue, "read-only fact");
  flowArrow(slide, ctx, 294, 525, 50);
  node(slide, ctx, "Incident", 348, 500, 150, 70, C.red, "candidate");
  flowArrow(slide, ctx, 500, 525, 50);
  node(slide, ctx, "RemediationPlan", 554, 500, 170, 70, C.purple, "draft");
  flowArrow(slide, ctx, 728, 525, 56);
  node(slide, ctx, "WorkPlan", 790, 500, 150, 70, C.cyan, "handoff");
  pill(slide, ctx, "PermissionGrant bounds autonomy", 166, 626, 310, C.green);
  pill(slide, ctx, "ApprovalGate gates risky transitions", 502, 626, 330, C.amber);
  pill(slide, ctx, "AuditEvent records decisions", 858, 626, 280, C.blue);

  return slide;
}
