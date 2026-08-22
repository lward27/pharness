import { describe, expect, it } from "vitest";
import { buildLifecycleReview, findCorrectiveAction, lifecycleReviewKind, reviewAlternatives, type LifecycleAction } from "./lifecycleReview";

const item = {
  id: "witem_review",
  status: "awaiting_approval",
  source_repo: "https://github.com/lward27/yfinance_wrapper.git",
  source_commit: "a".repeat(40),
  target_environment: "production",
  target_namespace: "apps-prod",
  argo_application: "yfinance-wrapper",
  pipeline_contract_id: "pipeline-contract",
  deployment_contract_id: "deployment-contract",
  environment_profile_id: "python-3.11",
  environment_preparation_status: "succeeded",
  attempt_count: 1,
  max_attempts: 3,
  acceptance_criteria: ["python -m unittest discover -s tests -v"],
  repository_contract: { writable_paths: ["src/**", "tests/**"], agent_network: "denied" },
  rollback_owner: "lucas",
};

const flow = {
  workspaces: [{ id: "ws_attempt", source_repo: item.source_repo, resolved_commit: item.source_commit, branch: "pharness/attempt-2", status: "declared" }],
  delivery_configuration: {
    desired_digest: `sha256:${"d".repeat(64)}`,
    baseline_digest: `sha256:${"b".repeat(64)}`,
    rollback_owner: "lucas",
    target: { environment: "production", namespace: "apps-prod", argo_application: "yfinance-wrapper" },
    gitops: { repository: "https://github.com/lward27/lucas_engineering.git", kustomization_path: "charts/yfinance-wrapper/kustomization.yaml", image_name: "registry.lucas.engineering/yfinance_wrapper", desired_revision: "c".repeat(40) },
  },
  sdlc_flow: {
    work_plan: { id: "wplan_review", status: "proposed", summary: "Bounded validation plan", risk_level: "high", revision: 2, work_plan_json: { acceptance_criteria: item.acceptance_criteria, source_repository: { repo: item.source_repo, ref: "main" }, target: { environment: "production", namespace: "apps-prod", argo_application: "yfinance-wrapper" }, approval_gates: [{ kind: "source_mutation", required_before: "source PR" }] } },
    change_set: { id: "cset_review", status: "proposed", summary: "Four reviewed files", risk_level: "high", revision: 1, material_hash: "sha256:material", run_id: "run_review", change_set_json: { source: { base_commit: item.source_commit, branch: "pharness/attempt-2" }, evidence: { changed_paths: ["src/validation.py", "tests/test_validation.py"], git_diff_artifact_id: "art_diff" }, verification: { test_event_count: 2 } } },
    gitops_change_set: { id: "gcset_review", status: "proposed", summary: "Digest-only update", gitops_repo: "https://github.com/lward27/lucas_engineering.git", revision: 1, material_hash: "sha256:gitops" },
    pipeline_intent: { id: "pint_review", status: "proposed", summary: "Build immutable source", intent_json: { execution: { namespace: "tekton-pipelines", pipeline_ref: "pharness-yfinance-build" }, pipeline_contract: { id: "pipeline-contract" }, source_provenance: { merge_commit_sha: "e".repeat(40) }, execution_attempt: 2, build_output: { status: "verified", image_digest: `sha256:${"d".repeat(64)}` } } },
    deployment_intent: { id: "dint_review", status: "proposed", summary: "Deploy exact digest", target_environment: "production", target_namespace: "apps-prod", argo_application: "yfinance-wrapper", intent_json: { pipeline_evidence: { status: "satisfied", artifact_id: "art_pipeline" } } },
    release: { id: "rel_review", status: "proposed", summary: "Verify protected release", target_environment: "production", target_namespace: "apps-prod", argo_application: "yfinance-wrapper", commit_sha: "f".repeat(40), image_digest: `sha256:${"d".repeat(64)}`, rollback_ref: `image@sha256:${"b".repeat(64)}`, release_kind: "gitops_release", release_json: { post_sync_verification: { status: "verified", checks: [{ passed: true, summary: "Deployment ready" }] } } },
    approval_gates: [{ id: "agate_review", status: "pending", gate_kind: "source_mutation", gate_order: 1, summary: "Approve source delivery", resource_kind: "application", resource_name: "yfinance-wrapper" }],
  },
};

function action(id: string, resource: string): LifecycleAction {
  return { id, resource, status: "ready", lifecycle_stage: "source", effect_class: "approval_boundary", blockers: [], approval_requirements: [`${id}_review`], external_effect_summary: `Execute exact ${id} for ${resource}.`, state_hash: `state-${id}` };
}

describe("lifecycle review evidence", () => {
  it.each([
    ["approve_work_plan", "wplan_review", "work_plan", "Plan scope", "Acceptance: python -m unittest discover -s tests -v"],
    ["authorize_workspace_and_start", "wplan_review", "workspace", "Attempt authorization", "Writable: src/**"],
    ["approve_change_set", "cset_review", "change_set", "Proposed source change", "Changed: src/validation.py"],
    ["approve_gitops_change_set", "gcset_review", "gitops_change_set", "Digest-only GitOps change", null],
    ["approve_pipeline_intent", "pint_review", "pipeline_intent", "Build and execution evidence", null],
    ["approve_deployment_intent", "dint_review", "deployment_intent", "Protected deployment target", "Pipeline evidence: satisfied"],
    ["approve_release", "rel_review", "release", "Release evidence", "Passed: Deployment ready"],
    ["approve_rollback", "rollback_review", "rollback_intent", "Recovery binding", null],
  ])("builds an API-backed %s review", (id, resource, expectedKind, groupTitle, evidenceLine) => {
    const selected = action(id, resource);
    const model = buildLifecycleReview(selected, { item, flow, preview: {}, rollbackIntent: { content: { rollback_intent_id: "rollback_review", status: "prepared", baseline: { image_digest: `sha256:${"b".repeat(64)}` } } } });
    expect(lifecycleReviewKind(selected)).toBe(expectedKind);
    expect(model.groups.some((group) => group.title === groupTitle)).toBe(true);
    expect(model.stateHash).toBe(`state-${id}`);
    if (evidenceLine) expect(model.groups.flatMap((group) => group.evidence ?? [])).toContain(evidenceLine);
  });

  it("pairs approve and reject decisions only for the same exact resource", () => {
    const approve = action("approve_work_plan", "wplan_review");
    const reject = action("reject_work_plan", "wplan_review");
    const unrelated = action("reject_work_plan", "wplan_other");
    expect(reviewAlternatives(approve, [unrelated, reject, approve])).toEqual([approve, reject]);
  });

  it("links blockers only to a ready corrective lifecycle action", () => {
    const replan = action("replan_work_item", item.id);
    const blockedPlan = { ...action("approve_work_plan", "wplan_review"), status: "blocked" };
    expect(findCorrectiveAction({ code: "requires_replan", summary: "A replan is required." }, [blockedPlan, replan])).toBe(replan);
    expect(findCorrectiveAction({ code: "work_plan_review", summary: "WorkPlan review required." }, [blockedPlan, replan])).toBeUndefined();
  });
});
