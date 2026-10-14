import { listen } from "@tauri-apps/api/event";
import { useSignal } from "@preact/signals";
import { useEffect, useState } from "preact/hooks";
import { api, type OverlayEvent } from "../lib/api";
import { barHeight, createOverlay, emptyLevels, toastActions, type OverlayView } from "./model";
import styles from "./Overlay.module.css";

export default function Overlay() {
  const view = useSignal<OverlayView>({ ov: { kind: "idle" }, levels: emptyLevels() });
  const [overlay] = useState(() =>
    createOverlay(
      {
        dismiss: api.overlayDismiss,
        copy: api.copyDictation,
        openHistory: api.openHistory,
        schedule: (fn, ms) => setTimeout(fn, ms),
        cancel: (h) => clearTimeout(h as ReturnType<typeof setTimeout>),
        report: (e) => console.error(e),
      },
      (v) => (view.value = v),
    ),
  );

  useEffect(() => {
    const unOverlay = listen<OverlayEvent>("overlay", (e) => overlay.event(e.payload));
    const unLevel = listen<number>("audio-level", (e) => overlay.level(e.payload));
    return () => {
      overlay.destroy();
      unOverlay.then((f) => f());
      unLevel.then((f) => f());
    };
  }, []);

  const { ov, levels } = view.value;
  if (ov.kind === "recording") {
    return (
      <div class={styles.recording}>
        <div class={styles.pill} role="status" aria-label="Enregistrement">
          <span class={styles.dot}></span>
          <div class={styles.bars}>
            {levels.map((l, i) => (
              <span key={i} style={{ height: `${barHeight(l)}px` }}></span>
            ))}
          </div>
          {ov.locked && <span class={styles.tag}>Verrouillé</span>}
        </div>
        {ov.warning && (
          <p class={styles.warning} role="alert">
            {ov.warning}
          </p>
        )}
      </div>
    );
  }
  if (ov.kind === "processing") {
    return (
      <div class={styles.pill} role="status">
        <span class={styles.spinner}></span>
        <span class={styles.label}>Transcription…</span>
      </div>
    );
  }
  if (ov.kind === "toast") {
    const actions = toastActions(ov);
    return (
      <div class={`${styles.toast} ${styles[ov.level] ?? ""}`} data-level={ov.level} role="status">
        <div class={styles.text}>
          <strong>{ov.message}</strong>
          {ov.preview && <p>{ov.preview}</p>}
        </div>
        <div class={styles.actions}>
          {actions.copy && <button onClick={overlay.copy}>Copier</button>}
          {actions.view && <button onClick={overlay.openHistory}>Voir</button>}
          <button class={styles.close} onClick={overlay.dismiss} aria-label="Fermer">
            ×
          </button>
        </div>
      </div>
    );
  }
  return null;
}
