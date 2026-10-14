import { useSignal } from "@preact/signals";
import { useEffect } from "preact/hooks";
import { api } from "../lib/api";
import styles from "./Startup.module.css";

export type StartupProps = Record<string, never>;

// The state lives in the OS (registry Run key / LaunchAgent), not in the settings file: read it, switch it.
export default function Startup(_props: StartupProps) {
  const enabled = useSignal<boolean | null>(null);
  const busy = useSignal(false);
  const error = useSignal<string | null>(null);
  const note = useSignal<string | null>(null);

  async function load() {
    try {
      enabled.value = await api.getAutostart();
      error.value = null;
    } catch (e) {
      error.value = String(e);
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
      note.value =
        enabled.value === want ? null : "Le système n'a pas appliqué ce changement (désactivé dans le Gestionnaire des tâches ?).";
    } catch (err) {
      error.value = String(err);
      note.value = null;
    } finally {
      busy.value = false;
      // Always show what is really in place, not the click.
      input.checked = enabled.value ?? false;
    }
  }

  useEffect(() => {
    void load();
  }, []);

  return (
    <section class={styles.section}>
      <h2>Démarrage</h2>
      <label class={styles.check}>
        <input
          type="checkbox"
          checked={enabled.value ?? false}
          disabled={enabled.value === null}
          aria-disabled={busy.value}
          onChange={toggle}
        />
        Lancer Scribe à l'ouverture de session
      </label>
      <p class={styles.help}>Scribe démarre alors discrètement, sans ouvrir de fenêtre, prêt à dicter.</p>
      {note.value && (
        <p class={styles.help} role="status">
          {note.value}
        </p>
      )}
      {error.value && (
        <p class={styles.error} role="alert">
          Démarrage automatique indisponible&nbsp;: {error.value}
        </p>
      )}
    </section>
  );
}
