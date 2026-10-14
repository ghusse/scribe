import { vi } from "bun:test";

/** Fires the fake timers due within `ms`, then lets the IPC promises and the re-render they trigger settle. */
export async function advanceTimers(ms: number) {
  vi.advanceTimersByTime(ms);
  for (let i = 0; i < 10; i++) await Promise.resolve();
}
