import type { PermissionStatus } from "./api";

export type PermissionAction = "request" | "settings";

/** The OS prompt while it can still be shown, the settings page in every case. */
export function permissionActions(status: PermissionStatus): PermissionAction[] {
  if (status.state === "granted") return [];
  return status.state === "not_determined" ? ["request", "settings"] : ["settings"];
}

export function missingPermissions(statuses: PermissionStatus[]): PermissionStatus[] {
  return statuses.filter((s) => s.state !== "granted");
}
