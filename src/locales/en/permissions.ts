import type { Permission } from "../../lib/api";

export const permissions = {
  panel: "Missing permissions",
  heading: "Scribe needs your permission",
  allow: "Allow",
  openSettings: "Open System Settings",
  requestFailed: (detail: string) => `Could not ask: ${detail}`,
  openFailed: (detail: string) => `Could not open System Settings: ${detail}`,
  copy: {
    accessibility: {
      title: "Accessibility",
      why: "Without it, the shortcut does nothing and Scribe can neither paste the text nor check that it arrived.",
      how: "Click “Allow”, then turn Scribe on in System Settings: the shortcut works right away, no need to restart Scribe. If Scribe is already checked there (after an update, the permission no longer counts), “Allow” resets it: just turn it on again.",
    },
    microphone: {
      title: "Microphone",
      why: "Without it, Scribe hears nothing: dictations come out empty.",
      how: "Click “Allow” and accept the macOS prompt. If you declined it, turn Scribe on in System Settings > Microphone.",
    },
  } satisfies Record<Permission, { title: string; why: string; how: string }>,
};
