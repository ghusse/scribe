use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};

use tauri::AppHandle;

use scribe_core::focus::FocusDetector;
use scribe_core::insert::{Clipboard, KeySender};
use scribe_core::storage::Db;
use scribe_platform::HookConfig;

use crate::controller::ControllerMsg;
use crate::settings::Settings;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
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
            data_dir,
        }
    }
}

/// Everything the controller, the processing tasks and the Tauri commands share.
pub struct Services {
    pub app: AppHandle,
    pub db: Mutex<Db>,
    pub settings: RwLock<Settings>,
    pub paths: AppPaths,
    pub hook_cfg: Arc<HookConfig>,
    pub focus: Arc<dyn FocusDetector>,
    pub clipboard: Arc<dyn Clipboard>,
    pub keys: Arc<dyn KeySender>,
    /// Set while the settings UI waits for the user to press the new hotkey.
    pub key_capture: Mutex<Option<Sender<u32>>>,
    pub ctrl_tx: Mutex<Sender<ControllerMsg>>,
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
