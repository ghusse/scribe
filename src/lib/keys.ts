import { OS, type Os } from "./platform";

const NAMES: Record<number, string> = {
  0x08: "Retour arrière", 0x09: "Tab", 0x0d: "Entrée", 0x14: "Verr. Maj", 0x1b: "Échap", 0x20: "Espace",
  0x5b: "Windows gauche", 0x5c: "Windows droite", 0x5d: "Menu",
  0xa0: "Maj gauche", 0xa1: "Maj droite", 0xa2: "Ctrl gauche", 0xa3: "Ctrl droit",
  0xa4: "Alt gauche", 0xa5: "Alt droit (AltGr)",
};

/** macOS names of the keys that differ (the backend maps Command to the Windows keys, Option to Alt). */
const MAC_NAMES: Record<number, string> = {
  0x5b: "Cmd gauche", 0x5c: "Cmd droit", 0xa4: "Option gauche", 0xa5: "Option droite",
};

export function keyName(vk: number, os: Os = OS): string {
  if (vk === 0) return "Aucune";
  const name = (os === "mac" && MAC_NAMES[vk]) || NAMES[vk];
  if (name) return name;
  if (vk >= 0x30 && vk <= 0x39) return String.fromCharCode(vk);
  if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk);
  if (vk >= 0x70 && vk <= 0x87) return `F${vk - 0x6f}`;
  return `Touche 0x${vk.toString(16).toUpperCase()}`;
}

/** Modifier groups, mirroring `scribe_core::chord`: Ctrl, Maj, Alt, Win (generic, left, right). */
const MODIFIERS: [string, number[]][] = [
  ["Ctrl", [0x11, 0xa2, 0xa3]],
  ["Maj", [0x10, 0xa0, 0xa1]],
  ["Alt", [0x12, 0xa4, 0xa5]],
  ["Win", [0x5b, 0x5c]],
];
const MAC_MODIFIER_NAMES = ["Ctrl", "Maj", "Option", "Cmd"];

function modifierGroup(vk: number): number {
  return MODIFIERS.findIndex(([, codes]) => codes.includes(vk));
}

export function isModifier(vk: number): boolean {
  return modifierGroup(vk) >= 0;
}

/**
 * « Ctrl + Maj + A ». A single key keeps its side (« Ctrl droit »); in a combination, modifiers match
 * either side and are named without one.
 */
export function chordName(keys: number[], os: Os = OS): string {
  if (keys.length === 0) return "Aucun";
  if (keys.length === 1) return keyName(keys[0], os);
  const modifierName = (k: number) => (os === "mac" ? MAC_MODIFIER_NAMES : MODIFIERS.map(([n]) => n))[modifierGroup(k)];
  return keys.map((k) => (isModifier(k) ? modifierName(k) : keyName(k, os))).join(" + ");
}

/** Whether `vk` belongs to the combination (`scribe_core::chord::contains`). */
export function chordContains(keys: number[], vk: number): boolean {
  if (vk === 0) return false;
  const sideless = keys.length > 1;
  return keys.some((k) => k === vk || (sideless && isModifier(k) && modifierGroup(k) === modifierGroup(vk)));
}

/** Combinations Windows keeps for itself (`scribe_core::chord::reserved_warning`). The macOS hook intercepts
 * every combination. */
export function reservedWarning(keys: number[], os: Os = OS): string | null {
  if (os === "mac") return null;
  const groups = keys.map(modifierGroup);
  const has = (g: number) => groups.includes(g);
  const only = (...gs: number[]) => groups.every((g) => gs.includes(g));
  if (has(3) && keys.includes(0x4c)) return "Windows réserve Win + L (verrouillage de session) : ce raccourci ne peut pas être intercepté.";
  if (keys.length === 2 && has(2) && has(1) && only(1, 2)) return "Windows peut utiliser Alt + Maj pour changer la langue du clavier.";
  if (keys.length === 2 && has(0) && has(1) && only(0, 1)) return "Windows peut utiliser Ctrl + Maj pour changer la disposition du clavier.";
  return null;
}
