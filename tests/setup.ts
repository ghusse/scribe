import { afterEach, beforeEach, jest, mock } from "bun:test";
import { cleanup } from "@testing-library/preact";
import { coreModule, eventModule, resetTauri } from "./tauri";

// bun test runs every file in one process and module mocks are global: the Tauri doubles are installed once here.
mock.module("@tauri-apps/api/core", () => coreModule);
mock.module("@tauri-apps/api/event", () => eventModule);

// Imported once the doubles are installed: i18n loads the Tauri modules.
const { lang } = await import("../src/lib/i18n");

// @testing-library/dom drives bun's fake timers through the `jest` global, which bun only defines in test files.
Object.assign(globalThis, { jest });

beforeEach(() => {
  resetTauri();
  // Most assertions read the French UI; a test about English sets `lang` itself.
  lang.value = "fr";
});
afterEach(cleanup);
