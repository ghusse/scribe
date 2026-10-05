# Scribe — Spécification de conception (MVP)

Date : 2026-10-05
Statut : en revue

## 1. Objectif

Application de dictée vocale personnelle pour macOS et Windows :
parler au micro via un raccourci clavier global, obtenir un texte transcrit, corrigé selon le
vocabulaire de l'utilisateur, et inséré dans le champ texte actif.

**Utilisateur cible** : usage personnel (un seul utilisateur), dictée en français et anglais
mélangés dans une même phrase, vocabulaire technique/métier.

**Critères de succès**
- Une dictée courte (≤ 10 s) est insérée en ~2 s après le relâchement de la touche dans le cas nominal.
- Le vocabulaire de domaine est correctement orthographié une fois présent dans le glossaire.
- Aucune dictée n'est jamais perdue (audio conservé, texte récupérable).
- Fonctionne sur Mac Apple Silicon, Mac Intel et PC Windows sans GPU.

## 2. Périmètre

### 2.1 Dans le MVP
- Raccourci global configurable (y compris modificateur seul). Gestes configurables ;
  défaut : maintien = push-to-talk, double-tap = mode verrouillé (nouvel appui pour arrêter).
- Enregistrement micro, transcription cloud, langue en détection automatique.
- Post-traitement LLM à 3 niveaux configurables : `raw` (aucun), `clean` (vocabulaire + ponctuation
  + suppression des hésitations et faux départs), `formatted` (clean + mise en forme selon l'app active).
- Insertion par presse-papier + Cmd/Ctrl+V simulé, puis restauration du presse-papier précédent.
- Si aucun champ éditable : texte laissé dans le presse-papier + notification donnant accès au texte.
- Si focus incertain : collage + restauration + notification discrète « Copier ».
- Historique : audio, texte brut, texte final, texte édité ; recherche ; copie ; édition ; retranscription.
- Glossaire personnel : saisie manuelle, apprentissage par correction dans l'historique,
  suggestions automatiques extraites par LLM (toujours validées par l'utilisateur).
- Banc d'essai : rejouer des dictées de l'historique sur plusieurs fournisseurs, comparer texte et latence.
- Pastille flottante d'enregistrement (niveau sonore, état « traitement »), sans prise de focus.
- Icône barre de menus / zone de notification (historique, réglages, pause).
- Onboarding des permissions (Micro, Accessibilité sur macOS ; Micro sur Windows).
- Clés API stockées dans le trousseau système.
- Purge automatique configurable de l'audio (âge et/ou taille totale).

### 2.2 Hors MVP (l'architecture doit le permettre)
- Transcription locale (whisper.cpp / Parakeet).
- Import de documents pour enrichir le glossaire.
- Transcription en streaming pendant la dictée.
- Détection des retouches faites directement dans l'application cible.
- Commandes vocales.
- Signature/notarisation pour distribution publique (signature ad-hoc suffisante en usage perso).
- File d'attente de dictées (un appui pendant le traitement est ignoré).

## 3. Choix technologiques

| Domaine | Choix |
|---|---|
| Framework | Tauri v2 (cœur Rust, UI web) |
| UI | Svelte |
| Audio | `cpal`, PCM 16 kHz mono, encodage Opus pour stockage/envoi |
| Hook clavier | macOS : `CGEventTap` ; Windows : `SetWindowsHookEx(WH_KEYBOARD_LL)` |
| Focus / accessibilité | macOS : `AXUIElement` (via `objc2`/bindings) ; Windows : UI Automation (crate `windows`) |
| Simulation de touches | macOS : `CGEvent` ; Windows : `SendInput` |
| Stockage | SQLite via `rusqlite`, migrations SQL manuelles |
| Réglages | `tauri-plugin-store` (JSON) |
| Secrets | crate `keyring` (Keychain / Credential Manager) |
| HTTP | `reqwest` |
| Notifications | `tauri-plugin-notification` |

**Fournisseurs**
- Transcription : adaptateur « compatible OpenAI » (OpenAI `gpt-4o-transcribe`, Groq Whisper large v3
  turbo, Mistral Voxtral via URL + modèle configurables) + un adaptateur spécialiste multilingue/mots-clés
  (ElevenLabs Scribe, Soniox ou Deepgram — choix arrêté pendant la planification après vérification
  de la gestion du code-switching FR/EN et de l'injection de mots-clés).
- Correction : Anthropic Claude, appelé en HTTP brut (pas de SDK Rust officiel). Modèle par défaut
  `claude-opus-5-5` avec `output_config.effort: "low"` ; modèle configurable (ex. `claude-haiku-4-5`
  si la latence mesurée dépasse le budget). Prompt caching sur le préfixe stable
  (instructions + glossaire).

## 4. Architecture

```
src-tauri/src/
  main.rs               bootstrap, tray, fenêtres
  app_state.rs          machine à états de la session de dictée
  hotkey/gesture.rs     (partagé) événements bruts → gestes
  hotkey/macos.rs       CGEventTap
  hotkey/windows.rs     WH_KEYBOARD_LL
  audio/recorder.rs     capture + encodage
  pipeline/transcriber.rs   trait Transcriber + adaptateurs
  pipeline/corrector.rs     trait Corrector + adaptateur Anthropic
  pipeline/prompt.rs        construction des prompts, sélection des hints
  output/focus/{macos,windows}.rs   trait FocusDetector
  output/inserter.rs    presse-papier, collage, restauration
  output/notifier.rs    notifications
  learning/glossary.rs  CRUD glossaire
  learning/diff_learner.rs  correction utilisateur → paires candidates
  learning/miner.rs     extraction périodique de termes via LLM
  storage/db.rs         SQLite
  bench.rs              banc d'essai
  secrets.rs            trousseau
src/
  overlay/              pastille d'enregistrement
  main/                 Historique · Vocabulaire · Suggestions · Banc d'essai · Réglages
```

### 4.1 Interfaces

```rust
trait Transcriber { async fn transcribe(&self, audio: &Audio, hints: &[String]) -> Result<RawTranscript>; }
trait Corrector   { async fn correct(&self, raw: &str, glossary: &[Term], ctx: &AppContext, level: Level) -> Result<String>; }
trait FocusDetector { fn snapshot(&self) -> FocusSnapshot; } // { app_name, bundle_id, window_id, state: Editable | NotEditable | Unknown }
trait TextInserter  { fn insert(&self, text: &str, focus: &FocusSnapshot) -> InsertOutcome; }
```

### 4.2 Principes
- Le pipeline n'a aucune dépendance OS ; tout code système est derrière des traits et `#[cfg(target_os)]`.
- Machine à états unique : `Idle → Recording(mode) → Processing → Idle`. UI et pastille l'observent via
  événements Tauri. Transition `Processing → Recording` refusée.
- Les hooks clavier ne produisent que `KeyDown(t)` / `KeyUp(t)` et les poussent dans un canal ;
  `gesture.rs` est une fonction pure du temps, testable sans OS.
- Le callback de hook ne fait aucun travail lourd (sur macOS, un tap lent est désactivé par l'OS ;
  réactivation sur `tapDisabledByTimeout`).
- La pastille est une fenêtre non focusable, toujours au premier plan, sans décoration
  (panel non activant sur macOS).
- L'enregistrement démarre dès le premier appui (démarrage optimiste), le geste étant résolu ensuite.

## 5. Flux d'une dictée

1. Appui → `gesture.rs` émet `StartRecording(mode)`.
2. `FocusDetector.snapshot()` capturé **au début** ; démarrage de l'enregistrement ; pastille « écoute ».
3. Relâchement (push-to-talk) ou nouvel appui (verrouillé) → `StopRecording`.
4. Si durée < ~300 ms ou silence détecté → abandon silencieux.
5. Pastille « traitement ». `Transcriber` avec hints = sélection de termes du glossaire.
6. Si niveau ≠ `raw` : `Corrector` avec glossaire complet, app active, niveau.
7. Nouveau snapshot de focus. Si la fenêtre a changé depuis l'étape 2 → presse-papier + notification.
8. Sinon selon l'état :
   - `Editable` : sauvegarde presse-papier, écriture du texte, Cmd/Ctrl+V, attente 100–200 ms, restauration.
   - `Unknown` : idem + notification discrète « Texte inséré ? Copier ».
   - `NotEditable` : texte laissé dans le presse-papier + notification « Texte copié ».
9. Enregistrement en base : audio, raw, final, app, fournisseurs, latences, outcome.

**Restauration du presse-papier** : le MVP sauvegarde/restaure texte et image. Pour tout autre
format, pas de restauration (log).

**Sélection des hints pour le transcripteur** : budget limité (~200 tokens ou liste de mots-clés
selon le fournisseur) ; les termes sont classés (fréquence d'usage, récence, association à l'app
active). Le correcteur reçoit le glossaire complet.

### 5.1 Boucle d'apprentissage (asynchrone)
- **Correction** : l'édition du texte final dans l'historique déclenche `diff_learner` (diff au niveau
  des mots raw/final/édité) → suggestions « terme (entendu : variante) ».
- **Extraction** : `miner` s'exécute toutes les 50 dictées ou une fois par jour ; envoie le lot récent
  au LLM → suggestions de termes de domaine.
- Toute suggestion requiert validation. Les suggestions rejetées sont mémorisées et non reproposées.

## 6. Modèle de données

```sql
dictations (
  id INTEGER PRIMARY KEY, created_at TEXT NOT NULL,
  mode TEXT NOT NULL,              -- 'hold' | 'locked'
  app_name TEXT, app_bundle_id TEXT,
  audio_path TEXT,                 -- NULL après purge
  duration_ms INTEGER NOT NULL,
  raw_text TEXT, final_text TEXT, edited_text TEXT,
  level TEXT NOT NULL,             -- 'raw' | 'clean' | 'formatted'
  transcriber TEXT, corrector TEXT,
  stt_ms INTEGER, llm_ms INTEGER,
  outcome TEXT NOT NULL,           -- 'pasted' | 'pasted_uncertain' | 'clipboard' | 'error'
  error TEXT
);

glossary_terms (
  id INTEGER PRIMARY KEY, term TEXT NOT NULL UNIQUE,
  variants_json TEXT NOT NULL DEFAULT '[]',
  note TEXT,
  source TEXT NOT NULL,            -- 'manual' | 'correction' | 'mined'
  use_count INTEGER NOT NULL DEFAULT 0, last_used_at TEXT, created_at TEXT NOT NULL
);

suggestions (
  id INTEGER PRIMARY KEY, term TEXT NOT NULL,
  variants_json TEXT NOT NULL DEFAULT '[]',
  evidence_json TEXT NOT NULL DEFAULT '[]',  -- ids de dictées
  source TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',    -- 'pending' | 'accepted' | 'rejected'
  created_at TEXT NOT NULL
);

bench_runs (
  id INTEGER PRIMARY KEY, dictation_id INTEGER NOT NULL REFERENCES dictations(id),
  provider TEXT NOT NULL, model TEXT NOT NULL,
  text TEXT, latency_ms INTEGER, created_at TEXT NOT NULL
);
```

Audios : fichiers `.opus` dans le dossier de données de l'application. Réglages : JSON via
`tauri-plugin-store`. Clés API : trousseau système uniquement.

## 7. Gestion des erreurs

Règle : une dictée n'est jamais perdue.

| Situation | Comportement |
|---|---|
| Permission manquante | Icône en alerte + onboarding au démarrage ; notification explicative en cours de dictée |
| Micro absent / débranché | Arrêt, audio partiel conservé, notification |
| Échec transcription (réseau, 5xx, 429) | 1 retry court, puis `outcome=error` + audio conservé ; notification « Réessayer » ; bouton « Retranscrire » dans l'historique |
| Échec/timeout LLM (~3 s) | Repli sur le texte brut, inséré normalement ; notification discrète « non corrigé » |
| Clé API absente/invalide | Validation à la saisie dans les réglages ; notification explicite |
| Collage impossible | Repli presse-papier + notification |
| Appui pendant `Processing` | Ignoré |
| Hook désactivé par l'OS | Réactivation automatique |

## 8. Tests

- **Unitaires (Rust pur)** : `gesture.rs` (séquences horodatées → gestes), machine à états,
  `prompt.rs` et sélection des hints, `diff_learner.rs`, décisions de l'inserter avec faux
  presse-papier et faux `FocusDetector`.
- **Pipeline** : `FakeTranscriber` / `FakeCorrector`, y compris erreurs, timeouts et replis.
- **Contrat des adaptateurs** : serveur HTTP simulé (`wiremock`).
- **Réel optionnel** : exécuté seulement si les clés API sont présentes en variables d'environnement ;
  quelques audios de référence, vérification de la présence des termes du glossaire.
- **Checklist manuelle par OS** : focus dans Slack, VS Code, Chrome, Word, terminal ; pastille sans
  vol de focus ; restauration du presse-papier ; permissions.

## 9. Risques identifiés

- **Détection du champ éditable** peu fiable dans les apps Electron/terminaux → atténué par le cas
  `Unknown` (collage + notification).
- **Latence du LLM** au-delà du budget → mesurée dès le premier prototype ; modèle configurable.
- **Touche `Fn` sur macOS** interceptée par le système (emoji/dictée) → documenter le réglage
  système à modifier ; proposer d'autres touches par défaut si nécessaire.
- **Restauration du presse-papier** trop rapide → délai configurable.
