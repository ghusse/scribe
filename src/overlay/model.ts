import type { HitArea, Notice, OverlayAreas, OverlayEvent, StyleBadge, StyleCatalog } from "../lib/api";
import { errorText } from "../lib/errors";

/** Number of bars in the recording pill's level meter. */
export const LEVEL_BARS = 12;

type Toast = Extract<OverlayEvent, { kind: "toast" }>;

/** How long a toast stays before it dismisses itself: errors stay longer, a style badge leaves time to reach it. */
export function toastTimeout(ev: Toast): number {
  if (ev.level === "error") return 10000;
  return ev.style ? 8000 : 6000;
}

/** Once a toast's delay ran out under the pointer, how long it stays after the pointer leaves. */
export const LEAVE_GRACE_MS = 1000;

export function emptyLevels(): number[] {
  return Array(LEVEL_BARS).fill(0);
}

/** Scrolls the meter by one bar: the RMS level is amplified (speech is ~0.05–0.15) and capped at 1. */
export function pushLevel(levels: number[], sample: number): number[] {
  return [...levels.slice(1), Math.min(1, Math.max(0, sample * 6))];
}

/** Height in px of one bar of the meter. */
export function barHeight(level: number): number {
  return 4 + level * 24;
}

/** Notices whose dictation has a transcript another style can rewrite. */
const RESTYLABLE: ReadonlySet<Notice["code"]> = new Set<Notice["code"]>([
  "pasted",
  "pasted_uncertain",
  "pasted_without_correction",
  "copied",
  "not_pasted",
  "restyled",
  "restyle_copied",
]);

/**
 * Buttons of a toast: « Copier » needs a dictation and is pointless on an error, « Voir » needs a dictation,
 * « Changer de style » a dictation with a style badge, a transcript and correction enabled.
 */
export function toastActions(ov: OverlayEvent, correctionEnabled = false): { copy: boolean; view: boolean; restyle: boolean } {
  if (ov.kind !== "toast" || ov.dictation_id === null) return { copy: false, view: false, restyle: false };
  const restyle = correctionEnabled && ov.style !== null && RESTYLABLE.has(ov.message.code);
  return { copy: ov.level !== "error", view: true, restyle };
}

export interface OverlayView {
  ov: OverlayEvent;
  levels: number[];
  /** Styles offered by the picker; null until fetched. */
  catalog: StyleCatalog | null;
  /** The style picker replaces the toast's text and buttons. */
  picker: boolean;
  /** The style asked for while its rewrite runs. */
  rewriting: StyleBadge | null;
}

export function initialView(): OverlayView {
  return { ov: { kind: "idle" }, levels: emptyLevels(), catalog: null, picker: false, rewriting: null };
}

/**
 * State after an `overlay` event: a new (unlocked) recording starts with an empty meter, a toast
 * schedules its own dismissal after `timeout` ms (null: nothing to schedule).
 */
export function reduce(view: OverlayView, ev: OverlayEvent): { view: OverlayView; timeout: number | null } {
  const levels = ev.kind === "recording" && !ev.locked ? emptyLevels() : view.levels;
  return { view: { ...view, ov: ev, levels, picker: false, rewriting: null }, timeout: ev.kind === "toast" ? toastTimeout(ev) : null };
}

const area = (el: Element): HitArea => {
  const r = el.getBoundingClientRect();
  return [r.x, r.y, r.width, r.height];
};

/** A button scrolled out of its `[data-clip]` row keeps its index but only its visible part, so it never covers another button. */
function visibleArea(el: Element): HitArea {
  const clip = el.parentElement?.closest("[data-clip]");
  return clip ? intersect(area(el), area(clip)) : area(el);
}

function intersect(a: HitArea, b: HitArea): HitArea {
  const left = Math.max(a[0], b[0]);
  const top = Math.max(a[1], b[1]);
  const right = Math.min(a[0] + a[2], b[0] + b[2]);
  const bottom = Math.min(a[1] + a[3], b[1] + b[3]);
  return [left, top, Math.max(0, right - left), Math.max(0, bottom - top)];
}

/** Buttons the native pointer tracking knows, by index: `hitAreas` and `markHovered` must list the same ones. */
const CLICKABLE = "button:not(:disabled)";

/**
 * What the native pointer tracking checks (see `overlay_hit_areas`): the bounding box of the `[data-surface]`
 * elements (the visible pill or toast, not the transparent window) and the enabled buttons.
 */
export function hitAreas(root: ParentNode): OverlayAreas {
  return { surface: bounds([...root.querySelectorAll("[data-surface]")].map(area)), buttons: [...root.querySelectorAll(CLICKABLE)].map(visibleArea) };
}

function bounds(areas: HitArea[]): HitArea | null {
  if (areas.length === 0) return null;
  const left = Math.min(...areas.map((a) => a[0]));
  const top = Math.min(...areas.map((a) => a[1]));
  const right = Math.max(...areas.map((a) => a[0] + a[2]));
  const bottom = Math.max(...areas.map((a) => a[1] + a[3]));
  return [left, top, right - left, bottom - top];
}

/** Marks the button the native pointer tracking reports as hovered (`index` in `hitAreas` order), so it can be styled. */
export function markHovered(root: ParentNode, index: number | null) {
  root.querySelectorAll("button").forEach((b) => b.removeAttribute("data-hover"));
  if (index !== null) root.querySelectorAll(CLICKABLE)[index]?.setAttribute("data-hover", "");
}

/**
 * A vertical wheel scrolls an overflowing row sideways (Chromium and WebKit leave it to the page). Returns whether
 * it did, so the caller prevents the default.
 */
export function wheelSideways(row: { scrollLeft: number; scrollWidth: number; clientWidth: number }, deltaX: number, deltaY: number): boolean {
  if (row.scrollWidth <= row.clientWidth || Math.abs(deltaY) <= Math.abs(deltaX)) return false;
  row.scrollLeft += deltaY;
  return true;
}

/** `overlay-pointer` event: where the native tracking sees the pointer. */
export interface OverlayPointer {
  inside: boolean;
  button: number | null;
}

export interface OverlayDeps {
  /** Tells the backend the overlay is hidden (overlay_dismiss). */
  dismiss: () => Promise<void>;
  copy: (id: number) => Promise<void>;
  openHistory: (id: number) => Promise<void>;
  styleCatalog: () => Promise<StyleCatalog>;
  /** Rewrites the dictation with another style; the result arrives as a toast event. */
  restyle: (id: number, styleId: string) => Promise<void>;
  schedule: (fn: () => void, ms: number) => unknown;
  cancel: (handle: unknown) => void;
  /** Failures nobody waits for (dismiss, open history): logged, never an unhandled rejection. */
  report: (e: unknown) => void;
}

/**
 * The overlay's behaviour, independent of the UI framework: `render` receives every new view.
 *
 * Every incoming event bumps a generation; an action that awaits the backend (« Copier ») only
 * dismisses if no event arrived meanwhile, so a toast copied just as a new dictation starts never
 * hides the recording pill.
 */
export function createOverlay(deps: OverlayDeps, render: (v: OverlayView) => void) {
  let view: OverlayView = initialView();
  let timer: unknown;
  let generation = 0;
  let catalogRequest = 0;
  let hovered = false;
  /** The toast's delay ran out while the pointer was over it. */
  let expired = false;

  const set = (v: OverlayView) => {
    view = v;
    // A toast unmounted under the pointer gets no leave event (DOM or native): a stale hover would keep later toasts.
    if (v.ov.kind !== "toast") hovered = false;
    render(v);
  };
  const stopTimer = () => {
    if (timer !== undefined) deps.cancel(timer);
    timer = undefined;
  };

  function event(ev: OverlayEvent) {
    stopTimer();
    generation++;
    expired = false;
    const next = reduce(view, ev);
    set(next.view);
    if (next.timeout !== null) timer = deps.schedule(expire, next.timeout);
    // Fetched for every badged toast: styles may have changed in the settings since the last one.
    if (ev.kind === "toast" && ev.style !== null && ev.dictation_id !== null) loadCatalog();
  }

  function loadCatalog() {
    const request = ++catalogRequest;
    deps
      .styleCatalog()
      .then((catalog) => {
        if (request === catalogRequest) set({ ...view, catalog });
      })
      .catch(deps.report);
  }

  /** Style names follow the UI language: a catalog already fetched is fetched again. */
  function languageChanged() {
    if (view.catalog !== null) loadCatalog();
  }

  function expire() {
    timer = undefined;
    if (hovered) expired = true;
    else dismiss();
  }

  /** A toast never hides under the pointer: once its delay ran out, it goes shortly after the pointer leaves. */
  function hover(inside: boolean) {
    hovered = inside;
    if (!expired) return;
    stopTimer();
    if (!inside) timer = deps.schedule(dismiss, LEAVE_GRACE_MS);
  }

  function level(sample: number) {
    set({ ...view, levels: pushLevel(view.levels, sample) });
  }

  function dismiss() {
    stopTimer();
    generation++;
    set({ ...view, ov: { kind: "idle" }, picker: false, rewriting: null });
    deps.dismiss().catch(deps.report);
  }

  /** « Copier »: copies, then closes the toast, unless something else is shown by then. */
  async function copy() {
    const ov = view.ov;
    if (ov.kind !== "toast" || ov.dictation_id === null) return;
    const id = ov.dictation_id;
    const gen = generation;
    try {
      await deps.copy(id);
    } catch (e) {
      if (gen === generation) event({ kind: "toast", level: "error", message: { code: "copy_failed", detail: errorText(e) }, preview: null, dictation_id: id, style: ov.style });
      return;
    }
    if (gen === generation) dismiss();
  }

  /** « Voir »: opens the dictation in the history window; the toast stays. */
  function openHistory() {
    const ov = view.ov;
    if (ov.kind !== "toast" || ov.dictation_id === null) return;
    deps.openHistory(ov.dictation_id).catch(deps.report);
  }

  const restylable = () => toastActions(view.ov, view.catalog?.correction_enabled).restyle;

  /** « Changer de style »: shows the style chips; the toast no longer dismisses itself. */
  function openPicker() {
    if (!restylable()) return;
    stopTimer();
    expired = false;
    set({ ...view, picker: true });
  }

  /**
   * Asks for a rewrite with `styleId` and shows it running; the result toast comes from the backend. A refused
   * call becomes an error toast, unless something else is shown by then.
   */
  async function restyle(styleId: string) {
    const ov = view.ov;
    const style = view.catalog?.styles.find((s) => s.id === styleId);
    if (ov.kind !== "toast" || ov.dictation_id === null || !style || !restylable()) return;
    const id = ov.dictation_id;
    const gen = generation;
    set({ ...view, picker: false, rewriting: style });
    try {
      await deps.restyle(id, styleId);
    } catch (e) {
      if (gen === generation) event({ kind: "toast", level: "error", message: { code: "restyle_refused", detail: errorText(e) }, preview: null, dictation_id: id, style: ov.style });
    }
  }

  return { event, level, hover, dismiss, copy, openHistory, openPicker, restyle, languageChanged, view: () => view, destroy: stopTimer };
}
