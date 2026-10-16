import { mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "bun:test";
import { removeShadowingShims, shadowingShims } from "../scripts/prepare.ts";

describe("shadowingShims", () => {
  it("picks every shim of a build tool, whatever its extension, and nothing else", () => {
    const entries = ["rc", "rc.exe", "RC.CMD", "rc.bunx", "tauri", "tauri.exe", "rcedit.exe", "src"];
    expect(shadowingShims(entries)).toEqual(["rc", "rc.exe", "RC.CMD", "rc.bunx"]);
  });
});

describe("removeShadowingShims", () => {
  let dir = "";
  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  it("removes the build-tool shims and keeps the others", () => {
    dir = mkdtempSync(join(tmpdir(), "scribe-prepare-"));
    const bin = join(dir, ".bin");
    mkdirSync(bin);
    for (const name of ["rc.exe", "rc.bunx", "tauri.exe"]) writeFileSync(join(bin, name), "");
    expect(removeShadowingShims(bin).sort()).toEqual(["rc.bunx", "rc.exe"]);
    expect(readdirSync(bin)).toEqual(["tauri.exe"]);
  });

  it("does nothing without a .bin directory", () => {
    dir = mkdtempSync(join(tmpdir(), "scribe-prepare-"));
    expect(removeShadowingShims(join(dir, ".bin"))).toEqual([]);
  });
});
