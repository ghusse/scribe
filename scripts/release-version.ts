// Release helper run by semantic-release (release.config.mjs > @semantic-release/exec), with bun:
//   bun scripts/release-version.ts <X.Y.Z>           prepare: writes the version into the three files below
//   bun scripts/release-version.ts --output <X.Y.Z>  success: appends version=X.Y.Z to $GITHUB_OUTPUT
// The app version is the one of src-tauri/Cargo.toml (tauri.conf.json has none); package.json and the scribe-app
// entry of Cargo.lock follow it, so `cargo build --locked` and the bundles agree on the released version.
// Text edits only (no reformatting, no cargo call: the release runner has no registry cache to re-resolve from).
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/** Versions semantic-release may publish here: X.Y.Z only (the MSI bundler rejects pre-release versions). */
export function assertVersion(version: string): void {
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error(`not an X.Y.Z version: ${JSON.stringify(version)}`);
}

/** Replaces the single match of the global `pattern` (two groups around the version) or throws with `what`. */
function replaceOnce(text: string, pattern: RegExp, version: string, what: string): string {
  const count = text.match(pattern)?.length ?? 0;
  if (count !== 1) throw new Error(`${what}: expected one version, found ${count}`);
  return text.replace(pattern, (_, before: string, after: string) => `${before}${version}${after}`);
}

/** package.json: the top-level "version" (indented by two spaces, as bun writes it). */
export function setPackageJsonVersion(text: string, version: string): string {
  return replaceOnce(text, /^( {2}"version": ")[^"]*(",?\r?)$/gm, version, "package.json");
}

/** Cargo.toml: `version = "..."` inside the [package] table, never a dependency's version. */
export function setCargoTomlVersion(text: string, version: string): string {
  const lines = text.split("\n");
  let inPackage = false;
  let found = 0;
  const out = lines.map((line) => {
    const bare = line.replace(/\r$/, "");
    if (/^\s*\[/.test(bare)) inPackage = bare.trim() === "[package]";
    const m = inPackage ? bare.match(/^(version\s*=\s*")[^"]*(".*)$/) : null;
    if (!m) return line;
    found++;
    return `${m[1]}${version}${m[2]}${line.endsWith("\r") ? "\r" : ""}`;
  });
  if (found !== 1) throw new Error(`Cargo.toml: expected one [package] version, found ${found}`);
  return out.join("\n");
}

/** Cargo.lock: the version of the `name` package entry only. */
export function setCargoLockVersion(text: string, name: string, version: string): string {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return replaceOnce(
    text,
    new RegExp(`(\\[\\[package\\]\\]\\r?\\nname = "${escaped}"\\r?\\nversion = ")[^"]*(")`, "g"),
    version,
    `Cargo.lock (${name})`,
  );
}

/** The files of a release commit (besides CHANGELOG.md, written by @semantic-release/changelog). */
export const RELEASE_FILES = ["package.json", "src-tauri/Cargo.toml", "Cargo.lock"] as const;

/** Writes `version` into every release file under `root`. Checks all of them before writing any. */
export function bumpVersion(root: string, version: string): void {
  assertVersion(version);
  const edits: Record<(typeof RELEASE_FILES)[number], (text: string) => string> = {
    "package.json": (t) => setPackageJsonVersion(t, version),
    "src-tauri/Cargo.toml": (t) => setCargoTomlVersion(t, version),
    "Cargo.lock": (t) => setCargoLockVersion(t, "scribe-app", version),
  };
  const updated = RELEASE_FILES.map((file) => [file, edits[file](readFileSync(join(root, file), "utf8"))] as const);
  for (const [file, text] of updated) writeFileSync(join(root, file), text);
}

/** Appends `version=X.Y.Z` to the step outputs file, when there is one (GitHub Actions). */
export function writeOutput(outputFile: string | undefined, version: string): void {
  assertVersion(version);
  if (outputFile) appendFileSync(outputFile, `version=${version}\n`);
}

export function main(args: string[], root: string, env: Record<string, string | undefined>): void {
  if (args.length === 2 && args[0] === "--output") return writeOutput(env.GITHUB_OUTPUT, args[1]);
  if (args.length === 1 && !args[0].startsWith("-")) return bumpVersion(root, args[0]);
  throw new Error("usage: release-version.ts <X.Y.Z> | --output <X.Y.Z>");
}

if (import.meta.main) main(process.argv.slice(2), process.cwd(), process.env);
