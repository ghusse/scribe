//! Muting the audio outputs while dictating: which outputs to mute and which ones to unmute afterwards. The raw
//! audio calls are an [`OutputBackend`]; the system one is `output_backend()` (WASAPI on Windows, CoreAudio on
//! macOS, excluded from coverage).
use scribe_core::insert::SystemMute;

/// One raw audio call per method; errors are display strings. Output ids are stable across launches (they are
/// written to the crash-recovery file).
pub trait OutputBackend: Send + Sync {
    /// Ids of the active outputs (headphones included).
    fn active_outputs(&self) -> Result<Vec<String>, String>;
    /// An output that no longer exists is an error.
    fn is_muted(&self, id: &str) -> Result<bool, String>;
    fn set_muted(&self, id: &str, muted: bool) -> Result<(), String>;
}

/// [`SystemMute`] on top of an [`OutputBackend`].
#[derive(Debug, Default, Clone, Copy)]
pub struct OutputMuter<B>(pub B);

impl<B: OutputBackend> SystemMute for OutputMuter<B> {
    fn mute_all(&self) -> Vec<String> {
        let outputs = match self.0.active_outputs() {
            Ok(outputs) => outputs,
            Err(e) => {
                tracing::warn!("sorties audio illisibles, son non coupé : {e}");
                return Vec::new();
            }
        };
        let mut muted = Vec::new();
        for id in outputs {
            match self.0.is_muted(&id) {
                // Muted by the user: not ours to unmute later.
                Ok(true) => continue,
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!("sortie audio {id} illisible, ignorée : {e}");
                    continue;
                }
            }
            match self.0.set_muted(&id, true) {
                Ok(()) => muted.push(id),
                Err(e) => tracing::warn!("impossible de couper la sortie audio {id} : {e}"),
            }
        }
        muted
    }

    fn restore(&self, ids: &[String]) {
        for id in ids {
            match self.0.is_muted(id) {
                Ok(true) => {
                    if let Err(e) = self.0.set_muted(id, false) {
                        tracing::warn!("impossible de rétablir la sortie audio {id} : {e}");
                    }
                }
                // Unmuted meanwhile (by the user): nothing to do.
                Ok(false) => {}
                // Unplugged or disabled meanwhile: not an error, there is nothing left to unmute.
                Err(e) => tracing::debug!("sortie audio {id} introuvable, non rétablie : {e}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// In-memory outputs: id → muted. `broken` outputs fail every call, `stuck` ones refuse `set_muted`,
    /// `unlisted` makes `active_outputs` fail. Every `set_muted` call is logged.
    #[derive(Default)]
    struct Fake {
        outputs: Mutex<BTreeMap<String, bool>>,
        broken: Vec<&'static str>,
        stuck: Vec<&'static str>,
        unlisted: bool,
        log: Mutex<Vec<String>>,
    }

    impl Fake {
        fn with(outputs: &[(&str, bool)]) -> Self {
            let outputs = outputs.iter().map(|(id, m)| (id.to_string(), *m)).collect();
            Fake { outputs: Mutex::new(outputs), ..Fake::default() }
        }
        fn muted(&self, id: &str) -> Option<bool> {
            self.outputs.lock().unwrap().get(id).copied()
        }
        fn set(&self, id: &str, muted: bool) {
            self.outputs.lock().unwrap().insert(id.into(), muted);
        }
        fn remove(&self, id: &str) {
            self.outputs.lock().unwrap().remove(id);
        }
        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    impl OutputBackend for Fake {
        fn active_outputs(&self) -> Result<Vec<String>, String> {
            if self.unlisted {
                return Err("no audio service".into());
            }
            Ok(self.outputs.lock().unwrap().keys().cloned().collect())
        }
        fn is_muted(&self, id: &str) -> Result<bool, String> {
            if self.broken.contains(&id) {
                return Err("device error".into());
            }
            self.muted(id).ok_or_else(|| "not found".into())
        }
        fn set_muted(&self, id: &str, muted: bool) -> Result<(), String> {
            self.log.lock().unwrap().push(format!("{id}={muted}"));
            if self.broken.contains(&id) || self.stuck.contains(&id) {
                return Err("access denied".into());
            }
            let mut outputs = self.outputs.lock().unwrap();
            let slot = outputs.get_mut(id).ok_or("not found")?;
            *slot = muted;
            Ok(())
        }
    }

    fn ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn mutes_every_active_output_and_returns_them() {
        let m = OutputMuter(Fake::with(&[("headphones", false), ("speakers", false)]));
        assert_eq!(m.mute_all(), ids(&["headphones", "speakers"]));
        assert_eq!(m.0.muted("headphones"), Some(true));
        assert_eq!(m.0.muted("speakers"), Some(true));
    }

    #[test]
    fn an_output_already_muted_is_left_alone_and_not_returned() {
        let m = OutputMuter(Fake::with(&[("hdmi", true), ("speakers", false)]));
        assert_eq!(m.mute_all(), ids(&["speakers"]));
        assert_eq!(m.0.muted("hdmi"), Some(true));
        assert_eq!(m.0.log(), ["speakers=true"]);
    }

    #[test]
    fn an_output_that_fails_is_skipped_and_the_others_are_muted() {
        let fake = Fake { broken: vec!["bluetooth"], stuck: vec!["hdmi"], ..Fake::with(&[]) };
        for id in ["bluetooth", "hdmi", "speakers"] {
            fake.set(id, false);
        }
        let m = OutputMuter(fake);
        assert_eq!(m.mute_all(), ids(&["speakers"]));
        assert_eq!(m.0.muted("hdmi"), Some(false));
        assert_eq!(m.0.muted("speakers"), Some(true));
        // The broken output is never written: reading it failed first.
        assert_eq!(m.0.log(), ["hdmi=true", "speakers=true"]);
    }

    #[test]
    fn nothing_is_muted_when_the_outputs_cannot_be_listed() {
        let m = OutputMuter(Fake { unlisted: true, ..Fake::with(&[("speakers", false)]) });
        assert!(m.mute_all().is_empty());
        assert_eq!(m.0.muted("speakers"), Some(false));
        assert!(m.0.log().is_empty());
    }

    #[test]
    fn no_output_means_nothing_to_mute() {
        assert!(OutputMuter(Fake::default()).mute_all().is_empty());
    }

    #[test]
    fn restore_unmutes_the_outputs_it_muted() {
        let m = OutputMuter(Fake::with(&[("hdmi", true), ("speakers", false)]));
        let muted = m.mute_all();
        m.restore(&muted);
        assert_eq!(m.0.muted("speakers"), Some(false));
        // Muted by the user before dictating: stays muted.
        assert_eq!(m.0.muted("hdmi"), Some(true));
    }

    #[test]
    fn restore_skips_an_output_unmuted_meanwhile() {
        let m = OutputMuter(Fake::with(&[("speakers", false)]));
        let muted = m.mute_all();
        m.0.set("speakers", false);
        m.restore(&muted);
        assert_eq!(m.0.muted("speakers"), Some(false));
        assert_eq!(m.0.log(), ["speakers=true"]);
    }

    #[test]
    fn restore_ignores_an_output_gone_meanwhile_and_restores_the_others() {
        let m = OutputMuter(Fake::with(&[("headphones", false), ("speakers", false)]));
        let muted = m.mute_all();
        m.0.remove("headphones");
        m.restore(&muted);
        assert_eq!(m.0.muted("headphones"), None);
        assert_eq!(m.0.muted("speakers"), Some(false));
        assert_eq!(m.0.log(), ["headphones=true", "speakers=true", "speakers=false"]);
    }

    #[test]
    fn restore_goes_on_after_an_output_refuses() {
        let fake = Fake { stuck: vec!["hdmi"], ..Fake::with(&[("hdmi", true), ("speakers", true)]) };
        let m = OutputMuter(fake);
        m.restore(&ids(&["hdmi", "speakers"]));
        assert_eq!(m.0.muted("hdmi"), Some(true));
        assert_eq!(m.0.muted("speakers"), Some(false));
        assert_eq!(m.0.log(), ["hdmi=false", "speakers=false"]);
    }

    #[test]
    fn restore_of_nothing_touches_nothing() {
        let m = OutputMuter(Fake::with(&[("speakers", true)]));
        m.restore(&[]);
        assert_eq!(m.0.muted("speakers"), Some(true));
        assert!(m.0.log().is_empty());
    }

    #[test]
    fn works_behind_the_core_trait() {
        let m: Box<dyn SystemMute> = Box::new(OutputMuter(Fake::with(&[("speakers", false)])));
        let muted = m.mute_all();
        assert_eq!(muted, ids(&["speakers"]));
        m.restore(&muted);
    }
}
