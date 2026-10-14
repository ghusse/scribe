import { listen } from "@tauri-apps/api/event";
import { useSignal } from "@preact/signals";
import { useEffect } from "preact/hooks";
import { api } from "../lib/api";
import { needsSetup } from "../lib/apiKeys";
import { needsAttention, type SaveState } from "../lib/autosave";
import { t, useLanguage } from "../lib/i18n";
import History from "./History";
import Permissions from "./Permissions";
import Glossary from "./Glossary";
import Settings from "./Settings";
import styles from "./App.module.css";

type Tab = "history" | "glossary" | "settings";
const TABS: Tab[] = ["history", "glossary", "settings"];

export default function App() {
  useLanguage();
  const tab = useSignal<Tab>("history");
  // Set by the toast's « Voir » button; consumed by History once it has scrolled to the card.
  const focusRequest = useSignal<{ id: number } | null>(null);
  // Settings stays mounted (hidden) so its edits, drafts and errors survive a tab switch.
  const settingsStatus = useSignal<SaveState>("saved");

  useEffect(() => {
    // Listened here (always mounted) rather than in History, which may not be mounted.
    const unFocus = listen<number>("focus-dictation", (e) => {
      focusRequest.value = { id: e.payload };
      tab.value = "history";
    });
    // First launch: without the API keys, open on the settings tab.
    // Same rule as needs_setup in src-tauri/src/bootstrap.rs.
    Promise.all([api.getSettings(), api.keyStatus()])
      .then(([st, k]) => {
        if (needsSetup(st, k) && tab.value === "history" && focusRequest.value === null) tab.value = "settings";
      })
      .catch(() => {});
    return () => {
      unFocus.then((f) => f());
    };
  }, []);

  return (
    <>
      <nav class={styles.nav}>
        {TABS.map((id) => (
          <button key={id} class={tab.value === id ? styles.active : undefined} onClick={() => (tab.value = id)}>
            {t.value.app.tabs[id]}
            {id === "settings" && needsAttention(settingsStatus.value) && (
              <span class={styles.dot} role="img" aria-label={t.value.app.unsavedLabel} title={t.value.app.unsavedTitle}></span>
            )}
          </button>
        ))}
      </nav>
      <main class={styles.main}>
        <Permissions />
        {tab.value === "history" ? (
          <History focus={focusRequest.value} onFocused={() => (focusRequest.value = null)} />
        ) : tab.value === "glossary" ? (
          <Glossary />
        ) : null}
        <div hidden={tab.value !== "settings"}>
          <Settings
            active={tab.value === "settings"}
            status={settingsStatus.value}
            onStatusChange={(s) => (settingsStatus.value = s)}
          />
        </div>
      </main>
    </>
  );
}
