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
  if (swapped) note = `Touches échangées\u00a0: Déclenchement = ${keyName(keys.trigger_vk)}, Verrouillage = ${keyName(keys.lock_vk)}.`;
  const confirm = keys.trigger_vk !== cur.trigger_vk && isTypingKey(keys.trigger_vk);
  return { ok: true, keys, note, confirm };
}

/**
 * After a capture ends, the captured key's own keydown/keyup can still reach the page (the hook
 * lets it through, and its IPC reply may arrive first): keep guarding the button that long.
 */
export const CAPTURE_GRACE_MS = 300;

/**
 * Whether a keydown on a role's « Changer / Annuler » button must not activate it: during that
 * role's capture (or just after) the key is the one being captured, so Space or Enter would
 * otherwise cancel the capture or start a new one. Tab still moves the focus; Échap cancels
 * through the hook, and the mouse still works.
 */
export function swallowsKey(
  key: string,
  role: HotkeyRole,
  capturing: HotkeyRole | null,
  ended: { role: HotkeyRole; at: number } | null,
  now: number,
): boolean {
  if (key === "Tab") return false;
  if (capturing === role) return true;
  return ended !== null && ended.role === role && now - ended.at < CAPTURE_GRACE_MS;
}

/** What the confirmation before a typing-key trigger shows: the lock too when it changes (swap). */
export function pendingSummary(next: Hotkeys, cur: Hotkeys): { trigger: string; lock: string | null } {
  return { trigger: keyName(next.trigger_vk), lock: next.lock_vk !== cur.lock_vk ? keyName(next.lock_vk) : null };
}

/**
 * The lock key has one control: « Désactiver » sets lock_vk to 0. Settings saved with the old
 * « Touche de verrouillage active » box unchecked are shown (and later saved) that way.
 */
export function normalizeLock<T extends { lock_vk: number; gesture: { lock_key_enabled: boolean } }>(s: T): T {
  if (s.gesture.lock_key_enabled) return s;
  return { ...s, lock_vk: 0, gesture: { ...s.gesture, lock_key_enabled: true } };
}
