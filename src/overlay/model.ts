import type { OverlayEvent, ToastLevel } from "../lib/api";

/** Number of bars in the recording pill's level meter. */
export const LEVEL_BARS = 12;

/** How long a toast stays before it dismisses itself: errors stay longer. */
export function toastTimeout(level: ToastLevel): number {
  return level === "error" ? 10000 : 6000;
}

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
  return { view: { ov: ev, levels }, timeout: ev.kind === "toast" ? toastTimeout(ev.level) : null };
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
 * The overlay's behaviour, without Svelte: `render` receives every new view.
 *
 * Every incoming event bumps a generation; an action that awaits the backend (« Copier ») only
 * dismisses if no event arrived meanwhile, so a toast copied just as a new dictation starts never
 * hides the recording pill.
 */
export function createOverlay(deps: OverlayDeps, render: (v: OverlayView) => void) {
  let view: OverlayView = { ov: { kind: "idle" }, levels: emptyLevels() };
  let timer: unknown;
  let generation = 0;

  const set = (v: OverlayView) => {
    view = v;
    render(v);
  };
  const stopTimer = () => {
    if (timer !== undefined) deps.cancel(timer);
    timer = undefined;
  };

  function event(ev: OverlayEvent) {
    stopTimer();
    generation++;
    const next = reduce(view, ev);
    set(next.view);
    if (next.timeout !== null) timer = deps.schedule(dismiss, next.timeout);
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
      if (gen === generation) event({ kind: "toast", level: "error", message: `Copie impossible : ${e}`, preview: null, dictation_id: id });
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

  return { event, level, dismiss, copy, openHistory, view: () => view, destroy: stopTimer };
}
