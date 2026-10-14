import { useLayoutEffect, useRef } from "preact/hooks";
import { cx } from "../lib/ui";
import { fieldHelp, fieldText, NUMBER_FIELDS, parseNumber, type NumberFieldId } from "../lib/validation";
import styles from "./Settings.module.css";

export interface NumberFieldProps {
  id: NumberFieldId;
  value: number;
  invalid: boolean;
  onCommit: (n: number) => void;
}

/** A bounded number of the settings, committed on change (blur or Entrée), never per keystroke. */
export default function NumberField({ id, value, invalid, onCommit }: NumberFieldProps) {
  const f = NUMBER_FIELDS[id];
  const text = fieldText(id);
  const input = useRef<HTMLInputElement>(null);
  // Written only when the value changes: a re-render must not wipe a number being typed.
  useLayoutEffect(() => {
    input.current!.value = String(value);
  }, [value]);
  return (
    <label class={cx(styles.row, styles.field)}>
      <span>{text.label}</span>
      <span class={styles.num}>
        <input
          ref={input}
          type="number"
          min={f.min}
          max={f.max}
          step="1"
          aria-invalid={invalid}
          aria-describedby={`help-${id}`}
          onChange={(e) => onCommit(parseNumber(e.currentTarget.value))}
        />
        <span class={styles.unit}>{text.unit}</span>
      </span>
      <span class={styles.help} id={`help-${id}`}>
        {fieldHelp(id)}
      </span>
    </label>
  );
}
