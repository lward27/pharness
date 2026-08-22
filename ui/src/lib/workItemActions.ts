export type WorkItemRailAction = {
  id: string;
  status: string;
  lifecycle_stage?: string;
};

export type WorkItemReconcilePreview = {
  action?: string;
  can_apply?: boolean;
};

export function selectPrimaryWorkItemAction<T extends WorkItemRailAction>(
  actionRail: T[] | undefined,
  preview: WorkItemReconcilePreview | undefined,
  workItemStatus?: string,
) {
  const actions = actionRail ?? [];
  if (workItemStatus === "completed" || workItemStatus === "cancelled") {
    return undefined;
  }

  const forwardActions = actions.filter((entry) => entry.lifecycle_stage !== "rollback");
  const readyPreview = preview?.can_apply
    ? forwardActions.find((entry) => entry.id === preview.action && entry.status === "ready")
    : undefined;

  return readyPreview
    ?? forwardActions.find((entry) => entry.status === "ready")
    ?? forwardActions.find((entry) => entry.id === preview?.action)
    ?? forwardActions[0];
}

export function selectRecoveryActions<T extends WorkItemRailAction>(actionRail: T[] | undefined) {
  return (actionRail ?? []).filter((entry) => entry.lifecycle_stage === "rollback");
}
