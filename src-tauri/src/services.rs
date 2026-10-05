use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use scribe_core::chord::{ChordRecorder, Recorded};
use scribe_core::focus::FocusDetector;
use scribe_core::insert::{Clipboard, KeySender};
use scribe_core::pipeline::{Corrector, ProviderError, Transcriber};
use scribe_core::storage::Db;
use scribe_platform::{HookConfig, RawKey};

use crate::controller::ControllerMsg;
use crate::overlay::Overlay;
use crate::secrets::SecretStore;
use crate::settings::Settings;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub audio_dir: PathBuf,
    pub db_path: PathBuf,
    pub settings_path: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            audio_dir: data_dir.join("audio"),
            db_path: data_dir.join("scribe.db"),
            settings_path: data_dir.join("settings.json"),
        }
    }
}

/// Everything the backend tells the main (settings/history) window, apart from the overlay.
pub trait UiSink: Send + Sync {
    /// Microphone level (RMS 0..1) for the recording pill.
    fn audio_level(&self, rms: f32);
    /// The history list must be reloaded.
    fn history_changed(&self);
    /// Scroll the history to this dictation.
    fn focus_dictation(&self, id: i64);
    /// Show, restore and focus the main window.
    fn show_main(&self);
}

pub type Providers = (Box<dyn Transcriber>, Box<dyn Corrector>);
/// Builds the transcriber and corrector for the current settings (`providers::build` in the app).
pub type ProviderFactory = Box<dyn Fn(&Settings, &dyn SecretStore) -> Result<Providers, ProviderError> + Send + Sync>;

/// Everything the controller, the processing tasks and the Tauri commands share.
pub struct Services {
    pub db: Mutex<Db>,
    pub settings: RwLock<Settings>,
    pub paths: AppPaths,
    pub hook_cfg: Arc<HookConfig>,
    pub focus: Arc<dyn FocusDetector>,
    pub clipboard: Arc<dyn Clipboard>,
    pub keys: Arc<dyn KeySender>,
    pub secrets: Arc<dyn SecretStore>,
    pub providers: ProviderFactory,
    pub overlay: Overlay,
    pub ui: Arc<dyn UiSink>,
    /// Set while the settings UI waits for the user to press the new hotkey.
    pub key_capture: KeyCapture,
    pub ctrl_tx: Mutex<Sender<ControllerMsg>>,
}

impl Services {
    pub fn build_providers(&self, settings: &Settings) -> Result<Providers, ProviderError> {
        (self.providers)(settings, self.secrets.as_ref())
    }

    pub fn send_ctrl(&self, msg: ControllerMsg) {
        let _ = self.ctrl_tx.lock().unwrap().send(msg);
    }
}

/// One pending « press the new hotkey » request. Each capture gets a token, so a capture that ends
/// (timeout, superseded) never clears a newer capture started meanwhile. While a capture is pending the
/// hook swallows every key (`HookConfig::capturing`).
pub struct KeyCapture {
    /// Last token handed out, and the pending capture.
    state: Mutex<(u64, Option<PendingCapture>)>,
    hook: Arc<HookConfig>,
}

struct PendingCapture {
    token: u64,
    recorder: ChordRecorder,
    tx: Sender<Vec<u32>>,
}

impl KeyCapture {
    pub fn new(hook: Arc<HookConfig>) -> Self {
        Self { state: Mutex::new((0, None)), hook }
    }

    fn set(&self, state: &mut (u64, Option<PendingCapture>), pending: Option<PendingCapture>) {
        state.1 = pending;
        self.hook.capturing.store(state.1.is_some(), Ordering::Relaxed);
    }

    /// Starts a capture, superseding any pending one (whose receiver then disconnects).
    pub fn begin(&self) -> (u64, Receiver<Vec<u32>>) {
        let (tx, rx) = mpsc::channel();
        let mut state = self.state.lock().unwrap();
        state.0 += 1;
        let token = state.0;
        self.set(&mut state, Some(PendingCapture { token, recorder: ChordRecorder::new(), tx }));
        (token, rx)
    }

    /// Hands a key event to the pending capture, if any. Returns whether it was taken (then it is not a
    /// gesture); events of keys already down when the capture started are not. The capture sends the combination once all keys are released, or an empty one on Échap.
    pub fn offer(&self, key: RawKey) -> bool {
        let mut state = self.state.lock().unwrap();
        let Some(pending) = state.1.as_mut() else {
            return false;
        };
        let done = match pending.recorder.feed(key.vk, key.down, key.repeat) {
            // A key held from before the capture: its events stay gestures (e.g. releasing the trigger).
            Recorded::Ignored => return false,
            Recorded::Pending => return true,
            Recorded::Done(keys) => keys,
            Recorded::Cancelled => Vec::new(),
        };
        let sent = pending.tx.send(done).is_ok();
        self.set(&mut state, None);
        sent
    }

    /// Ends the capture `token`, leaving a newer one alone.
    pub fn finish(&self, token: u64) {
        let mut state = self.state.lock().unwrap();
        if matches!(&state.1, Some(p) if p.token == token) {
            self.set(&mut state, None);
        }
    }

    /// Ends the pending capture early: its waiter returns None at once.
    pub fn cancel(&self) {
        let mut state = self.state.lock().unwrap();
        self.set(&mut state, None);
    }

    #[cfg(test)]
    pub fn is_pending(&self) -> bool {
        self.state.lock().unwrap().1.is_some()
    }

    /// Waits up to `timeout` for a combination: None on timeout or cancel, empty on Échap.
    pub fn wait(&self, timeout: Duration) -> Option<Vec<u32>> {
        let (token, rx) = self.begin();
        let keys = rx.recv_timeout(timeout).ok();
        self.finish(token);
        keys
    }
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Instant;

    use super::*;

    #[test]
    fn paths_live_in_the_data_dir() {
        let p = AppPaths::new(PathBuf::from("data"));
        assert_eq!(p.audio_dir, Path::new("data").join("audio"));
        assert_eq!(p.db_path, Path::new("data").join("scribe.db"));
        assert_eq!(p.settings_path, Path::new("data").join("settings.json"));
    }

    #[test]
    fn now_is_rfc3339_utc_with_millis() {
        let now = now_rfc3339();
        assert!(chrono::DateTime::parse_from_rfc3339(&now).is_ok(), "{now}");
        assert!(now.ends_with('Z') && now.len() == "2026-10-05T12:00:00.000Z".len(), "{now}");
    }

    fn kc() -> KeyCapture {
        KeyCapture::new(Arc::new(HookConfig::new(&[0xA3], 0x20)))
    }

    fn key(vk: u32, down: bool) -> RawKey {
        RawKey { vk, down, repeat: false, t_ms: 0 }
    }

    #[test]
    fn capture_receives_the_combination_once_all_keys_are_released() {
        let kc = Arc::new(kc());
        assert!(!kc.offer(key(0x41, true)), "nothing pending");
        assert!(!kc.hook.capturing.load(Ordering::Relaxed));
        let kc2 = kc.clone();
        let waiter = std::thread::spawn(move || kc2.wait(Duration::from_secs(5)));
        while !kc.is_pending() {
            std::thread::yield_now();
        }
        assert!(kc.hook.capturing.load(Ordering::Relaxed), "the hook swallows keys during a capture");
        for (vk, down) in [(0xA2, true), (0x41, true), (0x41, false)] {
            assert!(kc.offer(key(vk, down)));
            assert!(kc.is_pending());
        }
        assert!(kc.offer(key(0xA2, false)));
        assert_eq!(waiter.join().unwrap(), Some(vec![0xA2, 0x41]));
        assert!(!kc.is_pending());
        assert!(!kc.hook.capturing.load(Ordering::Relaxed), "the keyboard works again");
        assert!(!kc.offer(key(0x42, true)), "the capture is over");
    }

    #[test]
    fn keys_held_before_the_capture_are_left_to_the_gestures() {
        let kc = kc();
        let (_, rx) = kc.begin();
        assert!(!kc.offer(key(0xA3, false)), "release of the trigger held when the capture began");
        assert!(!kc.offer(RawKey { vk: 0x0D, down: true, repeat: true, t_ms: 0 }), "auto-repeat of the Entrée that clicked");
        assert!(kc.is_pending());
        assert!(kc.offer(key(0x77, true)));
        assert!(kc.offer(RawKey { vk: 0x77, down: true, repeat: true, t_ms: 0 }));
        assert!(kc.offer(key(0x77, false)));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(vec![0x77]));
    }

    #[test]
    fn escape_ends_the_capture_with_an_empty_combination() {
        let kc = kc();
        let (_, rx) = kc.begin();
        assert!(kc.offer(key(0xA2, true)));
        assert!(kc.offer(key(0x1B, true)));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(vec![]));
        assert!(!kc.is_pending());
        assert!(!kc.hook.capturing.load(Ordering::Relaxed));
    }

    #[test]
    fn capture_times_out_and_clears_itself() {
        let kc = kc();
        let start = Instant::now();
        assert_eq!(kc.wait(Duration::from_millis(20)), None);
        assert!(start.elapsed() >= Duration::from_millis(20));
        assert!(!kc.is_pending());
        assert!(!kc.hook.capturing.load(Ordering::Relaxed), "a timed-out capture must not block the keyboard");
    }

    #[test]
    fn cancel_ends_the_pending_capture_at_once() {
        let kc = kc();
        let (_, rx) = kc.begin();
        kc.cancel();
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_err(), "sender dropped");
        assert!(!kc.is_pending());
        assert!(!kc.hook.capturing.load(Ordering::Relaxed));
    }

    #[test]
    fn an_ending_capture_does_not_clear_a_newer_one() {
        // Two concurrent captures (settings window opened twice, double click): the first is
        // superseded and returns None, and its cleanup must not cancel the second.
        let kc = kc();
        let (first, first_rx) = kc.begin();
        let (second, second_rx) = kc.begin();
        assert_ne!(first, second);
        assert!(first_rx.recv_timeout(Duration::from_secs(5)).is_err(), "superseded");
        kc.finish(first);
        assert!(kc.is_pending(), "the newer capture survives");
        assert!(kc.hook.capturing.load(Ordering::Relaxed));
        kc.offer(key(0x20, true));
        assert!(kc.offer(key(0x20, false)));
        assert_eq!(second_rx.recv().unwrap(), vec![0x20]);
        kc.finish(second);
        assert!(!kc.is_pending());
    }

    #[test]
    fn a_combination_for_a_dropped_waiter_is_not_a_capture() {
        let kc = kc();
        drop(kc.begin());
        assert!(kc.offer(key(0x41, true)), "still pending: the key is taken");
        assert!(!kc.offer(key(0x41, false)), "nobody receives the combination");
        assert!(!kc.hook.capturing.load(Ordering::Relaxed));
    }
}
