import { useSignal } from "@preact/signals";
import { useEffect, useRef } from "preact/hooks";
import { checkModelId } from "../lib/validation";
import styles from "./ModelPicker.module.css";

export interface ModelPickerProps {
  models: string[];
  value: string;
  onValueChange: (next: string) => void;
  onChange?: () => void;
}

const CUSTOM = "__custom__";

/**
 * Known models in a dropdown (first = newest = default), plus « Autre… » for a model typed by hand.
 * Remount it (with `key`) when the provider changes.
 */
export default function ModelPicker({ models, value, onValueChange, onChange }: ModelPickerProps) {
  const forceCustom = useSignal(false);
  const invalid = useSignal<string | null>(null);
  const focusPending = useRef(false);
  const input = useRef<HTMLInputElement>(null);
  const selected = forceCustom.value || !models.includes(value) ? CUSTOM : value;

  // Uncontrolled input: a rejected id stays as typed; only a new committed value rewrites it.
  useEffect(() => {
    if (input.current) input.current.value = value;
  }, [value, selected === CUSTOM]);

  useEffect(() => {
    if (!focusPending.current) return;
    focusPending.current = false;
    // Typing replaces the prefilled id; leaving it as is keeps the current model.
    input.current?.focus();
    input.current?.select();
  });

  function update(next: string) {
    onValueChange(next);
    onChange?.();
  }

  function pick(e: Event) {
    const v = (e.currentTarget as HTMLSelectElement).value;
    forceCustom.value = v === CUSTOM;
    invalid.value = null;
    if (v !== CUSTOM) {
      update(v);
      return;
    }
    focusPending.current = true;
  }

  // Committed on change (blur or Entrée), never per keystroke: « claude- » must not be saved.
  function commit(e: Event) {
    const el = e.currentTarget as HTMLInputElement;
    const r = checkModelId(el.value);
    if (!r.ok) {
      invalid.value = r.message;
      return;
    }
    invalid.value = null;
    el.value = r.value;
    if (r.value !== value) update(r.value);
  }

  return (
    <div class={styles.picker}>
      <select value={selected} onChange={pick}>
        {models.map((m, i) => (
          <option key={m} value={m}>
            {m}
            {i === 0 ? " (dernier, par défaut)" : ""}
          </option>
        ))}
        <option value={CUSTOM}>Autre…</option>
      </select>
      {selected === CUSTOM && (
        <input
          ref={input}
          defaultValue={value}
          title={value}
          placeholder="ex. gpt-4o-mini (identifiant exact de l'API)"
          aria-label="Identifiant du modèle personnalisé"
          aria-invalid={invalid.value !== null}
          onChange={commit}
        />
      )}
      {invalid.value && (
        <span class={styles.pickerError} role="alert">
          {invalid.value}
        </span>
      )}
    </div>
  );
}
