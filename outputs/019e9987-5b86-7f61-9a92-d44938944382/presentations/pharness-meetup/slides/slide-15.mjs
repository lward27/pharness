import { addBase, bigNumber, bullets, card, C, flowArrow, node, paragraph, pill, screenshotPath, sectionLabel, title } from "./deck-utils.mjs";

export async function slide15(presentation, ctx) {
  const slide = addBase(presentation, ctx);
  
  sectionLabel(slide, ctx, "talk track");
  title(slide, ctx, "Typed capabilities", "Production delivery should not become a pile of shell wrappers.");
  bullets(slide, ctx, ["kubernetes_get","argo_get_app","tekton_get_pipeline_runs","tekton_get_task_runs","tekton_analyze_pipeline_run","prometheus_query / prometheus_inventory","loki_log_summary"], 112, 296, 900, { size: 20, gap: 42 });
  

  return slide;
}
