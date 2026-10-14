import { t, type Messages } from "./i18n";
import { OS, type Os } from "./platform";

type NamedKey = Exclude<keyof Messages["settings"]["keyNames"], "unknown">;

const NAMES: Record<number, NamedKey> = {
  0x08: "backspace", 0x09: "tab", 0x0d: "enter", 0x14: "capsLock", 0x1b: "escape", 0x20: "space",
  0x5b: "leftWin", 0x5c: "rightWin", 0x5d: "menu",
  0xa0: "leftShift", 0xa1: "rightShift", 0xa2: "leftCtrl", 0xa3: "rightCtrl",
  0xa4: "leftAlt", 0xa5: "rightAlt",
};

/** macOS names of the keys that differ (the backend maps Command to the Windows keys, Option to Alt). */
const MAC_NAMES: Record<number, NamedKey> = {
  0x5b: "leftCmd", 0x5c: "rightCmd", 0xa4: "leftOption", 0xa5: "rightOption",
};

export function keyName(vk: number, os: Os = OS): string {
  const names = t.value.settings.keyNames;
  if (vk === 0) return names.none;
  const name = (os === "mac" && MAC_NAMES[vk]) || NAMES[vk];
  if (name) return names[name];
  if (vk >= 0x30 && vk <= 0x39) return String.fromCharCode(vk);
  if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk);
  if (vk >= 0x70 && vk <= 0x87) return `F${vk - 0x6f}`;
  return names.unknown(vk.toString(16).toUpperCase());
}

/** Modifier groups, mirroring `scribe_core::chord`: Ctrl, Maj, Alt, Win (generic, left, right). */
const MODIFIERS: number[][] = [
  [0x11, 0xa2, 0xa3],
  [0x10, 0xa0, 0xa1],
  [0x12, 0xa4, 0xa5],
  [0x5b, 0x5c],
];

function modifierNames(os: Os): string[] {
  const shift = t.value.settings.keyNames.shift;
  return os === "mac" ? ["Ctrl", shift, "Option", "Cmd"] : ["Ctrl", shift, "Alt", "Win"];
}

function modifierGroup(vk: number): number {
  return MODIFIERS.findIndex((codes) => codes.includes(vk));
}

export function isModifier(vk: number): boolean {
  return modifierGroup(vk) >= 0;
}

/**
 * « Ctrl + Maj + A ». A single key keeps its side (« Ctrl droit »); in a combination, modifiers match
 * either side and are named without one.
 */
export function chordName(keys: number[], os: Os = OS): string {
  if (keys.length === 0) return t.value.settings.keyNames.noChord;
  if (keys.length === 1) return keyName(keys[0], os);
  const modifiers = modifierNames(os);
  return keys.map((k) => (isModifier(k) ? modifiers[modifierGroup(k)] : keyName(k, os))).join(" + ");
}

/** Whether `vk` belongs to the combination (`scribe_core::chord::contains`). */
export function chordContains(keys: number[], vk: number): boolean {
  if (vk === 0) return false;
  const sideless = keys.length > 1;
  return keys.some((k) => k === vk || (sideless && isModifier(k) && modifierGroup(k) === modifierGroup(vk)));
}

/** Combinations Windows keeps for itself. The macOS hook intercepts every combination. */
export function reservedWarning(keys: number[], os: Os = OS): string | null {
  if (os === "mac") return null;
  const m = t.value.settings.hotkey;
  const groups = keys.map(modifierGroup);
  const has = (g: number) => groups.includes(g);
  const only = (...gs: number[]) => groups.every((g) => gs.includes(g));
  if (has(3) && keys.includes(0x4c)) return m.reservedWinL;
  if (keys.length === 2 && has(2) && has(1) && only(1, 2)) return m.reservedAltShift;
  if (keys.length === 2 && has(0) && has(1) && only(0, 1)) return m.reservedCtrlShift;
  return null;
}
