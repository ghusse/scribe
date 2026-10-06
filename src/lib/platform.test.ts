import { describe, expect, it } from "vitest";
import { detectOs } from "./platform";

describe("detectOs", () => {
  it("recognises WKWebView on macOS and defaults to Windows", () => {
    expect(detectOs("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)")).toBe("mac");
    expect(detectOs("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Edg/130.0")).toBe("windows");
    expect(detectOs("")).toBe("windows");
  });
});
