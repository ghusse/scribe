import { useSignal } from "@preact/signals";
import { api, type UpdateStatus } from "../lib/api";
import styles from "./Updates.module.css";

export default function Updates() {
  // Checked on demand only: opening the settings never calls the network (Scribe also checks once at startup).
  const status = useSignal<UpdateStatus | null>(null);
  const phase = useSignal<"idle" | "checking" | "installing">("idle");
  const error = useSignal<string | null>(null);

  async function check() {
    phase.value = "checking";
    error.value = null;
    try {
      status.value = await api.checkUpdate();
    } catch (e) {
      error.value = `Vérification impossible : ${e}`;
    } finally {
      phase.value = "idle";
    }
  }

  async function install() {
    phase.value = "installing";
    error.value = null;
    try {
      // Scribe restarts on success: this only returns on failure.
      await api.installUpdate();
    } catch (e) {
      error.value = `Installation impossible : ${e}`;
      phase.value = "idle";
    }
  }

  const current = status.value;
  const next = current?.available;
  return (
    <section class={styles.section}>
      <h2>Mises à jour</h2>
      <div class={styles.row}>
        <button onClick={check} disabled={phase.value !== "idle"}>
          {phase.value === "checking" ? "Recherche…" : "Rechercher des mises à jour"}
        </button>
        {current && !current.available && <span role="status">Scribe {current.current} est à jour.</span>}
      </div>
      {current && next && (
        <div class={styles.available} role="status">
          <p>
            Scribe <strong>{next.version}</strong> est disponible (version actuelle&nbsp;: {current.current}).
          </p>
          {next.notes && <pre class={styles.notes}>{next.notes}</pre>}
          <button class="primary" onClick={install} disabled={phase.value !== "idle"}>
            {phase.value === "installing" ? "Installation… Scribe va redémarrer" : "Installer et redémarrer"}
          </button>
        </div>
      )}
      {error.value && (
        <p class={styles.error} role="alert">
          {error.value}
        </p>
      )}
    </section>
  );
}
