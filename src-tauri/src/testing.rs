//! Test doubles for `Services`: in-memory Db, temp dirs, and fakes for every OS/network seam.
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, RwLock};

use scribe_core::focus::{FocusDetector, FocusSnapshot, FocusState};
use scribe_core::insert::{Clipboard, ClipboardContent, KeySender};
use scribe_core::pipeline::{Corrector, ProviderError, Transcriber};
use scribe_core::prompt::CorrectionPrompt;
use scribe_core::storage::Db;
use scribe_platform::HookConfig;

use crate::controller::ControllerMsg;
use crate::overlay::fake::FakeWindow;
use crate::overlay::Overlay;
use crate::secrets::memory::MemorySecretStore;
use crate::services::{AppPaths, KeyCapture, Services, UiSink};
use crate::settings::Settings;

#[derive(Debug, Clone, PartialEq)]
pub enum UiCall {
    Level(f32),
    HistoryChanged,
    FocusDictation(i64),
    ShowMain,
}

#[derive(Default)]
pub struct FakeUi {
    pub calls: Mutex<Vec<UiCall>>,
}

impl FakeUi {
    pub fn calls(&self) -> Vec<UiCall> {
        self.calls.lock().unwrap().clone()
    }
    pub fn count(&self, call: &UiCall) -> usize {
        self.calls().iter().filter(|c| *c == call).count()
    }
}

impl UiSink for FakeUi {
    fn audio_level(&self, rms: f32) {
        self.calls.lock().unwrap().push(UiCall::Level(rms));
    }
    fn history_changed(&self) {
        self.calls.lock().unwrap().push(UiCall::HistoryChanged);
    }
    fn focus_dictation(&self, id: i64) {
        self.calls.lock().unwrap().push(UiCall::FocusDictation(id));
    }
    fn show_main(&self) {
        self.calls.lock().unwrap().push(UiCall::ShowMain);
    }
}

/// Clipboard holding text; `fail` makes writes fail.
#[derive(Default)]
pub struct FakeClipboard {
    pub content: Mutex<Option<String>>,
    pub fail: Mutex<bool>,
}

impl FakeClipboard {
    pub fn text(&self) -> Option<String> {
        self.content.lock().unwrap().clone()
    }
}

impl Clipboard for FakeClipboard {
    fn read(&self) -> ClipboardContent {
        self.content.lock().unwrap().clone().map_or(ClipboardContent::Empty, ClipboardContent::Text)
    }
    fn write_text(&self, text: &str) -> Result<(), String> {
        if *self.fail.lock().unwrap() {
            return Err("presse-papier occupé".into());
        }
        *self.content.lock().unwrap() = Some(text.into());
        Ok(())
    }
    fn restore(&self, content: &ClipboardContent) -> Result<(), String> {
        *self.content.lock().unwrap() = match content {
            ClipboardContent::Text(t) => Some(t.clone()),
            _ => None,
        };
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeKeys {
    pub pastes: Mutex<u32>,
    pub fail: Mutex<bool>,
}

impl KeySender for FakeKeys {
    fn send_paste(&self) -> Result<(), String> {
        if *self.fail.lock().unwrap() {
            return Err("SendInput refusé".into());
        }
        *self.pastes.lock().unwrap() += 1;
        Ok(())
    }
}

pub struct FakeFocus(pub Mutex<FocusSnapshot>);

impl FocusDetector for FakeFocus {
    fn snapshot(&self) -> FocusSnapshot {
        self.0.lock().unwrap().clone()
    }
}

pub fn editable(app: &str) -> FocusSnapshot {
    FocusSnapshot { app_name: Some(app.into()), window_id: Some(7), state: FocusState::Editable }
}

/// What the fake providers answer. `build` fails the factory itself (unknown provider, missing key).
#[derive(Clone)]
pub struct ProviderPlan {
    pub build: Result<(), ProviderError>,
    pub stt: Result<String, ProviderError>,
    pub llm: Result<String, ProviderError>,
}

impl Default for ProviderPlan {
    fn default() -> Self {
        Self { build: Ok(()), stt: Ok("bonjour scribe".into()), llm: Ok("<output>Bonjour Scribe.</output>".into()) }
    }
}

struct FakeStt(Result<String, ProviderError>);

#[async_trait::async_trait]
impl Transcriber for FakeStt {
    fn name(&self) -> String {
        "fake-stt".into()
    }
    async fn transcribe(&self, _wav: &[u8], _hints: &[String]) -> Result<String, ProviderError> {
        self.0.clone()
    }
}

struct FakeLlm(Result<String, ProviderError>);

#[async_trait::async_trait]
impl Corrector for FakeLlm {
    fn name(&self) -> String {
        "fake-llm".into()
    }
    async fn correct(&self, _p: &CorrectionPrompt) -> Result<String, ProviderError> {
        self.0.clone()
    }
}

pub struct Fixture {
    pub svc: Arc<Services>,
    pub window: Arc<FakeWindow>,
    pub ui: Arc<FakeUi>,
    pub clipboard: Arc<FakeClipboard>,
    pub keys: Arc<FakeKeys>,
    pub focus: Arc<FakeFocus>,
    pub secrets: Arc<MemorySecretStore>,
    pub plan: Arc<Mutex<ProviderPlan>>,
    pub ctrl_rx: Receiver<ControllerMsg>,
    pub _dir: tempfile::TempDir,
}

impl Fixture {
    pub fn new() -> Self {
        Self::with_settings(Settings { restore_delay_ms: 0, ..Default::default() })
    }

    pub fn with_settings(settings: Settings) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().to_path_buf());
        std::fs::create_dir_all(&paths.audio_dir).unwrap();
        let window = Arc::new(FakeWindow::default());
        let ui = Arc::new(FakeUi::default());
        let clipboard = Arc::new(FakeClipboard::default());
        let keys = Arc::new(FakeKeys::default());
        let focus = Arc::new(FakeFocus(Mutex::new(editable("Notepad"))));
        let secrets = Arc::new(MemorySecretStore::default());
        let plan = Arc::new(Mutex::new(ProviderPlan::default()));
        let factory_plan = plan.clone();
        let (tx, ctrl_rx) = mpsc::channel();
        let svc = Arc::new(Services {
            db: Mutex::new(Db::open_in_memory().unwrap()),
            hook_cfg: Arc::new(HookConfig::new(settings.trigger_vk, settings.lock_vk)),
            settings: RwLock::new(settings),
            paths,
            focus: focus.clone(),
            clipboard: clipboard.clone(),
            keys: keys.clone(),
            secrets: secrets.clone(),
            providers: Box::new(move |_s, _store| {
                let plan = factory_plan.lock().unwrap().clone();
                plan.build?;
                Ok((Box::new(FakeStt(plan.stt)), Box::new(FakeLlm(plan.llm))))
            }),
            overlay: Overlay::new(window.clone()),
            ui: ui.clone(),
            key_capture: KeyCapture::default(),
            ctrl_tx: Mutex::new(tx),
        });
        Self { svc, window, ui, clipboard, keys, focus, secrets, plan, ctrl_rx, _dir: dir }
    }

    pub fn plan(&self, f: impl FnOnce(&mut ProviderPlan)) {
        f(&mut self.plan.lock().unwrap());
    }

    pub fn wav_files(&self) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(&self.svc.paths.audio_dir).unwrap().map(|e| e.unwrap().path()).collect()
    }
}
