import type { Permission, PermissionStatus } from "./api";

export const PERMISSION_COPY: Record<Permission, { title: string; why: string; how: string }> = {
  accessibility: {
    title: "Accessibilité",
    why: "Sans elle, le raccourci ne répond pas et Scribe ne peut ni coller le texte ni vérifier qu'il est arrivé.",
    how: "Cliquez sur « Autoriser », puis activez Scribe dans Réglages Système : le raccourci marche aussitôt, sans relancer Scribe. Si Scribe y est déjà coché (après une mise à jour, l'autorisation ne vaut plus), « Autoriser » la remet à zéro : il suffit de le réactiver.",
  },
  microphone: {
    title: "Micro",
    why: "Sans lui, Scribe n'entend rien : les dictées sont vides.",
    how: "Cliquez sur « Autoriser » et acceptez la demande de macOS. Si vous l'avez refusée, activez Scribe dans Réglages Système > Micro.",
  },
};

export type PermissionAction = "request" | "settings";

/** The OS prompt while it can still be shown, the settings page in every case. */
export function permissionActions(status: PermissionStatus): PermissionAction[] {
  if (status.state === "granted") return [];
  return status.state === "not_determined" ? ["request", "settings"] : ["settings"];
}

export function missingPermissions(statuses: PermissionStatus[]): PermissionStatus[] {
  return statuses.filter((s) => s.state !== "granted");
}
