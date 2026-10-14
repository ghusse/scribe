import { defineConfig, mergeConfig } from "vitest/config";
import viteConfig from "./vite.config.ts";

// Coverage rule (see CLAUDE.md): >= 95 % of lines for TypeScript AND for Preact components (.tsx), each gated on
// its own (a per-glob threshold), so one language cannot hide the other behind a combined total.
// Excludes and thresholds must stay in sync with CLAUDE.md: tests/coverage-policy.test.ts checks it.
// The tests run on the bun runtime (`bun --bun vitest`): hence happy-dom (jsdom fails to start under bun) and
// istanbul coverage (the v8 provider needs V8's inspector; bun runs JavaScriptCore).
export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      environment: "happy-dom",
      setupFiles: ["tests/setup.ts"],
      // Real CSS modules: the default stub invents a class for any key, so a missing class could not fail a test.
      css: { include: /\.module\.css$/ },
      include: ["src/**/*.test.{ts,tsx}", "tests/**/*.test.ts"],
      coverage: {
        provider: "istanbul",
        include: ["src/**/*.{ts,tsx}"],
        exclude: ["src/**/*.test.{ts,tsx}", "src/main/main.tsx", "src/overlay/main.tsx", "preview/**"],
        reporter: [["text", { skipFull: false }], "html", "lcov"],
        reportsDirectory: "coverage/ui",
        thresholds: {
          lines: 95,
          "src/**/*.ts": { lines: 95 },
          "src/**/*.tsx": { lines: 95 },
        },
      },
    },
  }),
);
