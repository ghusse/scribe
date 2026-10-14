import { listen } from "@tauri-apps/api/event";
import { useSignal } from "@preact/signals";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { api, type OverlayEvent, type StyleBadge } from "../lib/api";
import { noticeText } from "../lib/errors";
import { t, useLanguage } from "../lib/i18n";
import { barHeight, createOverlay, emptyLevels, hitAreas, markHovered, toastActions, type OverlayPointer, type OverlayView } from "./model";
import styles from "./Overlay.module.css";

function Badge({ style }: { style: StyleBadge | null }) {
  if (!style) return null;
  return (
    <span class={styles.badge} title={t.value.overlay.style(style.name)}>
      {style.name}
    </span>
  );
}

export default function Overlay() {
  useLanguage();
  const view = useSignal<OverlayView>({ ov: { kind: "idle" }, levels: emptyLevels() });
  const hovered = useSignal<number | null>(null);
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
    // macOS: the web view gets no mouse event while another app is active, the native side tracks the pointer.
    const unHover = listen<OverlayPointer>("overlay-pointer", (e) => {
      overlay.hover(e.payload.inside);
      hovered.value = e.payload.button;
    });
    return () => {
      overlay.destroy();
      unOverlay.then((f) => f());
      unLevel.then((f) => f());
      unHover.then((f) => f());
    };
  }, []);

  const sentAreas = useRef("");
  // Read during render so a hover change re-renders.
  const hoveredButton = hovered.value;
  useLayoutEffect(() => {
    markHovered(document, hoveredButton);
    const areas = hitAreas(document);
    const key = JSON.stringify(areas);
    if (key === sentAreas.current) return;
    sentAreas.current = key;
    api.overlayHitAreas(areas).catch((e) => console.error(e));
  });

  const { ov, levels } = view.value;
  const m = t.value.overlay;
  if (ov.kind === "recording") {
    return (
      <div class={styles.recording}>
        <div class={styles.pill} data-surface role="status" aria-label={m.recording}>
          <Badge style={ov.style} />
          <span class={styles.dot}></span>
          <div class={styles.bars}>
            {levels.map((l, i) => (
              <span key={i} style={{ height: `${barHeight(l)}px` }}></span>
            ))}
          </div>
          {ov.locked && <span class={styles.tag}>{m.locked}</span>}
        </div>
        {ov.warning && (
          <p class={styles.warning} data-surface role="alert">
            {m.warnings[ov.warning]}
          </p>
        )}
      </div>
    );
  }
  if (ov.kind === "processing") {
    return (
      <div class={styles.pill} data-surface role="status">
        <Badge style={ov.style} />
        <span class={styles.spinner}></span>
        <span class={styles.label}>{m.processing}</span>
      </div>
    );
  }
  if (ov.kind === "toast") {
    const actions = toastActions(ov);
    const notice = noticeText(ov.message);
    return (
      <div
        class={`${styles.toast} ${styles[ov.level] ?? ""}`}
        data-level={ov.level}
        data-surface
        role="status"
        onMouseEnter={() => overlay.hover(true)}
        onMouseLeave={() => overlay.hover(false)}
      >
        <div class={styles.text}>
          <div class={styles.head}>
            <Badge style={ov.style} />
            <strong>{notice.title}</strong>
          </div>
          {notice.detail && <span class={styles.detail}>{notice.detail}</span>}
          {ov.preview && <p>{ov.preview}</p>}
        </div>
        <div class={styles.actions}>
          {actions.copy && <button onClick={overlay.copy}>{m.copy}</button>}
          {actions.view && <button onClick={overlay.openHistory}>{m.view}</button>}
          <button class={styles.close} onClick={overlay.dismiss} aria-label={m.close}>
            <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
              <path d="M3 3l8 8M11 3l-8 8" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
            </svg>
          </button>
        </div>
      </div>
    );
  }
  return null;
}
