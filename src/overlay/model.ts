import type { HitArea, OverlayAreas, OverlayEvent } from "../lib/api";
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

/** Buttons of a toast: « Copier » needs a dictation and is pointless on an error, « Voir » needs a dictation. */
export function toastActions(ov: OverlayEvent): { copy: boolean; view: boolean } {
  if (ov.kind !== "toast" || ov.dictation_id === null) return { copy: false, view: false };
  return { copy: ov.level !== "error", view: true };
}

export interface OverlayView {
  ov: OverlayEvent;
  levels: number[];
}

/**
 * State after an `overlay` event: a new (unlocked) recording starts with an empty meter, a toast
 * schedules its own dismissal after `timeout` ms (null: nothing to schedule).
 */
export function reduce(view: OverlayView, ev: OverlayEvent): { view: OverlayView; timeout: number | null } {
  const levels = ev.kind === "recording" && !ev.locked ? emptyLevels() : view.levels;
  return { view: { ov: ev, levels }, timeout: ev.kind === "toast" ? toastTimeout(ev) : null };
}

const area = (el: Element): HitArea => {
  const r = el.getBoundingClientRect();
  return [r.x, r.y, r.width, r.height];
};

/**
 * What the native pointer tracking checks (see `overlay_hit_areas`): the bounding box of the `[data-surface]`
 * elements (the visible pill or toast, not the transparent window) and the buttons.
 */
export function hitAreas(root: ParentNode): OverlayAreas {
  return { surface: bounds([...root.querySelectorAll("[data-surface]")].map(area)), buttons: [...root.querySelectorAll("button")].map(area) };
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
  root.querySelectorAll("button").forEach((b, i) => b.toggleAttribute("data-hover", i === index));
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
  let view: OverlayView = { ov: { kind: "idle" }, levels: emptyLevels() };
  let timer: unknown;
  let generation = 0;
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
    set({ ...view, ov: { kind: "idle" } });
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

  return { event, level, hover, dismiss, copy, openHistory, view: () => view, destroy: stopTimer };
}
