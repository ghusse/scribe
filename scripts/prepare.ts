// Run by `bun install` (package.json > prepare): enables the git hooks and removes the `node_modules/.bin` shims
// named like a native build tool. `bun run` / `bun tauri` put that directory first on PATH, so on Windows
// turso_sdk_kit's build script ran the npm `rc` package (pulled by semantic-release) instead of the SDK's rc.exe:
// it exits 0 without writing the .res, and the link fails.
import { execFileSync } from "node:child_process";
import { readdirSync, rmSync } from "node:fs";
import { join } from "node:path";

/** Native build tools that a `node_modules/.bin` shim must not shadow. */
export const BUILD_TOOLS = ["rc"];

/** The entries of `.bin` that shadow a build tool, whatever the shim's extension (`rc`, `rc.exe`, `rc.bunx`, ...). */
export function shadowingShims(entries: string[]): string[] {
  return entries.filter((entry) => BUILD_TOOLS.includes(entry.split(".")[0].toLowerCase()));
}

/** Removes the shadowing shims from `binDir` (absent when nothing installed a binary) and returns their names. */
export function removeShadowingShims(binDir: string): string[] {
  let entries: string[];
  try {
    entries = readdirSync(binDir);
  } catch {
    return [];
  }
  const shims = shadowingShims(entries);
  for (const shim of shims) rmSync(join(binDir, shim), { force: true });
  return shims;
}

if (import.meta.main) {
  execFileSync("git", ["config", "core.hooksPath", ".githooks"], { stdio: "inherit" });
  removeShadowingShims(join(import.meta.dir, "..", "node_modules", ".bin"));
}
