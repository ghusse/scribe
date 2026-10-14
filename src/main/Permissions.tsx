import { useSignal } from "@preact/signals";
import { useEffect } from "preact/hooks";
import { api, type Permission, type PermissionStatus } from "../lib/api";
import { missingPermissions, PERMISSION_COPY, permissionActions } from "../lib/permissions";
import styles from "./Permissions.module.css";

/** Re-read while something is missing: the user grants it in System Settings, outside Scribe. */
const POLL_MS = 1500;

export default function Permissions() {
  const missing = useSignal<PermissionStatus[]>([]);
  const error = useSignal<string | null>(null);

  async function refresh() {
    try {
      missing.value = missingPermissions(await api.permissions());
    } catch {
      // Keep the last known state; the next poll tries again.
    }
  }

  async function request(permission: Permission) {
    error.value = null;
    try {
      missing.value = missingPermissions(await api.requestPermission(permission));
    } catch (e) {
      error.value = `Demande impossible : ${e}`;
    }
  }

  async function openSettings(permission: Permission) {
    error.value = null;
    try {
      await api.openPermissionSettings(permission);
    } catch (e) {
      error.value = `Ouverture des Réglages impossible : ${e}`;
    }
  }

  useEffect(() => {
    refresh();
    const timer = setInterval(() => {
      if (missing.peek().length > 0) refresh();
    }, POLL_MS);
    return () => clearInterval(timer);
  }, []);

  if (missing.value.length === 0) return null;

  return (
    <section class={styles.panel} aria-label="Autorisations manquantes">
      <h2>Scribe a besoin de votre autorisation</h2>
      {missing.value.map((status) => {
        const copy = PERMISSION_COPY[status.permission];
        return (
          <div key={status.permission} class={styles.permission}>
            <h3>{copy.title}</h3>
            <p>{copy.why}</p>
            <p class={styles.how}>{copy.how}</p>
            <div class={styles.row}>
              {permissionActions(status).map((action) =>
                action === "request" ? (
                  <button key={action} class="primary" onClick={() => request(status.permission)}>
                    Autoriser
                  </button>
                ) : (
                  <button key={action} onClick={() => openSettings(status.permission)}>
                    Ouvrir les Réglages Système
                  </button>
                ),
              )}
            </div>
          </div>
        );
      })}
      {error.value && (
        <p class={styles.error} role="alert">
          {error.value}
        </p>
      )}
    </section>
  );
}
