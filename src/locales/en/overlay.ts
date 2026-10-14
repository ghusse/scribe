import type { RecordingWarning } from "../../lib/api";

export const overlay = {
  recording: "Recording",
  locked: "Locked",
  processing: "Transcribing…",
  copy: "Copy",
  view: "View",
  close: "Close",
  warnings: { shared_microphone: "Microphone not exclusive: other apps can hear you" } satisfies Record<RecordingWarning, string>,
};
