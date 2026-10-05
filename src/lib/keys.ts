const NAMES: Record<number, string> = {
  0x08: "Retour arrière", 0x09: "Tab", 0x0d: "Entrée", 0x14: "Verr. Maj", 0x1b: "Échap", 0x20: "Espace",
  0x5b: "Windows gauche", 0x5c: "Windows droite", 0x5d: "Menu",
  0xa0: "Maj gauche", 0xa1: "Maj droite", 0xa2: "Ctrl gauche", 0xa3: "Ctrl droit",
  0xa4: "Alt gauche", 0xa5: "Alt droit (AltGr)",
};

export function keyName(vk: number): string {
  if (vk === 0) return "Aucune";
  if (NAMES[vk]) return NAMES[vk];
  if (vk >= 0x30 && vk <= 0x39) return String.fromCharCode(vk);
  if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk);
  if (vk >= 0x70 && vk <= 0x87) return `F${vk - 0x6f}`;
  return `Touche 0x${vk.toString(16).toUpperCase()}`;
}
