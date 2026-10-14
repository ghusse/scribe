import { useSignal } from "@preact/signals";
import { api, type UpdateStatus } from "../lib/api";
import { errorText } from "../lib/errors";
import { t } from "../lib/i18n";
import styles from "./Updates.module.css";

export default function Updates() {
  // Checked on demand only: opening the settings never calls the network (Scribe also checks once at startup).
  const status = useSignal<UpdateStatus | null>(null);
  const phase = useSignal<"idle" | "checking" | "installing">("idle");
  const error = useSignal<{ step: "check" | "install"; error: unknown } | null>(null);

  async function check() {
    phase.value = "checking";
    error.value = null;
    try {
      status.value = await api.checkUpdate();
    } catch (e) {
      error.value = { step: "check", error: e };
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
      error.value = { step: "install", error: e };
      phase.value = "idle";
    }
  }

  const current = status.value;
  const next = current?.available;
  const m = t.value.settings.updates;
  const failure = error.value;
  return (
    <section class={styles.section}>
      <h3>{m.heading}</h3>
      <div class={styles.row}>
        <button onClick={check} disabled={phase.value !== "idle"}>
          {phase.value === "checking" ? m.checking : m.check}
        </button>
        {current && !current.available && <span role="status">{m.upToDate(current.current)}</span>}
      </div>
      {current && next && (
        <div class={styles.available} role="status">
          <p>
            {m.availableBefore}
            <strong>{next.version}</strong>
            {m.availableAfter(current.current)}
          </p>
          {next.notes && <pre class={styles.notes}>{next.notes}</pre>}
          <button class="primary" onClick={install} disabled={phase.value !== "idle"}>
            {phase.value === "installing" ? m.installing : m.install}
          </button>
        </div>
      )}
      {failure && (
        <p class={styles.error} role="alert">
          {(failure.step === "check" ? m.checkFailed : m.installFailed)(errorText(failure.error))}
        </p>
      )}
    </section>
  );
}
