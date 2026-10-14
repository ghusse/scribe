import { useSignal } from "@preact/signals";
import type { StatusView } from "../lib/autosave";
import { t } from "../lib/i18n";
import { cx } from "../lib/ui";
import styles from "./Settings.module.css";

export interface SaveStatusBarProps {
  view: StatusView;
  /** Bumped at each save: replays the ✓ animation. */
  savedFlash: number;
  onRetry: () => void;
}

/** Sticky bar at the bottom of the settings: progress, a success that fades out, errors that stay. */
export default function SaveStatusBar({ view: b, savedFlash, onRetry }: SaveStatusBarProps) {
  const showDetails = useSignal(false);
  const m = t.value.settings;
  return (
    <div class={cx(styles.statusbar, styles[b.tone], !b.visible && styles.hidden)}>
      <span class={styles.msg} role="status">
        {!b.alert && (
          <>
            {b.tone === "muted" ? (
              <span class={styles.spinner}></span>
            ) : (
              <span key={savedFlash} class={styles.tick}>
                ✓
              </span>
            )}
            {b.tone === "muted" ? b.text : b.text.replace(/^✓ /, "")}
          </>
        )}
      </span>
      <span class={cx(styles.msg, styles.alert)} role="alert" title={b.alert ? b.text : undefined}>
        {b.alert ? b.text : ""}
      </span>
      {b.details && (
        <button
          type="button"
          class={styles.small}
          aria-expanded={showDetails.value}
          onClick={() => (showDetails.value = !showDetails.value)}
        >
          {m.details}
        </button>
      )}
      {b.retry && (
        <button
          type="button"
          class={cx(styles.small, "primary")}
          onClick={() => {
            showDetails.value = false;
            onRetry();
          }}
        >
          {m.retry}
        </button>
      )}
      {b.details && showDetails.value && <pre class={cx(styles.pre, styles.details)}>{b.details}</pre>}
    </div>
  );
}
