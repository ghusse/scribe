import { defineConfig, mergeConfig } from "vitest/config";
import { svelteTesting } from "@testing-library/svelte/vite";
import viteConfig from "./vite.config.ts";

// Coverage rule (see CLAUDE.md): >= 95 % of lines for TypeScript AND for Svelte components, each gated on
// its own (a per-glob threshold), so one language cannot hide the other behind a combined total.
// Excludes and thresholds must stay in sync with CLAUDE.md: tests/coverage-policy.test.ts checks it.
// The tests run on the bun runtime (`bun --bun vitest`): hence happy-dom (jsdom fails to start under bun) and
// istanbul coverage (the v8 provider needs V8's inspector; bun runs JavaScriptCore).
export default mergeConfig(
  viteConfig,
  defineConfig({
    plugins: [svelteTesting()],
    test: {
      environment: "happy-dom",
      include: ["src/**/*.test.ts", "tests/**/*.test.ts"],
      coverage: {
        provider: "istanbul",
        include: ["src/**/*.{ts,svelte}"],
        exclude: ["src/**/*.test.ts", "src/main/main.ts", "src/overlay/overlay.ts", "preview/**"],
        reporter: [["text", { skipFull: false }], "html", "lcov"],
        reportsDirectory: "coverage/ui",
        thresholds: {
          lines: 95,
          "src/**/*.ts": { lines: 95 },
          "src/**/*.svelte": { lines: 95 },
        },
      },
    },
  }),
);
