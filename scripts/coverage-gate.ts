// UI coverage gate (CLAUDE.md > "Coverage rule"): bun test only has a global threshold, so the per-language one is
// applied here, on the lcov report written by `bun test --coverage` (bunfig.toml).
import { readFileSync } from "node:fs";
import { Glob } from "bun";

export const THRESHOLD = 95;
/** Each glob is gated on its own, on top of the total, so one language cannot hide the other. */
export const GLOBS = ["src/**/*.ts", "src/**/*.tsx"];
export const EXCLUDES = [
  "src/**/*.d.ts",
  "src/**/*.test.{ts,tsx}",
  "src/main/main.tsx",
  "src/overlay/main.tsx",
  "preview/**",
];
export const LCOV = "coverage/ui/lcov.info";
/** Every source the gate considers; `measured` narrows it to `GLOBS` minus `EXCLUDES`. */
export const SOURCES = "src/**/*.{ts,tsx}";

export type Lines = { found: number; hit: number };
export type Gate = { name: string; files: number; lines: Lines; percent: number; ok: boolean };
export type Result = { gates: Gate[]; missing: string[]; ok: boolean };

/** Line counts per source file of an lcov report (`SF`, `LF`, `LH`), paths `/`-separated. */
export function parseLcov(text: string): Map<string, Lines> {
  const files = new Map<string, Lines>();
  let current: Lines | undefined;
  for (const raw of text.split(/\r?\n/)) {
    const colon = raw.indexOf(":");
    const key = raw.slice(0, colon);
    const value = raw.slice(colon + 1);
    if (key === "SF") {
      current = { found: 0, hit: 0 };
      files.set(value.replace(/\\/g, "/"), current);
    } else if (current && key === "LF") current.found = Number(value);
    else if (current && key === "LH") current.hit = Number(value);
  }
  return files;
}

const matches = (patterns: string[], path: string) => patterns.some((p) => new Glob(p).match(path));

/** Sources the gate measures: under one of the globs and not excluded. */
export function measured(sources: string[]): string[] {
  return sources.filter((p) => matches(GLOBS, p) && !matches(EXCLUDES, p)).sort();
}

function gate(name: string, files: string[], report: Map<string, Lines>): Gate {
  const lines = { found: 0, hit: 0 };
  for (const f of files) {
    lines.found += report.get(f)!.found;
    lines.hit += report.get(f)!.hit;
  }
  const percent = lines.found === 0 ? 100 : (100 * lines.hit) / lines.found;
  return { name, files: files.length, lines, percent, ok: percent >= THRESHOLD };
}

const transpiler = new Bun.Transpiler({ loader: "tsx" });

/** Whether a TS/TSX source emits any JavaScript (a type-only module emits nothing). */
export function hasRuntimeCode(code: string): boolean {
  return transpiler.transformSync(code).trim() !== "";
}

/**
 * The total and per-glob gates over `sources`. A measured source absent from the report was never loaded by a test
 * (bun only reports loaded files): it is listed in `missing` and fails the gate instead of being silently skipped,
 * unless `hasCode` says it emits no JavaScript (type-only modules are erased by `import type`, never loaded).
 */
export function evaluate(sources: string[], report: Map<string, Lines>, hasCode: (path: string) => boolean): Result {
  const files = measured(sources);
  const missing = files.filter((f) => !report.has(f) && hasCode(f));
  const reported = files.filter((f) => report.has(f));
  const gates = [
    gate("total", reported, report),
    ...GLOBS.map((g) => gate(g, reported.filter((f) => new Glob(g).match(f)), report)),
  ];
  return { gates, missing, ok: missing.length === 0 && gates.every((g) => g.ok) };
}

/** The sources under `SOURCES`, `/`-separated. */
export function scanSources(cwd = "."): string[] {
  return [...new Glob(SOURCES).scanSync(cwd)].map((p) => p.replace(/\\/g, "/"));
}

/** Human-readable outcome, one line per gate then one per missing file. */
export function format({ gates, missing }: Result): string {
  const rows = gates.map((g) => {
    const counts = `${g.lines.hit}/${g.lines.found}, ${g.files} files, threshold ${THRESHOLD} %`;
    return `${g.ok ? "ok  " : "FAIL"} ${g.name.padEnd(14)} ${g.percent.toFixed(2).padStart(6)} % lines (${counts})`;
  });
  return [...rows, ...missing.map((f) => `FAIL ${f}: not loaded by any test, so not measured`)].join("\n");
}

if (import.meta.main) {
  const hasCode = (path: string) => hasRuntimeCode(readFileSync(path, "utf8"));
  const result = evaluate(scanSources(), parseLcov(await Bun.file(LCOV).text()), hasCode);
  console.log(format(result));
  if (!result.ok) process.exit(1);
}
