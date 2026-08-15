export function resourceLabel(resource: any) {
  return [resource?.resource_kind, resource?.resource_name].filter(Boolean).join("/")
    || resource?.resource_namespace
    || "not scoped";
}

export function approvalActionName(approval: any) {
  return approval?.action?.action ?? approval?.kind ?? "tool approval";
}

export function approvalPreviewPath(approval: any) {
  return approval?.preview?.path ?? approval?.action?.path ?? "no preview path";
}

export function approvalPreviewDiff(approval: any) {
  return approval?.preview?.diff
    ?? approval?.action?.diff
    ?? approval?.summary
    ?? "No diff preview is available for this approval.";
}
