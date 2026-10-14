import type { RecordingWarning } from "../../lib/api";

export const overlay = {
  recording: "Recording",
  locked: "Locked",
  processing: "Transcribing…",
  copy: "Copy",
  view: "View",
  style: (name: string) => `Style: ${name}`,
  close: "Close",
  warnings: { shared_microphone: "Microphone not exclusive: other apps can hear you" } satisfies Record<RecordingWarning, string>,
};
