import { defineConfig, mergeConfig } from "vitest/config";
import { svelteTesting } from "@testing-library/svelte/vite";
import viteConfig from "./vite.config.ts";

// Coverage rule (see CLAUDE.md): >= 95 % of lines for every TS module and Svelte component.
// Exclusions must stay in sync with the list in CLAUDE.md.
export default mergeConfig(
  viteConfig,
  defineConfig({
    plugins: [svelteTesting()],
    test: {
      environment: "jsdom",
      include: ["src/**/*.test.ts"],
      coverage: {
        provider: "v8",
        include: ["src/**/*.{ts,svelte}"],
        exclude: ["src/**/*.test.ts", "src/main/main.ts", "src/overlay/overlay.ts", "preview/**"],
        reporter: [["text", { skipFull: false }], "html", "lcov"],
        reportsDirectory: "coverage/ui",
        thresholds: { lines: 95 },
      },
    },
  }),
);
