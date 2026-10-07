//! Muting the audio outputs while recording, so the speakers do not end up in the dictation.
//!
//! The controller thread paces the gestures (double-tap, ticks) and must never wait on the audio API: muting and
//! restoring run on a dedicated worker, in the order they were asked for. The worker also keeps the recovery file,
//! the outputs it muted, so a crash while recording does not leave them muted (`recover` at the next launch).
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use scribe_core::insert::SystemMute;

enum Command {
    Mute,
    Restore,
    /// Answers once every command sent before it has run.
    #[cfg(test)]
    Flush(Sender<()>),
}

/// The handle on the mute worker. Dropping every handle and guard ends the worker.
#[derive(Clone)]
pub struct AudioMute {
    tx: Sender<Command>,
}

impl AudioMute {
    pub fn spawn(mute: Arc<dyn SystemMute>, recovery_path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("scribe-audio-mute".into())
            .spawn(move || run(mute.as_ref(), &recovery_path, rx))
            .expect("audio mute thread");
        Self { tx }
    }

    /// Mutes the outputs now (without waiting); they are restored when the guard is dropped.
    pub fn guard(&self) -> MuteGuard {
        let _ = self.tx.send(Command::Mute);
        MuteGuard { tx: self.tx.clone() }
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

fn run(mute: &dyn SystemMute, path: &Path, rx: Receiver<Command>) {
    // What Scribe muted and has not restored yet.
    let mut muted: Vec<String> = Vec::new();
    for cmd in rx {
        match cmd {
            Command::Mute => {
                muted.extend(mute.mute_all());
                if !muted.is_empty() {
                    write_recovery(path, &muted);
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
            #[cfg(test)]
            Command::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}

fn write_recovery(path: &Path, ids: &[String]) {
    let json = serde_json::to_vec(ids).expect("serialize output ids");
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

/// At launch, before the controller starts: restores the outputs a crashed run left muted (only those still
/// muted), then forgets them. An unreadable file is dropped with a warning.
pub fn recover(mute: &dyn SystemMute, path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    match serde_json::from_str::<Vec<String>>(&text) {
        Ok(ids) => {
            tracing::info!("rétablissement du son coupé par la dernière dictée ({} sortie(s))", ids.len());
            mute.restore(&ids);
        }
        Err(e) => tracing::warn!("fichier de reprise du son illisible ({e}), ignoré"),
    }
    remove_recovery(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeMute, MuteCall};

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn setup() -> (Arc<FakeMute>, tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("muted_outputs.json");
        (Arc::new(FakeMute::default()), dir, path)
    }

    fn file(path: &Path) -> Option<Vec<String>> {
        std::fs::read_to_string(path).ok().map(|t| serde_json::from_str(&t).unwrap())
    }

    #[test]
    fn a_guard_mutes_then_restores_what_it_muted_and_keeps_the_recovery_file_meanwhile() {
        let (fake, _dir, path) = setup();
        let audio = AudioMute::spawn(fake.clone(), path.clone());
        let guard = audio.guard();
        audio.flush();
        assert_eq!(fake.calls(), vec![MuteCall::MuteAll]);
        assert_eq!(file(&path), Some(ids(&["haut-parleurs", "casque"])));
        drop(guard);
        audio.flush();
        assert_eq!(fake.calls(), vec![MuteCall::MuteAll, MuteCall::Restore(ids(&["haut-parleurs", "casque"]))]);
        assert_eq!(file(&path), None, "removed once restored");
    }

    #[test]
    fn nothing_muted_means_no_file_and_nothing_to_restore() {
        let (fake, _dir, path) = setup();
        fake.outputs.lock().unwrap().clear();
        let audio = AudioMute::spawn(fake.clone(), path.clone());
        drop(audio.guard());
        audio.flush();
        assert_eq!(fake.calls(), vec![MuteCall::MuteAll]);
        assert!(!path.exists());
    }

    #[test]
    fn commands_run_in_order_one_recording_after_the_other() {
        let (fake, _dir, path) = setup();
        let audio = AudioMute::spawn(fake.clone(), path.clone());
        drop(audio.guard());
        audio.flush();
        *fake.outputs.lock().unwrap() = ids(&["casque"]);
        let second = audio.guard();
        audio.flush();
        assert_eq!(file(&path), Some(ids(&["casque"])));
        drop(second);
        audio.flush();
        let all = ids(&["haut-parleurs", "casque"]);
        assert_eq!(
            fake.calls(),
            vec![MuteCall::MuteAll, MuteCall::Restore(all), MuteCall::MuteAll, MuteCall::Restore(ids(&["casque"]))]
        );
    }

    #[test]
    fn an_unwritable_recovery_file_does_not_prevent_muting() {
        let (fake, dir, _) = setup();
        let path = dir.path().join("absent").join("muted_outputs.json");
        let audio = AudioMute::spawn(fake.clone(), path.clone());
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
        let audio = AudioMute::spawn(fake.clone(), path.clone());
        drop(audio.guard());
        audio.flush();
        assert_eq!(fake.calls().len(), 2);
        assert!(path.is_dir());
    }

    #[test]
    fn recover_restores_the_outputs_left_muted_then_forgets_them() {
        let (fake, _dir, path) = setup();
        std::fs::write(&path, r#"["haut-parleurs"]"#).unwrap();
        recover(fake.as_ref(), &path);
        assert_eq!(fake.calls(), vec![MuteCall::Restore(ids(&["haut-parleurs"]))]);
        assert!(!path.exists());
    }

    #[test]
    fn recover_without_a_file_touches_nothing() {
        let (fake, _dir, path) = setup();
        recover(fake.as_ref(), &path);
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn recover_drops_an_unreadable_file() {
        let (fake, _dir, path) = setup();
        std::fs::write(&path, "{pas du json").unwrap();
        recover(fake.as_ref(), &path);
        assert!(fake.calls().is_empty());
        assert!(!path.exists());
    }
}
