import { keyName } from "./keys";

export type HotkeyRole = "trigger" | "lock";
export interface Hotkeys { trigger_vk: number; lock_vk: number }

export const VK_ESCAPE = 0x1b;
/** Shown when the 10 s capture window expires without a key press. */
export const CAPTURE_TIMEOUT_MESSAGE = "Aucune touche détectée, raccourci inchangé";
export const TYPING_KEY_WARNING = "Cette touche ne fonctionnera plus pour la saisie normale. Continuer ?";

/** Keys used to type text: as the trigger (never swallowed by the hook) each press starts a dictation. */
export function isTypingKey(vk: number): boolean {
  return (
    (vk >= 0x30 && vk <= 0x39) || // digits
    (vk >= 0x41 && vk <= 0x5a) || // letters
    (vk >= 0x60 && vk <= 0x69) || // numpad digits
    vk === 0x20 || vk === 0x0d || vk === 0x09 // Space, Enter, Tab
  );
}

/** How a capture_key call ended: the user cancelled (button or Échap), the window expired, or a key. */
export function captureOutcome(vk: number | null, cancelled: boolean): "cancelled" | "timeout" | "key" {
  if (cancelled || vk === VK_ESCAPE) return "cancelled";
  return vk === null ? "timeout" : "key";
}

export type Assignment =
  | { ok: true; keys: Hotkeys; note: string | null; confirm: boolean }
  | { ok: false; note: string };

/**
 * Assigns a captured key to a role. Taking the other role's key swaps the two (the backend
 * rejects trigger == lock); `confirm` asks before making a typing key the trigger.
 */
export function assignHotkey(role: HotkeyRole, vk: number, cur: Hotkeys): Assignment {
  let keys: Hotkeys;
  let note: string | null = null;
  if (role === "trigger") {
    keys = vk === cur.lock_vk ? { trigger_vk: vk, lock_vk: cur.trigger_vk } : { ...cur, trigger_vk: vk };
  } else if (vk === cur.trigger_vk) {
    // No lock key to give back to the trigger: a swap would leave it empty.
    if (cur.lock_vk === 0) return { ok: false, note: `${keyName(vk)} est déjà la touche de déclenchement.` };
    keys = { trigger_vk: cur.lock_vk, lock_vk: vk };
  } else {
    keys = { ...cur, lock_vk: vk };
  }
  const swapped = keys.trigger_vk === cur.lock_vk && keys.lock_vk === cur.trigger_vk && cur.trigger_vk !== cur.lock_vk;
  if (swapped) note = `Touches échangées : Déclenchement = ${keyName(keys.trigger_vk)}, Verrouillage = ${keyName(keys.lock_vk)}.`;
  const confirm = keys.trigger_vk !== cur.trigger_vk && isTypingKey(keys.trigger_vk);
  return { ok: true, keys, note, confirm };
}
