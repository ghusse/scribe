# Audit de couverture des tests (2026-10-05)

Couverture Rust de départ (`cargo llvm-cov --workspace`) : **66,9 % des lignes**. scribe-core 96–100 % (sauf
`model.rs` 75 %, `clock.rs` 0 %), scribe-providers 93–100 % (sauf `http.rs` 79 %), scribe-platform 0 % (sauf
`focus_rules`), src-tauri 0–28 % (sauf `settings.rs` 95 %). UI : vitest ajouté pour la logique des réglages
uniquement. Aucun CI.

## Comportements non testés, par risque

| Risque | Où | Comportement (scénario → attendu) | Comment tester | Seam |
|---|---|---|---|---|
| Haut | `dictation.rs::process` | (a) trop court / silence → Idle, ni fichier ni ligne ; (b) `Empty` → .wav supprimé, Idle, pas de ligne ; (c) providers en erreur (clé STT absente) → ligne `Error` + audio conservé + toast Error avec id ; (d) erreur de transcription idem (`raw=None`) ; (e) `Pasted` sans erreur → Idle ; (f) `Pasted` + erreur de correction → toast Info « Inséré sans correction (…) » ; (g) `PastedUncertain` → toast Uncertain (+ note non corrigé) ; (h) `ClipboardOnly`/`PasteFailed` → toast Copied ; (i) `ClipboardFailed` → toast Error, `outcome=Error`, texte en base ; (j) Raw → `corrector=None` ; (k) `bump_term_usage` avec les termes du texte final ; (l) échec d'écriture .wav → `audio_path=None`, la dictée continue ; (m) `history-changed` toujours émis | Fonctions pures `feedback(...)` et `build_record(...)` ; test d'intégration Db mémoire + tempdir + fakes | trait `UiSink` (overlay, history_changed, focus_dictation) à la place d'`AppHandle` ; fabrique de providers injectée dans `Services` |
| Haut | `windows/hook.rs::hook_proc` | Lock avalée (down+up) pendant que la trigger est tenue ; pause → non avalée ; `lock_vk=0` → jamais ; événements injectés ignorés. **Bug probable** : `trigger_down` reste vrai si un key-up est perdu (Win+L) ou si la trigger change pendant l'appui → Espace avalé dans tout le système | struct pure `KeyFilter` hors `cfg(windows)` | extraire `KeyFilter` ; reset quand la trigger change / vérif `GetAsyncKeyState` |
| Haut | `controller.rs` | Cycle appui → Recording → Processing → traitement avec le bon mode ; micro indisponible → abort + reset + toast ; `stop()` en erreur → toast + Idle ; tick > max → Finish + reset ; pause pendant l'enregistrement → Discard ; capture prioritaire ; SettingsChanged met à jour le hook sans casser l'enregistrement ; ProcessingDone → Idle | tests de `Controller::handle` sans threads | traits `Recorder`/`Recording`, `UiSink`, `spawn_processing` injecté, horloge injectable |
| Haut | `overlay.rs` | **Race** dismiss/emit : un toast qui expire pendant qu'on reprend la parole cache la pastille d'enregistrement ; `LAST_KIND` static global | fake `OverlayWindow`, test multi-thread | `OverlayState` sous verrou dans `Services`, trait `OverlayWindow` |
| Haut | `commands.rs::capture_key` | **Bug probable** : deux captures concurrentes s'annulent mutuellement ; timeout → None | type `KeyCapture` avec token | extraire `KeyCapture` |
| Haut | `dictation.rs::retranscribe` | id introuvable, audio purgé, lecture en échec, erreur provider, Empty ; outcome mis à jour | même seam que `process` | — |
| Moyen | `purge_audio` | Purge selon la rétention ; **à noter** : si `remove_file` échoue, le chemin est effacé et le fichier reste orphelin | `purge_audio_in(db, days, now)` + tempdir | extraction |
| Moyen | `main.rs` needs_setup + `App.svelte` | Règle dupliquée Rust/TS qui peut diverger | `needs_setup(&Settings, has_key)` + équivalent TS, même table de cas | extraction ×2 |
| Moyen | `secrets.rs` | Fournisseur inconnu, clé vide → suppression, trim | trait `SecretStore` + fake HashMap | injection dans Services |
| Moyen | `audio_capture.rs` | Conversions i16/u16 → f32, RMS, downmix + resample ; micro débranché → audio partiel conservé, rien capturé → erreur | fonctions pures `finish_clip`, conversions | extraction |
| Moyen | `Overlay.svelte` | Minuteries de toast, boutons selon niveau/id, reset des barres. **Bug probable** : `copy()` puis `dismiss()` peut masquer une pastille d'enregistrement arrivée entre-temps ; rejet non géré | `src/overlay/model.ts` pur + vitest | extraction TS |
| Moyen | `History.svelte` | Réponses de recherche dans le désordre ; texte affiché ; brouillon vide → null ; boutons désactivés | helpers purs + garde `latestOnly()` | extraction TS |
| Moyen | cœur | `bump_term_usage`, aller-retour `as_str/parse`, timeout reqwest → `Timeout` (retenté), 429/5xx retentés et 400 non | tests unitaires ; wiremock `set_delay` | — |
| Moyen | autres commandes | `copy_dictation`, `delete_dictation` (supprime le .wav), `save_settings` invalide → ni écrit ni appliqué, `test_providers` en Raw, `open_history` | logique extraite en fonctions sur `(&Db, &dyn Clipboard, …)` | extraction |
| Bas | `clipboard.rs` | Aller-retour texte, CRLF, image ; presse-papier vide = `Unsupported` | test réservé au CI Windows | — |
| Bas | `tray.rs`, `level_emitter` | Bascule de pause, limitation à 50 ms | `toggle_pause`, `Throttle` purs | extraction |
| Bas | `Glossary.svelte`, `keys.ts` | Variantes (virgules, blancs), `keyName` | helpers purs + vitest | extraction |
