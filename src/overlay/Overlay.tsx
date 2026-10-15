import { listen } from "@tauri-apps/api/event";
import { useSignal } from "@preact/signals";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { api, type OverlayEvent, type StyleBadge } from "../lib/api";
import { noticeText } from "../lib/errors";
import { lang, t, useLanguage } from "../lib/i18n";
import { cx } from "../lib/ui";
import { barHeight, createOverlay, hitAreas, initialView, markHovered, toastActions, wheelSideways, type OverlayPointer, type OverlayView } from "./model";
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
  const view = useSignal<OverlayView>(initialView());
  const hovered = useSignal<number | null>(null);
  const [overlay] = useState(() =>
    createOverlay(
      {
        dismiss: api.overlayDismiss,
        copy: api.copyDictation,
        openHistory: api.openHistory,
        styleCatalog: api.styleCatalog,
        restyle: api.restyleDictation,
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

  const language = lang.value;
  useEffect(() => overlay.languageChanged(), [language]);

  const sentAreas = useRef("");
  const syncAreas = () => {
    const areas = hitAreas(document);
    const key = JSON.stringify(areas);
    if (key === sentAreas.current) return;
    sentAreas.current = key;
    api.overlayHitAreas(areas).catch((e) => console.error(e));
  };
  // Read during render so a hover change re-renders.
  const hoveredButton = hovered.value;
  useLayoutEffect(() => {
    markHovered(document, hoveredButton);
    syncAreas();
  });

  const { ov, levels, catalog, picker, rewriting } = view.value;
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
    const actions = toastActions(ov, catalog?.correction_enabled);
    const notice = noticeText(ov.message);
    const busy = picker || rewriting !== null;
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
          {rewriting ? (
            <div class={styles.head}>
              <span class={styles.spinner}></span>
              <strong>{m.rewriting(rewriting.name)}</strong>
            </div>
          ) : (
            <div class={styles.head}>
              {!busy && actions.restyle && ov.style ? (
                // The badge opens the picker: a separate button would squeeze the text out of the 440px window.
                <button class={cx(styles.badge, styles.switch)} title={m.changeStyle} aria-haspopup="true" onClick={overlay.openPicker}>
                  <span>{ov.style.name}</span>
                  <svg width="8" height="8" viewBox="0 0 8 8" aria-hidden="true">
                    <path d="M1 2.5l3 3 3-3" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
                  </svg>
                </button>
              ) : (
                <Badge style={ov.style} />
              )}
              <strong>{notice.title}</strong>
            </div>
          )}
          {picker && catalog && (
            // `data-clip`: chips scrolled out of the row must not catch the native pointer (see `hitAreas`).
            <div
              class={styles.chips}
              data-clip
              role="group"
              aria-label={m.styles}
              onScroll={syncAreas}
              onWheel={(e) => wheelSideways(e.currentTarget, e.deltaX, e.deltaY) && e.preventDefault()}
            >
              {catalog.styles.map((s) => {
                const current = s.id === ov.style?.id;
                return (
                  <button key={s.id} class={cx(current && styles.current)} aria-pressed={current} disabled={current} onClick={() => overlay.restyle(s.id)}>
                    {s.name}
                  </button>
                );
              })}
            </div>
          )}
          {!busy && notice.detail && <span class={styles.detail}>{notice.detail}</span>}
          {!busy && ov.preview && <p>{ov.preview}</p>}
        </div>
        <div class={styles.actions}>
          {!busy && actions.copy && <button onClick={overlay.copy}>{m.copy}</button>}
          {!busy && actions.view && <button onClick={overlay.openHistory}>{m.view}</button>}
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
