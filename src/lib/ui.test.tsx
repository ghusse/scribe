import { describe, expect, it } from "vitest";
import { render } from "@testing-library/preact";
import { signal } from "@preact/signals";
import { cx, useSyncEffect, useTick } from "./ui";

describe("cx", () => {
  it("keeps only the truthy class names", () => {
    expect(cx("a", false, null, undefined, "", "b")).toBe("a b");
  });
});

describe("useSyncEffect", () => {
  it("re-runs synchronously on a signal change and stops once unmounted", async () => {
    const count = signal(0);
    const seen: number[] = [];
    function Probe() {
      useSyncEffect(() => {
        seen.push(count.value);
      });
      return null;
    }
    const { unmount } = render(<Probe />);
    count.value = 1;
    expect(seen).toEqual([0, 1]);
    unmount();
    // Preact 11 runs useEffect cleanups after paint.
    await new Promise((resolve) => setTimeout(resolve, 50));
    count.value = 2;
    expect(seen).toEqual([0, 1]);
  });
});

describe("useTick", () => {
  it("resolves after the pending state change is in the DOM", async () => {
    const text = signal("before");
    let tick!: () => Promise<void>;
    function Probe() {
      tick = useTick();
      return <p>{text.value}</p>;
    }
    const { container } = render(<Probe />);
    text.value = "after";
    await tick();
    expect(container.textContent).toBe("after");
  });
});
