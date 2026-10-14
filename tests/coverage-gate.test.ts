import { describe, expect, it } from "bun:test";
import { evaluate, format, hasRuntimeCode, measured, parseLcov, type Lines } from "../scripts/coverage-gate.ts";

const record = (path: string, found: number, hit: number) =>
  `SF:${path}\nFNF:1\nFNH:1\nDA:1,1\nLF:${found}\nLH:${hit}\nend_of_record\n`;
const report = (entries: Record<string, [number, number]>) =>
  new Map<string, Lines>(Object.entries(entries).map(([p, [found, hit]]) => [p, { found, hit }]));
const sources = ["src/lib/a.ts", "src/main/B.tsx"];
const withCode = () => true;

describe("parseLcov", () => {
  it("reads the line counts of each file, with CRLF and Windows separators", () => {
    const text = (record("src/lib/a.ts", 10, 9) + record("src\\main\\B.tsx", 4, 4)).replace(/\n/g, "\r\n");
    expect(parseLcov("TN:\n" + text)).toEqual(report({ "src/lib/a.ts": [10, 9], "src/main/B.tsx": [4, 4] }));
  });
});

describe("measured", () => {
  it("keeps TS modules and components, drops tests, declarations and entry files", () => {
    const files = [
      "src/lib/a.ts",
      "src/lib/a.test.ts",
      "src/main/App.tsx",
      "src/main/App.test.tsx",
      "src/main/main.tsx",
      "src/overlay/main.tsx",
      "src/css.d.ts",
      "src/main/App.module.css",
    ];
    expect(measured(files)).toEqual(["src/lib/a.ts", "src/main/App.tsx"]);
  });
});

describe("evaluate", () => {
  it("passes at exactly the threshold", () => {
    const result = evaluate(sources, report({ "src/lib/a.ts": [100, 95], "src/main/B.tsx": [20, 20] }), withCode);
    expect(result.ok).toBe(true);
    expect(result.gates.map((g) => [g.name, g.files, g.percent])).toEqual([
      ["total", 2, (100 * 115) / 120],
      ["src/**/*.ts", 1, 95],
      ["src/**/*.tsx", 1, 100],
    ]);
  });

  it("fails one language under the threshold even when the total passes", () => {
    const result = evaluate(sources, report({ "src/lib/a.ts": [1000, 1000], "src/main/B.tsx": [20, 18] }), withCode);
    expect(result.gates.map((g) => g.ok)).toEqual([true, true, false]);
    expect(result.ok).toBe(false);
  });

  it("fails on a source no test loaded, which the report does not list", () => {
    const result = evaluate([...sources, "src/lib/orphan.ts"], report({ "src/lib/a.ts": [1, 1], "src/main/B.tsx": [1, 1] }), withCode);
    expect(result.missing).toEqual(["src/lib/orphan.ts"]);
    expect(result.gates.every((g) => g.ok)).toBe(true);
    expect(result.ok).toBe(false);
  });

  it("does not require a type-only source, which no test can load", () => {
    const hasCode = (path: string) => path !== "src/lib/types.ts";
    const result = evaluate([...sources, "src/lib/types.ts"], report({ "src/lib/a.ts": [1, 1], "src/main/B.tsx": [1, 1] }), hasCode);
    expect(result.missing).toEqual([]);
    expect(result.ok).toBe(true);
  });

  it("ignores reported files outside the measured sources", () => {
    const result = evaluate(sources, report({ "src/lib/a.ts": [1, 1], "src/main/B.tsx": [1, 1], "tests/x.ts": [9, 0] }), withCode);
    expect(result.ok).toBe(true);
    expect(result.gates[0].lines).toEqual({ found: 2, hit: 2 });
  });
});

describe("hasRuntimeCode", () => {
  it("is false only for a module erased by the TypeScript transpilation", () => {
    expect(hasRuntimeCode('import type { X } from "./x";\nexport type T = { a: X };\nexport interface I {}\n')).toBe(false);
    expect(hasRuntimeCode("export const a = 1;")).toBe(true);
    expect(hasRuntimeCode('import "./side";')).toBe(true);
    expect(hasRuntimeCode("export const C = () => <div />;")).toBe(true);
  });
});

describe("format", () => {
  it("prints one line per gate, then each missing file", () => {
    const result = evaluate([...sources, "src/lib/orphan.ts"], report({ "src/lib/a.ts": [3, 2], "src/main/B.tsx": [1, 1] }), withCode);
    const text = format(result);
    expect(text.split("\n")).toEqual([
      "FAIL total           75.00 % lines (3/4, 2 files, threshold 95 %)",
      "FAIL src/**/*.ts     66.67 % lines (2/3, 1 files, threshold 95 %)",
      "ok   src/**/*.tsx   100.00 % lines (1/1, 1 files, threshold 95 %)",
      "FAIL src/lib/orphan.ts: not loaded by any test, so not measured",
    ]);
  });
});
