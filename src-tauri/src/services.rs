use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use scribe_core::focus::FocusDetector;
use scribe_core::insert::{Clipboard, KeySender};
use scribe_core::pipeline::{Corrector, ProviderError, Transcriber};
use scribe_core::storage::Db;
use scribe_platform::HookConfig;

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
/// (timeout, superseded) never clears a newer capture started meanwhile.
#[derive(Default)]
pub struct KeyCapture {
    /// Last token handed out, and the pending capture (its token and sender).
    state: Mutex<(u64, Option<PendingCapture>)>,
}

type PendingCapture = (u64, Sender<u32>);

impl KeyCapture {
    /// Starts a capture, superseding any pending one (whose receiver then disconnects).
    pub fn begin(&self) -> (u64, Receiver<u32>) {
        let (tx, rx) = mpsc::channel();
        let mut state = self.state.lock().unwrap();
        state.0 += 1;
        let token = state.0;
        state.1 = Some((token, tx));
        (token, rx)
    }

    /// Hands a key press to the pending capture, if any. Returns whether it was captured.
    pub fn offer(&self, vk: u32) -> bool {
        match self.state.lock().unwrap().1.take() {
            Some((_, tx)) => tx.send(vk).is_ok(),
            None => false,
        }
    }

    /// Ends the capture `token`, leaving a newer one alone.
    pub fn finish(&self, token: u64) {
        let mut state = self.state.lock().unwrap();
        if matches!(state.1, Some((t, _)) if t == token) {
            state.1 = None;
        }
    }

    /// Ends the pending capture early: its waiter returns None at once.
    pub fn cancel(&self) {
        self.state.lock().unwrap().1 = None;
    }

    #[cfg(test)]
    pub fn is_pending(&self) -> bool {
        self.state.lock().unwrap().1.is_some()
    }

    /// Waits up to `timeout` for the next key press.
    pub fn wait(&self, timeout: Duration) -> Option<u32> {
        let (token, rx) = self.begin();
        let key = rx.recv_timeout(timeout).ok();
        self.finish(token);
        key
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

    #[test]
    fn capture_receives_the_offered_key() {
        let kc = Arc::new(KeyCapture::default());
        assert!(!kc.offer(0x41), "nothing pending");
        let kc2 = kc.clone();
        let waiter = std::thread::spawn(move || kc2.wait(Duration::from_secs(5)));
        while !kc.is_pending() {
            std::thread::yield_now();
        }
        assert!(kc.offer(0x41));
        assert_eq!(waiter.join().unwrap(), Some(0x41));
        assert!(!kc.is_pending());
        assert!(!kc.offer(0x42), "a capture takes one key only");
    }

    #[test]
    fn capture_times_out_and_clears_itself() {
        let kc = KeyCapture::default();
        let start = Instant::now();
        assert_eq!(kc.wait(Duration::from_millis(20)), None);
        assert!(start.elapsed() >= Duration::from_millis(20));
        assert!(!kc.is_pending());
    }

    #[test]
    fn cancel_ends_the_pending_capture_at_once() {
        let kc = KeyCapture::default();
        let (_, rx) = kc.begin();
        kc.cancel();
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_err(), "sender dropped");
        assert!(!kc.is_pending());
    }

    #[test]
    fn an_ending_capture_does_not_clear_a_newer_one() {
        // Two concurrent captures (settings window opened twice, double click): the first is
        // superseded and returns None, and its cleanup must not cancel the second.
        let kc = KeyCapture::default();
        let (first, first_rx) = kc.begin();
        let (second, second_rx) = kc.begin();
        assert_ne!(first, second);
        assert!(first_rx.recv_timeout(Duration::from_secs(5)).is_err(), "superseded");
        kc.finish(first);
        assert!(kc.is_pending(), "the newer capture survives");
        assert!(kc.offer(0x20));
        assert_eq!(second_rx.recv().unwrap(), 0x20);
        kc.finish(second);
        assert!(!kc.is_pending());
    }

    #[test]
    fn offer_to_a_dropped_waiter_is_not_a_capture() {
        let kc = KeyCapture::default();
        drop(kc.begin());
        assert!(!kc.offer(0x41));
    }
}
