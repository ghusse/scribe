import { effect } from "@preact/signals";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";

/** Joins the truthy class names. */
export const cx = (...names: (string | false | null | undefined)[]) => names.filter(Boolean).join(" ");

/**
 * Runs `run` now and again, synchronously, whenever a signal it reads changes. `useSignalEffect` defers re-runs to
 * the next frame, too late for state read right after an edit or on unmount.
 */
export function useSyncEffect(run: () => void) {
  const latest = useRef(run);
  latest.current = run;
  useEffect(() => effect(() => latest.current()), []);
}

/** Returns a function resolving once the state changes made so far are committed to the DOM. */
export function useTick(): () => Promise<void> {
  const [, rerender] = useState(0);
  const waiting = useRef<(() => void)[]>([]);
  useLayoutEffect(() => {
    for (const resolve of waiting.current.splice(0)) resolve();
  });
  return useCallback(
    () =>
      new Promise<void>((resolve) => {
        waiting.current.push(resolve);
        rerender((n) => n + 1);
      }),
    [],
  );
}
