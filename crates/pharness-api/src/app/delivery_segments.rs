use crate::dto::{
    DeliverySegmentResourceResponse, DeliverySegmentResponse, SdlcFlowResponse, WorkspaceResponse,
};

pub(in crate::app) fn work_item_delivery_segments(
    flow: &Option<SdlcFlowResponse>,
    workspace: Option<&WorkspaceResponse>,
) -> Vec<DeliverySegmentResponse> {
    let Some(flow) = flow else {
        return vec![delivery_segment(
            "source",
            "Source",
            "active",
            "Awaiting WorkPlan declaration before source work can begin.",
            Vec::new(),
        )];
    };

    sdlc_flow_delivery_segments(flow, workspace)
}

pub(in crate::app) fn sdlc_flow_delivery_segments(
    flow: &SdlcFlowResponse,
    workspace: Option<&WorkspaceResponse>,
) -> Vec<DeliverySegmentResponse> {
    let change_set = flow.change_set.as_ref();
    let source_blocked = change_set.is_some_and(|item| is_blocked_delivery_status(&item.status));
    let source_complete = workspace.is_some_and(|item| item.status == "captured")
        && change_set.is_some_and(|item| is_complete_delivery_status(&item.status));
    let mut source_resources = vec![delivery_resource(
        "work_plan",
        &flow.work_plan.id,
        "WorkPlan",
        Some(flow.work_plan.summary.clone()),
    )];
    if let Some(workspace) = workspace {
        source_resources.push(delivery_resource(
            "workspace",
            &workspace.id,
            "Workspace",
            Some(format!(
                "{} @ {}",
                workspace.source_repo,
                workspace
                    .resolved_commit
                    .as_deref()
                    .unwrap_or(workspace.source_ref.as_str())
            )),
        ));
    }
    if let Some(change_set) = change_set {
        source_resources.push(delivery_resource(
            "change_set",
            &change_set.id,
            "ChangeSet",
            Some(change_set.summary.clone()),
        ));
    }
    if let Some(artifact) = flow
        .git_delivery
        .as_ref()
        .and_then(|delivery| delivery.latest_result.as_ref())
    {
        source_resources.push(delivery_resource(
            "artifact",
            &artifact.id,
            "Source PR evidence",
            Some(artifact.label.clone()),
        ));
    }
    let source = delivery_segment(
        "source",
        "Source",
        if source_blocked {
            "blocked"
        } else if source_complete {
            "complete"
        } else {
            "active"
        },
        if source_blocked {
            change_set
                .and_then(|item| item.status_reason.as_deref())
                .unwrap_or("Source change requires a new reviewed revision.")
        } else if source_complete {
            "Immutable source change and workspace provenance are recorded."
        } else {
            "Awaiting a captured workspace and reviewed source ChangeSet."
        },
        source_resources,
    );

    let pipeline = flow.pipeline_intent.as_ref();
    let build = next_delivery_segment(
        "build",
        "Build",
        source_complete,
        pipeline.map(|item| item.status.as_str()),
        pipeline.map(|item| {
            item.status_reason
                .as_deref()
                .unwrap_or(item.summary.as_str())
        }),
        pipeline.map(|item| {
            vec![delivery_resource(
                "pipeline_intent",
                &item.id,
                "PipelineIntent",
                Some(item.summary.clone()),
            )]
        }),
        [
            "Awaiting immutable source delivery before build can start.",
            "Awaiting a PipelineIntent for the immutable source revision.",
        ],
    );
    let build_complete = build.status == "complete";

    let gitops = flow.gitops_change_set.as_ref();
    let gitops_segment = next_delivery_segment(
        "gitops",
        "GitOps",
        build_complete,
        gitops.map(|item| item.status.as_str()),
        gitops.map(|item| {
            item.status_reason
                .as_deref()
                .unwrap_or(item.summary.as_str())
        }),
        gitops.map(|item| {
            vec![delivery_resource(
                "gitops_change_set",
                &item.id,
                "GitOps ChangeSet",
                Some(item.summary.clone()),
            )]
        }),
        [
            "Awaiting verified build output before GitOps delivery can start.",
            "Awaiting a GitOps ChangeSet for the verified build output.",
        ],
    );
    let gitops_complete = gitops_segment.status == "complete";

    let deploy = flow.deployment_intent.as_ref();
    let deploy_segment = next_delivery_segment(
        "deploy",
        "Deploy",
        gitops_complete,
        deploy.map(|item| item.status.as_str()),
        deploy.map(|item| {
            item.status_reason
                .as_deref()
                .unwrap_or(item.summary.as_str())
        }),
        deploy.map(|item| {
            vec![delivery_resource(
                "deployment_intent",
                &item.id,
                "DeploymentIntent",
                Some(item.summary.clone()),
            )]
        }),
        [
            "Awaiting GitOps merge provenance before deployment can start.",
            "Awaiting a DeploymentIntent for the immutable GitOps revision.",
        ],
    );
    let deploy_complete = deploy_segment.status == "complete";

    let release = flow.release.as_ref();
    let mut verify_resources = release
        .map(|item| {
            vec![delivery_resource(
                "release",
                &item.id,
                "Release",
                Some(item.summary.clone()),
            )]
        })
        .unwrap_or_default();
    if let Some(evidence) = &flow.registry_evidence {
        verify_resources.push(delivery_resource(
            "registry_evidence",
            &evidence.id,
            "Registry evidence",
            Some(evidence.summary.clone()),
        ));
    }
    let verify = next_delivery_segment(
        "verify",
        "Verify",
        deploy_complete,
        release.map(|item| item.status.as_str()),
        release.map(|item| {
            item.status_reason
                .as_deref()
                .unwrap_or(item.summary.as_str())
        }),
        Some(verify_resources),
        [
            "Awaiting completed deployment before verification can start.",
            "Awaiting a Release and post-deploy verification evidence.",
        ],
    );

    vec![source, build, gitops_segment, deploy_segment, verify]
}

pub(in crate::app) fn delivery_resource(
    kind: &str,
    id: &str,
    label: &str,
    summary: Option<String>,
) -> DeliverySegmentResourceResponse {
    DeliverySegmentResourceResponse {
        kind: kind.to_string(),
        id: id.to_string(),
        label: label.to_string(),
        summary,
    }
}

pub(in crate::app) fn delivery_segment(
    key: &str,
    label: &str,
    status: &str,
    summary: &str,
    resources: Vec<DeliverySegmentResourceResponse>,
) -> DeliverySegmentResponse {
    DeliverySegmentResponse {
        key: key.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        summary: summary.to_string(),
        stopping_reason: (status != "complete").then(|| summary.to_string()),
        resources,
    }
}

pub(in crate::app) fn next_delivery_segment(
    key: &str,
    label: &str,
    previous_complete: bool,
    resource_status: Option<&str>,
    resource_summary: Option<&str>,
    resources: Option<Vec<DeliverySegmentResourceResponse>>,
    summaries: [&str; 2],
) -> DeliverySegmentResponse {
    if !previous_complete {
        return delivery_segment(key, label, "unreached", summaries[0], Vec::new());
    }
    let Some(status) = resource_status else {
        return delivery_segment(key, label, "active", summaries[1], Vec::new());
    };
    let mapped_status = if is_complete_delivery_status(status) {
        "complete"
    } else if is_blocked_delivery_status(status) {
        "blocked"
    } else {
        "active"
    };
    delivery_segment(
        key,
        label,
        mapped_status,
        resource_summary.unwrap_or("Controller state recorded."),
        resources.unwrap_or_default(),
    )
}

pub(in crate::app) fn is_complete_delivery_status(status: &str) -> bool {
    matches!(
        status,
        "captured"
            | "completed"
            | "verified"
            | "merged"
            | "succeeded"
            | "satisfied"
            | "ready"
            | "applied"
    )
}

pub(in crate::app) fn is_blocked_delivery_status(status: &str) -> bool {
    matches!(status, "blocked" | "failed" | "rejected" | "stale")
}
