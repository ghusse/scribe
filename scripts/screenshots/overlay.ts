// Overlay with demo data: the recording pill with a speech-like level meter.
import { emit } from "./tauri-demo";
import "../../src/overlay/main";

const levels = [0.02, 0.05, 0.12, 0.08, 0.15, 0.1, 0.04, 0.13, 0.16, 0.09, 0.06, 0.11];
setTimeout(() => {
  emit("overlay", { kind: "recording", locked: false, warning: null });
  for (const l of levels) emit("audio-level", l);
}, 50);
