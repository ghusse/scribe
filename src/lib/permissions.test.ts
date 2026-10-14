import { describe, expect, it } from "bun:test";
import { missingPermissions, permissionActions } from "./permissions";

describe("permissions", () => {
  it("offers the prompt while the OS can show it, the settings page in every case", () => {
    expect(permissionActions({ permission: "microphone", state: "not_determined" })).toEqual(["request", "settings"]);
    expect(permissionActions({ permission: "microphone", state: "denied" })).toEqual(["settings"]);
    expect(permissionActions({ permission: "accessibility", state: "granted" })).toEqual([]);
  });

  it("keeps only the permissions not granted", () => {
    const mic = { permission: "microphone", state: "denied" } as const;
    expect(missingPermissions([{ permission: "accessibility", state: "granted" }, mic])).toEqual([mic]);
    expect(missingPermissions([])).toEqual([]);
  });
});
