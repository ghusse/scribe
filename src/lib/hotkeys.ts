import { t } from "./i18n";
import { chordContains, chordName, isModifier, keyName, reservedWarning } from "./keys";

export type HotkeyRole = "trigger" | "lock";
export interface Hotkeys { trigger_keys: number[]; lock_vk: number }

export const MAX_KEYS = 4;

/** Keys used to type text: they are swallowed when they complete the trigger combination. */
export function isTypingKey(vk: number): boolean {
  return (
    (vk >= 0x30 && vk <= 0x39) || // digits
    (vk >= 0x41 && vk <= 0x5a) || // letters
    (vk >= 0x60 && vk <= 0x69) || // numpad digits
    vk === 0x20 || vk === 0x0d || vk === 0x09 // Space, Enter, Tab
  );
}

/**
 * How a capture_key call ended: the cancel button, Échap (the backend answers an empty combination: the
 * hook swallows every key during a capture, so the page never sees Échap), the 10 s window, or keys.
 */
export function captureOutcome(keys: number[] | null, cancelled: boolean): "cancelled" | "timeout" | "keys" {
  if (cancelled || keys?.length === 0) return "cancelled";
  return keys === null ? "timeout" : "keys";
}

export type Assignment =
  | { ok: true; keys: Hotkeys; note: string | null; confirm: boolean }
  | { ok: false; note: string };

const isShift = (vk: number) => vk === 0x10 || vk === 0xa0 || vk === 0xa1;
const single = (keys: number[]) => (keys.length === 1 ? keys[0] : null);
const sameKeys = (a: number[], b: number[]) => a.length === b.length && a.every((k, i) => k === b[i]);

/**
 * Assigns a captured combination to a role (the backend rejects a lock key inside the trigger). Taking the
 * other role's key swaps the two when both are single keys; `confirm` asks before a trigger made only of
 * typing keys, which would be swallowed while typing.
 */
export function assignHotkey(role: HotkeyRole, captured: number[], cur: Hotkeys): Assignment {
  const m = t.value.settings.hotkey;
  let keys: Hotkeys;
  let swapped = false;
  if (role === "trigger") {
    if (captured.length > MAX_KEYS) return { ok: false, note: m.tooManyKeys(MAX_KEYS) };
    const old = single(cur.trigger_keys);
    if (chordContains(captured, cur.lock_vk)) {
      if (single(captured) !== cur.lock_vk || old === null) {
        return { ok: false, note: m.lockInTrigger(keyName(cur.lock_vk)) };
      }
      keys = { trigger_keys: [cur.lock_vk], lock_vk: old };
      swapped = true;
    } else {
      keys = { ...cur, trigger_keys: captured };
    }
  } else {
    const vk = single(captured);
    if (vk === null) return { ok: false, note: m.lockSingleKey };
    if (chordContains(cur.trigger_keys, vk)) {
      if (single(cur.trigger_keys) !== vk) return { ok: false, note: m.partOfTrigger(keyName(vk)) };
      // No lock key to give back to the trigger: a swap would leave it empty.
      if (cur.lock_vk === 0) return { ok: false, note: m.alreadyTrigger(keyName(vk)) };
      keys = { trigger_keys: [cur.lock_vk], lock_vk: vk };
      swapped = true;
    } else {
      keys = { ...cur, lock_vk: vk };
    }
  }
  const notes = [];
  if (swapped) notes.push(m.swapped(chordName(keys.trigger_keys), keyName(keys.lock_vk)));
  const changed = !sameKeys(keys.trigger_keys, cur.trigger_keys);
  const warning = changed ? reservedWarning(keys.trigger_keys) : null;
  if (warning) notes.push(warning);
  const trigger = keys.trigger_keys;
  // Maj alone does not keep typing safe: Maj + A would swallow every capital A.
  const confirm = changed && !trigger.some((k) => isModifier(k) && !isShift(k)) && trigger.some(isTypingKey);
  return { ok: true, keys, note: notes.length ? notes.join(" ") : null, confirm };
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
  return { trigger: chordName(next.trigger_keys), lock: next.lock_vk !== cur.lock_vk ? keyName(next.lock_vk) : null };
}

/**
 * The lock key has one control: « Désactiver » sets lock_vk to 0. Settings saved with the old
 * « Touche de verrouillage active » box unchecked are shown (and later saved) that way.
 */
export function normalizeLock<T extends { lock_vk: number; gesture: { lock_key_enabled: boolean } }>(s: T): T {
  if (s.gesture.lock_key_enabled) return s;
  return { ...s, lock_vk: 0, gesture: { ...s.gesture, lock_key_enabled: true } };
}
