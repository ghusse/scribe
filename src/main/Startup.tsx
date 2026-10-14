import { useSignal } from "@preact/signals";
import { useEffect } from "preact/hooks";
import { api } from "../lib/api";
import { errorText } from "../lib/errors";
import { t } from "../lib/i18n";
import styles from "./Startup.module.css";

export type StartupProps = Record<string, never>;

// The state lives in the OS (registry Run key / LaunchAgent), not in the settings file: read it, switch it.
export default function Startup(_props: StartupProps) {
  const enabled = useSignal<boolean | null>(null);
  const busy = useSignal(false);
  const error = useSignal<unknown>(null);
  const notApplied = useSignal(false);

  async function load() {
    try {
      enabled.value = await api.getAutostart();
      error.value = null;
    } catch (e) {
      error.value = e;
    }
  }

  async function toggle(e: Event) {
    // currentTarget is null once the handler awaits: keep the element.
    const input = e.currentTarget as HTMLInputElement;
    const want = input.checked;
    if (busy.value) {
      // aria-disabled rather than disabled while switching: a disabled input would lose the keyboard focus.
      input.checked = !want;
      return;
    }
    busy.value = true;
    try {
      enabled.value = await api.setAutostart(want);
      error.value = null;
      notApplied.value = enabled.value !== want;
    } catch (err) {
      error.value = err;
      notApplied.value = false;
    } finally {
      busy.value = false;
      // Always show what is really in place, not the click.
      input.checked = enabled.value ?? false;
    }
  }

  useEffect(() => {
    void load();
  }, []);

  const m = t.value.settings.startup;
  return (
    <section class={styles.section}>
      <h3>{m.heading}</h3>
      <label class={styles.check}>
        <input
          type="checkbox"
          checked={enabled.value ?? false}
          disabled={enabled.value === null}
          aria-disabled={busy.value}
          onChange={toggle}
        />
        {m.launchAtLogin}
      </label>
      <p class={styles.help}>{m.help}</p>
      {notApplied.value && (
        <p class={styles.help} role="status">
          {m.notApplied}
        </p>
      )}
      {error.value !== null && (
        <p class={styles.error} role="alert">
          {m.unavailable(errorText(error.value))}
        </p>
      )}
    </section>
  );
}
