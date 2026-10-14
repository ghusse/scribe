// Guards the coverage rule (CLAUDE.md > "Exclusion policy"): the documented threshold and exclusions must be
// exactly what the tooling applies. Widening the Rust regex, adding a UI exclude or lowering a threshold
// without updating CLAUDE.md (and its justification) fails here.
import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "bun:test";
import { EXCLUDES, GLOBS, LCOV, measured, scanSources, THRESHOLD } from "../scripts/coverage-gate.ts";

const root = new URL("../", import.meta.url);
const read = (path: string) => readFileSync(new URL(path, root), "utf8");

/** The backticked values of the CLAUDE.md policy bullet starting with `label` (bullets may wrap). */
function documented(label: string): string[] {
  const md = read("CLAUDE.md").replace(/\r\n/g, "\n");
  const start = md.indexOf(`\n- ${label}`);
  expect(start, `CLAUDE.md must have a "- ${label}" bullet`).toBeGreaterThanOrEqual(0);
  const rest = md.slice(start + 1);
  const end = rest.slice(2).search(/\n(- |\n|\|)/);
  const bullet = end < 0 ? rest : rest.slice(0, end + 2);
  // Parenthesised notes (where the value lives, why) are not values; the values follow the colon.
  const text = bullet.replace(/\([^)]*\)/g, "");
  const values = text.slice(text.indexOf(":") + 1);
  return [...values.matchAll(/`([^`]+)`/g)].map((m) => m[1]);
}

const threshold = Number(documented("Threshold")[0]);
const packageJson = JSON.parse(read("package.json")) as { scripts: Record<string, string> };
const bunfig = Bun.TOML.parse(read("bunfig.toml")) as {
  test: { coverageReporter: string[]; coverageDir: string; coverageSkipTestFiles: boolean };
};

/** Every Rust source llvm-cov can report on (workspace crates and the Tauri crate), `/`-separated. */
function rustSources(): string[] {
  return ["crates", "src-tauri"].flatMap((dir) =>
    (readdirSync(new URL(dir, root), { recursive: true }) as string[])
      .map((p) => `${dir}/${p.replace(/\\/g, "/")}`)
      .filter((p) => p.endsWith(".rs") && !p.split("/").includes("target")),
  );
}

describe("coverage policy", () => {
  it("documents a 95 % threshold", () => {
    expect(threshold).toBe(95);
  });

  describe("Rust (cargo llvm-cov)", () => {
    const script = packageJson.scripts["coverage:rust"];
    const regex = script.match(/--ignore-filename-regex "([^"]+)"/)?.[1];

    it("fails under the documented threshold", () => {
      expect(script).toMatch(/^cargo llvm-cov --workspace /);
      expect(script).toContain(`--fail-under-lines ${threshold} `);
    });

    it("ignores exactly the documented regex", () => {
      expect(regex).toBe(documented("Rust ignore regex")[0]);
    });

    it("the regex excludes exactly the documented files, with either path separator", () => {
      const re = new RegExp(regex!);
      const sources = rustSources();
      expect(sources).toContain("crates/scribe-platform/src/key_filter.rs");
      const excluded = sources.filter((p) => re.test(p)).sort();
      expect(excluded).toEqual([...documented("Rust files excluded by that regex")].sort());
      const excludedWin = sources.filter((p) => re.test(`C:\\repo\\${p.replace(/\//g, "\\")}`)).sort();
      expect(excludedWin).toEqual(excluded);
    });
  });

  describe("UI (bun test + scripts/coverage-gate.ts)", () => {
    it("excludes exactly the documented globs", () => {
      expect(EXCLUDES).toEqual(documented("UI excludes"));
    });

    it("gates TypeScript and TSX each at the threshold", () => {
      expect(documented("UI per-language thresholds")).toEqual(["src/**/*.ts", "src/**/*.tsx"]);
      expect(GLOBS).toEqual(documented("UI per-language thresholds"));
      expect(THRESHOLD).toBe(threshold);
    });

    it("measures every TS module and TSX component", () => {
      const all = (readdirSync(new URL("src", root), { recursive: true }) as string[]).map(
        (p) => `src/${p.replace(/\\/g, "/")}`,
      );
      const scanned = measured(scanSources(fileURLToPath(root)));
      expect(scanned).toContain("src/lib/history.ts");
      expect(scanned).toContain("src/main/App.tsx");
      expect(scanned).toEqual(measured(all));
    });

    it("gates the lcov report bun test writes, without the test files", () => {
      expect(bunfig.test.coverageReporter).toContain("lcov");
      expect(LCOV).toBe(`${bunfig.test.coverageDir}/lcov.info`);
      expect(bunfig.test.coverageSkipTestFiles).toBe(true);
    });

    it("runs both gates from bun run coverage", () => {
      expect(packageJson.scripts["coverage:ui"]).toBe("bun test --coverage && bun scripts/coverage-gate.ts");
      expect(packageJson.scripts.coverage).toBe("bun run coverage:ui && bun run coverage:rust");
    });
  });

  describe("CI", () => {
    const ci = read(".github/workflows/ci.yml").replace(/\r\n/g, "\n");

    it("runs the UI and Rust gates as separate steps, the Rust one even when the UI one fails", () => {
      expect(ci).toContain("        run: bun run coverage:ui\n");
      expect(ci).toMatch(
        /\n {8}if: \$\{\{ !cancelled\(\) && steps\.build\.outcome == 'success' \}\}\n {8}run: bun run coverage:rust\n/,
      );
      expect(ci).not.toMatch(/run: bun run coverage\n/);
    });
  });
});
