import { describe, expect, it } from "vitest";
import { buildDeliveryWorkspace } from "./deliveryWorkspace";

const sourceSha = "a".repeat(40);
const sourceMergeSha = "b".repeat(40);
const gitopsMergeSha = "c".repeat(40);
const desiredDigest = `sha256:${"d".repeat(64)}`;
const baselineDigest = `sha256:${"e".repeat(64)}`;

function item() {
  return {
    id: "witem_delivery",
    source_repo: "https://github.com/lward27/yfinance_wrapper.git",
    source_commit: sourceSha,
    target_environment: "production",
    target_namespace: "apps-prod",
    argo_application: "yfinance-wrapper",
    workload_name: "yfinance-wrapper",
    rollback_owner: "lucas",
  };
}

function artifact(id: string, kind: string, content_json: any) {
  return { id, kind, label: id, content_json };
}

function baseFlow() {
  return {
    delivery_segments: [
      { key: "source", status: "complete" },
      { key: "build", status: "complete" },
      { key: "gitops", status: "active" },
      { key: "deploy", status: "unreached" },
      { key: "verify", status: "unreached" },
    ],
    delivery_configuration: {
      desired_digest: desiredDigest,
      current_digest: baselineDigest,
      baseline_digest: baselineDigest,
      rollback_owner: "lucas",
      rollback_status: "prepared",
      production_window_expires_at: "2000",
      gitops: { repository: "https://github.com/lward27/lucas_engineering.git", kustomization_path: "charts/yfinance-wrapper/kustomization.yaml" },
      target: { environment: "production", namespace: "apps-prod", workload_name: "yfinance-wrapper", argo_application: "yfinance-wrapper" },
    },
    sdlc_flow: {
      change_set: { id: "cset_delivery", status: "approved" },
      git_delivery: {
        latest_result: artifact("art_source_pr", "git_delivery_result", { details: { pull_request_number: 2, pull_request_url: "https://github.com/lward27/yfinance_wrapper/pull/2" } }),
        latest_observation: artifact("art_source_observe", "git_delivery_pr_observation", { merged: true, pull_request_number: 2, merge_commit_sha: sourceMergeSha }),
        latest_merge: artifact("art_source_merge", "git_delivery_merge", { pull_request_number: 2, merge_commit_sha: sourceMergeSha }),
      },
      pipeline_intent: {
        id: "pint_delivery",
        status: "approved",
        execution_state: { state: "pipeline_run_succeeded", pipeline_run_namespace: "tekton-pipelines", pipeline_run_name: "pharness-yfinance-build-2" },
        execution_evidence: { status: "succeeded" },
        intent_json: {
          execution: { pipeline_ref: "pharness-yfinance-build" },
          build_output: { status: "verified", source_commit: sourceMergeSha, image_digest: desiredDigest, artifact_id: "art_build" },
          execution_history: [{ status: "failed" }],
        },
      },
      gitops_change_set: { id: "gcset_delivery", status: "approved", gitops_repo: "https://github.com/lward27/lucas_engineering.git", kustomization_path: "charts/yfinance-wrapper/kustomization.yaml" },
      gitops_delivery: {
        latest_result: artifact("art_gitops_pr", "gitops_delivery_result", { details: { pull_request_number: 26, pull_request_url: "https://github.com/lward27/lucas_engineering/pull/26" } }),
        latest_observation: artifact("art_gitops_observe", "gitops_delivery_pr_observation", { merged: false, pull_request_number: 26, pull_request_state: "open" }),
      },
      deployment_intent: { id: "dint_delivery", status: "approved", argo_application: "yfinance-wrapper" },
    },
  };
}

describe("buildDeliveryWorkspace", () => {
  it("places manual merge waits and Tekton output in their owning stages", () => {
    const model = buildDeliveryWorkspace(baseFlow(), item(), 3_000);

    expect(model.stages.map((stage) => stage.status)).toEqual(["complete", "complete", "waiting", "unreached", "unreached"]);
    expect(model.stages[0].checkpoint).toBe("PR #2 merged manually");
    expect(model.stages[1].facts).toContainEqual(expect.objectContaining({ label: "PipelineRun", value: "succeeded · 1 earlier failure" }));
    expect(model.stages[2].checkpoint).toBe("PR #26 awaits manual merge");
    expect(model.stages[2].evidence[0]).toMatchObject({ status: "waiting" });
  });

  it("keeps the production window, desired digest, and rollback baseline visible", () => {
    const model = buildDeliveryWorkspace(baseFlow(), item(), 3_000);

    expect(model.guardrails).toMatchObject({
      target: "production · apps-prod · yfinance-wrapper",
      productionWindowStatus: "expired",
      desiredDigest,
      baselineDigest,
      rollbackOwner: "lucas",
      rollbackStatus: "prepared",
    });
    expect(model.guardrails.digestEqualityStatus).toBe("unverified");
  });

  it("derives completed deployment and verification from durable Argo and contract evidence", () => {
    const flow = baseFlow();
    flow.delivery_segments = flow.delivery_segments.map((stage) => ({ ...stage, status: "unreached" }));
    flow.sdlc_flow.gitops_delivery.latest_observation = artifact("art_gitops_observe", "gitops_delivery_pr_observation", { merged: true, pull_request_number: 26, merge_commit_sha: gitopsMergeSha });
    (flow.sdlc_flow.gitops_delivery as any).latest_merge = artifact("art_gitops_merge", "gitops_delivery_merge", { pull_request_number: 26, merge_commit_sha: gitopsMergeSha });
    (flow as any).audit_events = [];
    (flow.sdlc_flow as any).release = {
      id: "rel_delivery",
      status: "completed",
      image_digest: desiredDigest,
      release_json: {
        post_sync_verification: {
          status: "verified",
          deployment_contract_id: "dcontract-yfinance",
          checks: [
            { code: "completed_argo_sync", passed: true, summary: "Completed sync result artifact is current" },
            { code: "running_image_digest", passed: true, summary: "Running digest equals desired digest" },
            { code: "service_healthz", passed: true, summary: "Health check passed" },
          ],
        },
      },
    };

    const model = buildDeliveryWorkspace(flow, item(), 3_000);

    expect(model.stages.map((stage) => stage.status)).toEqual(["complete", "complete", "complete", "complete", "complete"]);
    expect(model.stages[3].checkpoint).toBe("Argo Synced · Succeeded");
    expect(model.stages[4].checkpoint).toBe("3/3 required checks passed");
    expect(model.guardrails.digestEqualityStatus).toBe("verified");
    expect(model.consistencyIssues).toHaveLength(5);
  });
});
