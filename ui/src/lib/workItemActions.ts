export type WorkItemRailAction = {
  id: string;
  status: string;
};

export type WorkItemReconcilePreview = {
  action?: string;
  can_apply?: boolean;
};

export function selectPrimaryWorkItemAction<T extends WorkItemRailAction>(
  actionRail: T[] | undefined,
  preview: WorkItemReconcilePreview | undefined,
) {
  const actions = actionRail ?? [];
  const readyPreview = preview?.can_apply
    ? actions.find((entry) => entry.id === preview.action && entry.status === "ready")
    : undefined;

  return readyPreview
    ?? actions.find((entry) => entry.status === "ready")
    ?? actions.find((entry) => entry.id === preview?.action)
    ?? actions[0];
}
