// @vitest-environment node
// The prepare step of semantic-release (scripts/release-version.ts): the release commit must change exactly the
// app version in package.json, src-tauri/Cargo.toml ([package]) and Cargo.lock (scribe-app), nothing else.
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  assertVersion,
  bumpVersion,
  main,
  RELEASE_FILES,
  setCargoLockVersion,
  setCargoTomlVersion,
  setPackageJsonVersion,
  writeOutput,
} from "../scripts/release-version.ts";

const repo = new URL("../", import.meta.url);
const read = (path: string) => readFileSync(new URL(path, repo), "utf8");

/** Lines that differ between two texts of the same length in lines. */
function changedLines(before: string, after: string): [string, string][] {
  const a = before.split("\n");
  const b = after.split("\n");
  expect(b).toHaveLength(a.length);
  return a.flatMap((line, i) => (line === b[i] ? [] : [[line, b[i]] as [string, string]]));
}

describe("assertVersion", () => {
  it("accepts X.Y.Z only", () => {
    expect(() => assertVersion("0.4.0")).not.toThrow();
    expect(() => assertVersion("12.0.31")).not.toThrow();
    for (const bad of ["v0.4.0", "0.4", "0.4.0-beta.1", "0.4.0\n", ""]) {
      expect(() => assertVersion(bad), bad).toThrow(/X\.Y\.Z/);
    }
  });
});

describe("setPackageJsonVersion", () => {
  it("changes the top-level version line only", () => {
    const before = read("package.json");
    const after = setPackageJsonVersion(before, "9.8.7");
    expect(JSON.parse(after).version).toBe("9.8.7");
    expect(changedLines(before, after)).toHaveLength(1);
  });

  it("keeps CRLF line endings", () => {
    const after = setPackageJsonVersion('{\r\n  "name": "x",\r\n  "version": "0.1.0",\r\n  "private": true\r\n}\r\n', "0.2.0");
    expect(after).toBe('{\r\n  "name": "x",\r\n  "version": "0.2.0",\r\n  "private": true\r\n}\r\n');
  });

  it("ignores nested versions and fails without a top-level one", () => {
    expect(() => setPackageJsonVersion('{\n  "a": {\n    "version": "1.0.0"\n  }\n}\n', "0.2.0")).toThrow(
      /package.json: expected one version, found 0/,
    );
  });
});

describe("setCargoTomlVersion", () => {
  it("changes the [package] version of src-tauri/Cargo.toml only", () => {
    const before = read("src-tauri/Cargo.toml");
    const after = setCargoTomlVersion(before, "9.8.7");
    expect(changedLines(before, after)).toEqual([[expect.stringMatching(/^version = "/), 'version = "9.8.7"']]);
  });

  it("leaves versions of other tables alone, keeps CRLF and trailing comments", () => {
    const toml = '[dependencies]\r\nversion = "1"\r\n[package]\r\nname = "a"\r\nversion = "0.1.0" # app\r\n[x]\r\nversion = "2"\r\n';
    expect(setCargoTomlVersion(toml, "0.2.0")).toBe(
      '[dependencies]\r\nversion = "1"\r\n[package]\r\nname = "a"\r\nversion = "0.2.0" # app\r\n[x]\r\nversion = "2"\r\n',
    );
  });

  it("fails without exactly one [package] version", () => {
    expect(() => setCargoTomlVersion('[package]\nname = "a"\n[workspace]\nversion = "1"\n', "0.2.0")).toThrow(/found 0/);
  });
});

describe("setCargoLockVersion", () => {
  it("changes the scribe-app entry of Cargo.lock only", () => {
    const before = read("Cargo.lock");
    const after = setCargoLockVersion(before, "scribe-app", "9.8.7");
    expect(changedLines(before, after)).toEqual([[expect.stringMatching(/^version = "/), 'version = "9.8.7"']]);
    expect(after).toContain('name = "scribe-app"\nversion = "9.8.7"\n');
  });

  it("matches the exact name, with CRLF too", () => {
    const lock = '[[package]]\r\nname = "scribe-app-x"\r\nversion = "1.0.0"\r\n\r\n[[package]]\r\nname = "scribe-app"\r\nversion = "0.1.0"\r\n';
    expect(setCargoLockVersion(lock, "scribe-app", "0.2.0")).toBe(lock.replace('"0.1.0"', '"0.2.0"'));
    expect(() => setCargoLockVersion(lock, "missing", "0.2.0")).toThrow(/Cargo.lock \(missing\): expected one version, found 0/);
  });
});

describe("bumpVersion / main", () => {
  let dir: string;
  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  function scratch(): string {
    dir = mkdtempSync(join(tmpdir(), "release-version-"));
    mkdirSync(join(dir, "src-tauri"));
    for (const file of RELEASE_FILES) copyFileSync(new URL(file, repo), join(dir, file));
    return dir;
  }

  it("writes the version into the three release files", () => {
    const root = scratch();
    main(["1.2.3"], root, {});
    expect(JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version).toBe("1.2.3");
    expect(readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8")).toMatch(/^\[package\]\nname = "scribe-app"\nversion = "1\.2\.3"/m);
    expect(readFileSync(join(root, "Cargo.lock"), "utf8")).toContain('name = "scribe-app"\nversion = "1.2.3"\n');
  });

  it("writes nothing when one file cannot be updated", () => {
    const root = scratch();
    writeFileSync(join(root, "Cargo.lock"), "# no scribe-app here\n");
    const packageJson = readFileSync(join(root, "package.json"), "utf8");
    expect(() => bumpVersion(root, "1.2.3")).toThrow(/Cargo.lock/);
    expect(readFileSync(join(root, "package.json"), "utf8")).toBe(packageJson);
  });

  it("rejects a malformed version before touching anything", () => {
    const root = scratch();
    expect(() => main(["v1.2.3"], root, {})).toThrow(/X\.Y\.Z/);
    expect(readFileSync(join(root, "package.json"), "utf8")).toBe(read("package.json"));
  });

  it("--output appends version= to $GITHUB_OUTPUT, and does nothing outside Actions", () => {
    const root = scratch();
    const output = join(root, "output");
    writeFileSync(output, "other=1\n");
    main(["--output", "1.2.3"], root, { GITHUB_OUTPUT: output });
    expect(readFileSync(output, "utf8")).toBe("other=1\nversion=1.2.3\n");
    expect(() => writeOutput(undefined, "1.2.3")).not.toThrow();
    expect(() => main(["--output", "latest"], root, { GITHUB_OUTPUT: output })).toThrow(/X\.Y\.Z/);
  });

  it("rejects other arguments", () => {
    for (const args of [[], ["--output"], ["1.2.3", "1.2.4"]]) {
      expect(() => main(args, "/nowhere", {}), args.join(" ")).toThrow(/usage/);
    }
  });
});
