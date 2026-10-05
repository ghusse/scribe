import { describe, expect, it, vi } from "vitest";
import type { OverlayEvent } from "../lib/api";
import { barHeight, createOverlay, emptyLevels, LEVEL_BARS, pushLevel, reduce, toastActions, toastTimeout, type OverlayView } from "./model";

const toast = (over: Partial<Extract<OverlayEvent, { kind: "toast" }>> = {}): OverlayEvent => ({
  kind: "toast", level: "info", message: "Inséré", preview: null, dictation_id: 7, ...over,
});

describe("pure helpers", () => {
  it("keeps error toasts longer", () => {
    expect(toastTimeout("error")).toBe(10000);
    for (const l of ["info", "uncertain", "copied"] as const) expect(toastTimeout(l)).toBe(6000);
  });

  it("scrolls the meter, amplifies and caps the level", () => {
    const start = emptyLevels();
    expect(start).toHaveLength(LEVEL_BARS);
    const one = pushLevel(start, 0.1);
    expect(one).toHaveLength(LEVEL_BARS);
    expect(one[LEVEL_BARS - 1]).toBeCloseTo(0.6);
    expect(one.slice(0, -1).every((l) => l === 0)).toBe(true);
    expect(pushLevel(one, 5)[LEVEL_BARS - 1]).toBe(1);
    expect(pushLevel(one, 5)[LEVEL_BARS - 2]).toBeCloseTo(0.6);
    expect(pushLevel(one, -1)[LEVEL_BARS - 1]).toBe(0);
    expect(start.every((l) => l === 0)).toBe(true); // not mutated
  });

  it("maps a level to a bar height", () => {
    expect(barHeight(0)).toBe(4);
    expect(barHeight(1)).toBe(28);
  });

  it("offers Copier and Voir only for a dictation, and no Copier on errors", () => {
    expect(toastActions({ kind: "idle" })).toEqual({ copy: false, view: false });
    expect(toastActions({ kind: "processing" })).toEqual({ copy: false, view: false });
    expect(toastActions(toast({ dictation_id: null }))).toEqual({ copy: false, view: false });
    expect(toastActions(toast())).toEqual({ copy: true, view: true });
    expect(toastActions(toast({ level: "copied" }))).toEqual({ copy: true, view: true });
    expect(toastActions(toast({ level: "error" }))).toEqual({ copy: false, view: true });
  });

  it("resets the meter on a new recording, keeps it when locking, schedules toasts", () => {
    const view: OverlayView = { ov: { kind: "idle" }, levels: pushLevel(emptyLevels(), 0.1) };
    const rec = reduce(view, { kind: "recording", locked: false });
    expect(rec.view.levels).toEqual(emptyLevels());
    expect(rec.timeout).toBeNull();
    const locked = reduce(view, { kind: "recording", locked: true });
    expect(locked.view.levels).toBe(view.levels);
    expect(locked.view.ov).toEqual({ kind: "recording", locked: true });
    expect(reduce(view, toast({ level: "error" })).timeout).toBe(10000);
    expect(reduce(view, { kind: "processing" }).timeout).toBeNull();
  });
});

/** A controller on fake timers and a fake backend; `views` records every render. */
function setup() {
  const timers: { fn: () => void; ms: number; cancelled: boolean }[] = [];
  let resolveCopy: (() => void) | undefined;
  let rejectCopy: ((e: unknown) => void) | undefined;
  const deps = {
    dismiss: vi.fn(() => Promise.resolve()),
    copy: vi.fn(() => new Promise<void>((res, rej) => { resolveCopy = res; rejectCopy = rej; })),
    openHistory: vi.fn(() => Promise.resolve()),
    schedule: vi.fn((fn: () => void, ms: number) => { const t = { fn, ms, cancelled: false }; timers.push(t); return t; }),
    cancel: vi.fn((h: unknown) => { (h as { cancelled: boolean }).cancelled = true; }),
    report: vi.fn(),
  };
  const views: OverlayView[] = [];
  const ctl = createOverlay(deps, (v) => views.push(v));
  return { ctl, deps, views, timers, resolveCopy: () => resolveCopy!(), rejectCopy: (e: unknown) => rejectCopy!(e) };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("createOverlay", () => {
  it("starts idle and renders each event", () => {
    const { ctl, views } = setup();
    expect(ctl.view().ov).toEqual({ kind: "idle" });
    ctl.event({ kind: "processing" });
    expect(views.at(-1)!.ov).toEqual({ kind: "processing" });
  });

  it("feeds the level meter", () => {
    const { ctl, views } = setup();
    ctl.event({ kind: "recording", locked: false });
    ctl.level(0.1);
    expect(views.at(-1)!.levels[LEVEL_BARS - 1]).toBeCloseTo(0.6);
    expect(views.at(-1)!.ov).toEqual({ kind: "recording", locked: false });
  });

  it("dismisses a toast on its own after its timeout and tells the backend", () => {
    const { ctl, deps, timers } = setup();
    ctl.event(toast());
    expect(timers).toHaveLength(1);
    expect(timers[0].ms).toBe(6000);
    timers[0].fn();
    expect(ctl.view().ov).toEqual({ kind: "idle" });
    expect(deps.dismiss).toHaveBeenCalledTimes(1);
  });

  it("a new event cancels the pending toast timer", () => {
    const { ctl, deps, timers } = setup();
    ctl.event(toast());
    ctl.event({ kind: "recording", locked: false });
    expect(timers[0].cancelled).toBe(true);
    expect(deps.cancel).toHaveBeenCalledTimes(1);
  });

  it("closing by hand cancels the timer", () => {
    const { ctl, deps, timers } = setup();
    ctl.event(toast());
    ctl.dismiss();
    expect(timers[0].cancelled).toBe(true);
    expect(ctl.view().ov).toEqual({ kind: "idle" });
    expect(deps.dismiss).toHaveBeenCalledTimes(1);
  });

  it("destroy cancels the timer without rendering", () => {
    const { ctl, timers, views } = setup();
    ctl.event(toast());
    const n = views.length;
    ctl.destroy();
    expect(timers[0].cancelled).toBe(true);
    expect(views).toHaveLength(n);
  });

  it("reports a failed dismiss instead of leaving an unhandled rejection", async () => {
    const { ctl, deps } = setup();
    deps.dismiss.mockReturnValueOnce(Promise.reject("ipc down"));
    ctl.dismiss();
    await flush();
    expect(deps.report).toHaveBeenCalledWith("ipc down");
  });

  it("copies then dismisses", async () => {
    const s = setup();
    s.ctl.event(toast());
    const done = s.ctl.copy();
    expect(s.deps.copy).toHaveBeenCalledWith(7);
    s.resolveCopy();
    await done;
    expect(s.ctl.view().ov).toEqual({ kind: "idle" });
    expect(s.deps.dismiss).toHaveBeenCalledTimes(1);
  });

  it("a recording that starts while copying is not hidden by the copy's dismiss", async () => {
    const s = setup();
    s.ctl.event(toast());
    const done = s.ctl.copy();
    s.ctl.event({ kind: "recording", locked: false });
    s.resolveCopy();
    await done;
    expect(s.ctl.view().ov).toEqual({ kind: "recording", locked: false });
    expect(s.deps.dismiss).not.toHaveBeenCalled();
  });

  it("a failed copy turns into an error toast (no unhandled rejection), with Voir still offered", async () => {
    const s = setup();
    s.ctl.event(toast());
    const done = s.ctl.copy();
    s.rejectCopy("presse-papier occupé");
    await expect(done).resolves.toBeUndefined();
    const ov = s.ctl.view().ov;
    expect(ov).toEqual({ kind: "toast", level: "error", message: "Copie impossible : presse-papier occupé", preview: null, dictation_id: 7 });
    expect(toastActions(ov)).toEqual({ copy: false, view: true });
    expect(s.timers.at(-1)!.ms).toBe(10000);
    expect(s.deps.dismiss).not.toHaveBeenCalled();
  });

  it("a failed copy does not cover an event that arrived meanwhile", async () => {
    const s = setup();
    s.ctl.event(toast());
    const done = s.ctl.copy();
    s.ctl.event({ kind: "processing" });
    s.rejectCopy("x");
    await done;
    expect(s.ctl.view().ov).toEqual({ kind: "processing" });
  });

  it("copy and Voir do nothing without a dictation", async () => {
    const s = setup();
    await s.ctl.copy();
    s.ctl.openHistory();
    s.ctl.event(toast({ dictation_id: null }));
    await s.ctl.copy();
    s.ctl.openHistory();
    expect(s.deps.copy).not.toHaveBeenCalled();
    expect(s.deps.openHistory).not.toHaveBeenCalled();
  });

  it("Voir opens the dictation, keeps the toast and reports failures", async () => {
    const s = setup();
    s.ctl.event(toast({ dictation_id: 3 }));
    s.ctl.openHistory();
    expect(s.deps.openHistory).toHaveBeenCalledWith(3);
    expect(s.ctl.view().ov.kind).toBe("toast");
    s.deps.openHistory.mockReturnValueOnce(Promise.reject("no window"));
    s.ctl.openHistory();
    await flush();
    expect(s.deps.report).toHaveBeenCalledWith("no window");
  });
});
