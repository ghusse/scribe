//! Muting the audio outputs while recording, so the speakers do not end up in the dictation.
//!
//! The controller thread paces the gestures (double-tap, ticks) and must never wait on the audio API: muting and
//! restoring run on a dedicated worker, in the order they were asked for. The worker also keeps the recovery file,
//! the outputs it muted, so a crash while recording does not leave them muted (`recover` at the next launch, in
//! the same boot session only). Quitting or updating while recording restores them first (`restore_before_exit`).
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use scribe_core::insert::SystemMute;

/// How long quitting waits for the outputs to be restored: a stuck audio API must not keep Scribe from closing.
pub const EXIT_RESTORE_TIMEOUT: Duration = Duration::from_secs(2);

/// Two boot times (wall clock minus uptime) closer than this are the same boot session: they drift only with
/// clock corrections, while two boots are minutes apart.
const SAME_BOOT_TOLERANCE_MS: u64 = 60_000;

enum Command {
    Mute,
    Restore,
    Recover,
    /// Answers once every command sent before it has run.
    Flush(Sender<()>),
}

/// The recovery file: the outputs Scribe muted and the boot session it muted them in.
#[derive(Serialize, Deserialize)]
struct Recovery {
    /// Unix time of the system boot (ms), `None` when the platform cannot tell.
    boot_ms: Option<u64>,
    outputs: Vec<String>,
}

/// The handle on the mute worker. Dropping every handle and guard ends the worker.
#[derive(Clone)]
pub struct AudioMute {
    tx: Sender<Command>,
}

impl AudioMute {
    /// `boot_ms`: when the system booted (`scribe_platform::boot_time_ms`), saved with the muted outputs.
    pub fn spawn(mute: Arc<dyn SystemMute>, recovery_path: PathBuf, boot_ms: Option<u64>) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("scribe-audio-mute".into())
            .spawn(move || run(mute.as_ref(), &recovery_path, boot_ms, rx))
            .expect("audio mute thread");
        Self { tx }
    }

    /// Mutes the outputs now (without waiting); they are restored when the guard is dropped.
    pub fn guard(&self) -> MuteGuard {
        let _ = self.tx.send(Command::Mute);
        MuteGuard { tx: self.tx.clone() }
    }

    /// At launch, before the controller starts: restores what a crashed run left muted (see `recover`), on the
    /// worker, before any later mute.
    pub fn recover(&self) {
        let _ = self.tx.send(Command::Recover);
    }

    /// Before the process ends (quit, update) while a recording may hold a guard: restores the outputs now and
    /// waits for it, at most `timeout`. Returns whether the restore ran. The guard, dropped later or never, then
    /// has nothing left to restore.
    pub fn restore_before_exit(&self, timeout: Duration) -> bool {
        let _ = self.tx.send(Command::Restore);
        let (tx, rx) = mpsc::channel();
        let _ = self.tx.send(Command::Flush(tx));
        rx.recv_timeout(timeout).is_ok()
    }

    /// Waits until every mute and restore asked so far has run.
    #[cfg(test)]
    pub fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        let _ = self.tx.send(Command::Flush(tx));
        let _ = rx.recv();
    }
}

/// Outputs muted for one recording; kept next to the microphone handle, restored on drop.
pub struct MuteGuard {
    tx: Sender<Command>,
}

impl Drop for MuteGuard {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Restore);
    }
}

fn run(mute: &dyn SystemMute, path: &Path, boot_ms: Option<u64>, rx: Receiver<Command>) {
    // What Scribe muted and has not restored yet.
    let mut muted: Vec<String> = Vec::new();
    for cmd in rx {
        match cmd {
            Command::Mute => {
                muted.extend(mute.mute_all());
                if !muted.is_empty() {
                    write_recovery(path, &Recovery { boot_ms, outputs: muted.clone() });
                }
            }
            Command::Restore => {
                if muted.is_empty() {
                    continue;
                }
                mute.restore(&muted);
                muted.clear();
                remove_recovery(path);
            }
            Command::Recover => recover(mute, path, boot_ms),
            Command::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}

fn write_recovery(path: &Path, recovery: &Recovery) {
    let json = serde_json::to_vec(recovery).expect("serialize output ids");
    if let Err(e) = std::fs::write(path, json) {
        tracing::warn!("sorties audio coupées non enregistrées ({e}) : un plantage les laisserait coupées");
    }
}

fn remove_recovery(path: &Path) {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            tracing::warn!("fichier de reprise du son non supprimé ({e})");
        }
        _ => {}
    }
}

/// Whether outputs muted in boot session `saved` may still be muted by Scribe now (`current`). After a reboot the
/// user has had every chance to notice and to mute them again on purpose: they are left alone. Unknown on either
/// side: assumed the same (the outputs are restored).
fn same_boot(saved: Option<u64>, current: Option<u64>) -> bool {
    match (saved, current) {
        (Some(a), Some(b)) => a.abs_diff(b) <= SAME_BOOT_TOLERANCE_MS,
        _ => true,
    }
}

/// Restores the outputs a crashed run left muted (only those still muted) if it crashed in this boot session
/// (`boot_ms`), then forgets them. An unreadable file is dropped with a warning.
///
/// Limit: an output the user unmuted after the crash, then muted again on purpose before Scribe is relaunched in
/// the same boot session, is unmuted all the same (Scribe cannot tell who muted it).
pub fn recover(mute: &dyn SystemMute, path: &Path, boot_ms: Option<u64>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    match serde_json::from_str::<Recovery>(&text) {
        Ok(r) if same_boot(r.boot_ms, boot_ms) => {
            tracing::info!("rétablissement du son coupé par la dernière dictée ({} sortie(s))", r.outputs.len());
            mute.restore(&r.outputs);
        }
        Ok(r) => tracing::info!(
            "son coupé par une dictée avant le redémarrage ({} sortie(s)) : laissé tel quel",
            r.outputs.len()
        ),
        Err(e) => tracing::warn!("fichier de reprise du son illisible ({e}), ignoré"),
    }
    remove_recovery(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeMute, MuteCall};

    const BOOT: Option<u64> = Some(1_700_000_000_000);

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn setup() -> (Arc<FakeMute>, tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("muted_outputs.json");
        (Arc::new(FakeMute::default()), dir, path)
    }

    fn file(path: &Path) -> Option<Recovery> {
        std::fs::read_to_string(path).ok().map(|t| serde_json::from_str(&t).unwrap())
    }

    fn outputs(path: &Path) -> Option<Vec<String>> {
        file(path).map(|r| r.outputs)
    }

    fn write_file(path: &Path, boot_ms: Option<u64>, outputs: &[&str]) {
        write_recovery(path, &Recovery { boot_ms, outputs: ids(outputs) });
    }

    #[test]
    fn a_guard_mutes_then_restores_what_it_muted_and_keeps_the_recovery_file_meanwhile() {
        let (fake, _dir, path) = setup();
        let audio = AudioMute::spawn(fake.clone(), path.clone(), BOOT);
        let guard = audio.guard();
        audio.flush();
        assert_eq!(fake.calls(), vec![MuteCall::MuteAll]);
        assert_eq!(outputs(&path), Some(ids(&["haut-parleurs", "casque"])));
        assert_eq!(file(&path).unwrap().boot_ms, BOOT, "the boot session is saved with the outputs");
        drop(guard);
        audio.flush();
        assert_eq!(fake.calls(), vec![MuteCall::MuteAll, MuteCall::Restore(ids(&["haut-parleurs", "casque"]))]);
        assert_eq!(outputs(&path), None, "removed once restored");
    }

    #[test]
    fn nothing_muted_means_no_file_and_nothing_to_restore() {
        let (fake, _dir, path) = setup();
        fake.outputs.lock().unwrap().clear();
        let audio = AudioMute::spawn(fake.clone(), path.clone(), BOOT);
        drop(audio.guard());
        audio.flush();
        assert_eq!(fake.calls(), vec![MuteCall::MuteAll]);
        assert!(!path.exists());
    }

    #[test]
    fn commands_run_in_order_one_recording_after_the_other() {
        let (fake, _dir, path) = setup();
        let audio = AudioMute::spawn(fake.clone(), path.clone(), BOOT);
        drop(audio.guard());
        audio.flush();
        *fake.outputs.lock().unwrap() = ids(&["casque"]);
        let second = audio.guard();
        audio.flush();
        assert_eq!(outputs(&path), Some(ids(&["casque"])));
        drop(second);
        audio.flush();
        let all = ids(&["haut-parleurs", "casque"]);
        assert_eq!(
            fake.calls(),
            vec![MuteCall::MuteAll, MuteCall::Restore(all), MuteCall::MuteAll, MuteCall::Restore(ids(&["casque"]))]
        );
    }

    #[test]
    fn restoring_before_exit_waits_for_the_restore_and_leaves_nothing_for_the_guard() {
        let (fake, _dir, path) = setup();
        let audio = AudioMute::spawn(fake.clone(), path.clone(), BOOT);
        // The recording still holds its guard when Scribe quits.
        let guard = audio.guard();
        assert!(audio.restore_before_exit(EXIT_RESTORE_TIMEOUT));
        let restored = vec![MuteCall::MuteAll, MuteCall::Restore(ids(&["haut-parleurs", "casque"]))];
        assert_eq!(fake.calls(), restored, "restored before it returns");
        assert!(!path.exists(), "nothing left to recover");
        drop(guard);
        audio.flush();
        assert_eq!(fake.calls(), restored, "the guard has nothing left to restore");
    }

    #[test]
    fn restoring_before_exit_without_a_recording_touches_nothing() {
        let (fake, _dir, path) = setup();
        let audio = AudioMute::spawn(fake.clone(), path, BOOT);
        assert!(audio.restore_before_exit(EXIT_RESTORE_TIMEOUT));
        assert!(fake.calls().is_empty());
    }

    /// Mutes one output; restoring blocks until `release` is sent.
    struct StuckMute(std::sync::Mutex<Receiver<()>>);

    impl SystemMute for StuckMute {
        fn mute_all(&self) -> Vec<String> {
            ids(&["haut-parleurs"])
        }
        fn restore(&self, _ids: &[String]) {
            let _ = self.0.lock().unwrap().recv();
        }
    }

    #[test]
    fn a_stuck_audio_api_does_not_keep_scribe_from_quitting() {
        let (release, stuck) = mpsc::channel();
        let dir = tempfile::tempdir().unwrap();
        let mute = Arc::new(StuckMute(std::sync::Mutex::new(stuck)));
        let audio = AudioMute::spawn(mute.clone(), dir.path().join("muted_outputs.json"), BOOT);
        let _guard = audio.guard();
        assert!(!audio.restore_before_exit(Duration::from_millis(50)), "gave up waiting");
        release.send(()).unwrap();
        audio.flush();
        assert_eq!(mute.mute_all(), ids(&["haut-parleurs"]));
    }

    #[test]
    fn an_unwritable_recovery_file_does_not_prevent_muting() {
        let (fake, dir, _) = setup();
        let path = dir.path().join("absent").join("muted_outputs.json");
        let audio = AudioMute::spawn(fake.clone(), path.clone(), BOOT);
        drop(audio.guard());
        audio.flush();
        assert_eq!(fake.calls().len(), 2, "muted and restored all the same");
        assert!(!path.exists());
    }

    #[test]
    fn a_recovery_file_that_cannot_be_removed_is_left_behind() {
        let (fake, dir, _) = setup();
        // A directory in place of the file: written fails, removing fails with another error than NotFound.
        let path = dir.path().join("muted_outputs.json");
        std::fs::create_dir(&path).unwrap();
        let audio = AudioMute::spawn(fake.clone(), path.clone(), BOOT);
        drop(audio.guard());
        audio.flush();
        assert_eq!(fake.calls().len(), 2);
        assert!(path.is_dir());
    }

    #[test]
    fn recover_restores_the_outputs_left_muted_in_this_boot_session_then_forgets_them() {
        let (fake, _dir, path) = setup();
        write_file(&path, BOOT, &["haut-parleurs"]);
        recover(fake.as_ref(), &path, BOOT.map(|b| b + 1_500));
        assert_eq!(fake.calls(), vec![MuteCall::Restore(ids(&["haut-parleurs"]))]);
        assert!(!path.exists());
    }

    #[test]
    fn recover_leaves_alone_the_outputs_muted_before_a_reboot_and_forgets_them() {
        let (fake, _dir, path) = setup();
        write_file(&path, BOOT, &["haut-parleurs"]);
        recover(fake.as_ref(), &path, BOOT.map(|b| b + 3_600_000));
        assert!(fake.calls().is_empty(), "the user may have muted them on purpose since");
        assert!(!path.exists());
    }

    #[test]
    fn recover_restores_when_a_boot_session_is_unknown() {
        for (saved, current) in [(None, BOOT), (BOOT, None), (None, None)] {
            let (fake, _dir, path) = setup();
            write_file(&path, saved, &["casque"]);
            recover(fake.as_ref(), &path, current);
            assert_eq!(fake.calls(), vec![MuteCall::Restore(ids(&["casque"]))], "{saved:?} / {current:?}");
        }
    }

    #[test]
    fn same_boot_tolerates_clock_corrections_only() {
        let b = 1_700_000_000_000;
        assert!(same_boot(Some(b), Some(b)));
        assert!(same_boot(Some(b), Some(b - SAME_BOOT_TOLERANCE_MS)));
        assert!(!same_boot(Some(b), Some(b + SAME_BOOT_TOLERANCE_MS + 1)));
    }

    #[test]
    fn recover_runs_on_the_worker_before_the_next_mute() {
        let (fake, _dir, path) = setup();
        write_file(&path, BOOT, &["casque"]);
        let audio = AudioMute::spawn(fake.clone(), path.clone(), BOOT);
        audio.recover();
        let guard = audio.guard();
        audio.flush();
        assert_eq!(fake.calls(), vec![MuteCall::Restore(ids(&["casque"])), MuteCall::MuteAll]);
        assert_eq!(outputs(&path), Some(ids(&["haut-parleurs", "casque"])), "the new recording's file");
        drop(guard);
    }

    #[test]
    fn recover_without_a_file_touches_nothing() {
        let (fake, _dir, path) = setup();
        recover(fake.as_ref(), &path, BOOT);
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn recover_drops_an_unreadable_file() {
        // The second one is the format before the boot session was saved.
        for text in ["{pas du json", r#"["haut-parleurs"]"#] {
            let (fake, _dir, path) = setup();
            std::fs::write(&path, text).unwrap();
            recover(fake.as_ref(), &path, BOOT);
            assert!(fake.calls().is_empty());
            assert!(!path.exists());
        }
    }
}
