# Couper le son pendant la dictée — conception

## But

Pendant qu'on dicte, tout ce qui fait du son sur l'ordinateur (musique, vidéos, notifications, appels…) est coupé,
pour que les haut-parleurs ne perturbent pas la transcription. Windows et macOS.

## Décisions

- Réglage `mute_audio_during_dictation: bool`, **activé par défaut** (y compris pour un `settings.json` existant
  qui n'a pas le champ, via `#[serde(default)]`). Case à cocher dans les réglages : « Couper le son de l'ordinateur
  pendant la dictée ». Lu au début de chaque enregistrement.
- On coupe **toutes les sorties audio actives**, casque compris (pas de détection casque/haut-parleurs). Pas de
  mise en pause des lecteurs.
- Coupure juste après l'ouverture réussie du micro (`SessionAction::BeginRecording`, `Ok`). Pas de coupure si le
  micro n'a pas pu s'ouvrir.
- Rétablissement **dès la fin de l'enregistrement** (`FinishRecording` après `stop()` du micro, `DiscardRecording`,
  pause, touche perdue), pas après le collage.
- On ne rétablit que les sorties **que Scribe a coupées** et qui sont **encore coupées**. Une sortie déjà coupée
  par l'utilisateur n'est pas touchée ; une sortie disparue entre-temps n'est pas une erreur.
- Un échec (API audio indisponible, sortie qui refuse) n'empêche jamais la dictée : il est journalisé
  (`tracing::warn!`), sans toast.
- Reprise après plantage : la liste des sorties coupées est écrite dans `<app_data_dir>/muted_outputs.json` au
  moment de la coupure, avec l'heure de démarrage du système (`{ "boot_ms", "outputs" }`), et supprimée au
  rétablissement. Au lancement, si le fichier existe et date de la même session (heures de démarrage à moins d'une
  minute près ; inconnue d'un côté = même session), on rétablit ces sorties (même règle « encore coupée ») ; dans
  tous les cas on le supprime ; un fichier illisible est supprimé avec un avertissement. Après un redémarrage, on ne
  touche à rien : l'utilisateur a pu entre-temps les couper volontairement.
  Limite connue : dans la même session, une sortie que l'utilisateur a réactivée puis recoupée volontairement avant
  de relancer Scribe est réactivée quand même (on ne sait pas qui l'a coupée).
- Quitter Scribe (menu « Quitter ») ou installer une mise à jour pendant un enregistrement : le son est rétabli
  avant que le processus se termine (attente bornée à 2 s, pour qu'une API audio bloquée n'empêche pas de quitter).

## Couche plateforme (`crates/scribe-platform`)

- `output_mute.rs` (testé) :
  - `trait OutputBackend: Send + Sync` : `active_outputs() -> Result<Vec<String>, String>`,
    `is_muted(&str) -> Result<bool, String>`, `set_muted(&str, bool) -> Result<(), String>` ; un appel système par
    méthode. Identifiants stables entre deux lancements.
  - `OutputMuter<B>` : `mute_all() -> Vec<String>` (coupe les sorties non coupées, renvoie celles coupées ; une
    sortie en erreur est sautée et journalisée) et `restore(&[String])` (ne réactive que les sorties encore coupées ;
    erreurs journalisées, on continue). Implémente le trait `scribe_core` ci-dessous.
- Trait dans `scribe-core` (à côté de `insert::Clipboard`) : `SystemMute { fn mute_all(&self) -> Vec<String>;
  fn restore(&self, ids: &[String]); }`.
- `windows/audio_output.rs` (exclu, regex `windows.` existante) : `IMMDeviceEnumerator::EnumAudioEndpoints(eRender,
  DEVICE_STATE_ACTIVE)`, `IMMDevice::GetId`, `IAudioEndpointVolume::{GetMute, SetMute}` ; `CoInitializeEx` à chaque
  appel (thread quelconque). Features `Win32_Media_Audio`, `Win32_Media_Audio_Endpoints` (+ ce qu'il faut).
- `macos/audio_output.rs` (exclu, regex `macos.` existante) : `kAudioHardwarePropertyDevices`, filtrées sur celles
  qui ont des canaux de sortie, UID `kAudioDevicePropertyDeviceUID`, `kAudioDevicePropertyMute` (scope sortie,
  élément principal) ; une sortie sans propriété mute renvoie une erreur (sautée). Crate `objc2-core-audio`
  (famille objc2 0.3). Pas de repli « volume à 0 » en V1.
- `fallback.rs` : `NoOutputs` (aucune sortie). Point d'entrée `output_backend()` (ou `system_mute()`) réexporté par
  `lib.rs` depuis `windows`/`macos`/`fallback`, comme `focus_detector()`.
- `boot_time_ms() -> Option<u64>` (heure de démarrage du système, ms Unix), réexporté de la même façon : Windows
  horloge moins `GetTickCount64`, macOS `kern.boottime`, `None` ailleurs.

## Application (`src-tauri`)

- `Services` reçoit `audio_mute: AudioMute` (le thread de travail ci-dessous, créé dans `main.rs` avec
  `system_mute()`, `muted_outputs_path` et `boot_time_ms()`) ; `AppPaths` reçoit `muted_outputs_path`.
- Nouveau module testé (ex. `audio_mute.rs`) :
  - `MuteGuard` : créé au début d'un enregistrement (si le réglage est actif), rétablit au `Drop`. Il est rangé
    dans `Controller::recording` avec le handle du micro, si bien que chaque chemin qui fait `recording.take()`
    rétablit le son. Dans `FinishRecording`, le guard est relâché **après** `stop()` du micro.
  - Le thread contrôleur ne doit jamais attendre l'API audio (il cadence les gestes : double-tap, ticks) : la
    coupure et le rétablissement s'exécutent sur un thread de travail dédié, **dans l'ordre** (une file de commandes
    mute / restore), qui tient aussi le fichier de reprise.
  - `recover(...)` au lancement : `main.rs` l'envoie au thread de travail avant le démarrage du contrôleur (donc
    avant toute coupure).
  - `restore_before_exit(timeout)` : rétablit et attend (borné). Appelé sur `RunEvent::Exit` dans `main.rs` et
    avant l'installation d'une mise à jour (`commands::install_update`) ; le guard de l'enregistrement n'a ensuite
    plus rien à rétablir.
- Fakes dans `testing.rs` ; tests : coupure si réglage actif, pas de coupure si inactif ou micro en erreur,
  rétablissement sur finish / discard / pause / touche perdue, ordre stop micro → rétablissement, fichier écrit puis
  supprimé, reprise au lancement (fichier présent, absent, illisible, autre session), rétablissement avant de
  quitter ou de mettre à jour (et API audio bloquée).

## UI

- `src/lib/api.ts` (type `Settings`), `tests/fixtures.ts`, case à cocher dans `src/main/Settings.svelte` avec un test
  de composant (le réglage est sauvegardé).

## Politique de couverture

`CLAUDE.md` : ajouter `windows/audio_output.rs` et `macos/audio_output.rs` à la liste des fichiers exclus, au
tableau et aux « remaining branches » ; `tests/coverage-policy.test.ts` doit passer.

## Hors périmètre

Détection casque / haut-parleurs, pause des lecteurs, repli « volume à 0 » sur macOS.
