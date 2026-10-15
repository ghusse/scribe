import type { RecordingWarning } from "../../lib/api";

export const overlay = {
  recording: "Recording",
  locked: "Locked",
  processing: "Transcribing…",
  copy: "Copy",
  view: "View",
  style: (name: string) => `Style: ${name}`,
  close: "Close",
  changeStyle: "Change style",
  styles: "Styles",
  rewriting: (name: string) => `Rewriting as ${name}…`,
  warnings: { shared_microphone: "Microphone not exclusive: other apps can hear you" } satisfies Record<RecordingWarning, string>,
};
