use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use tauri::Emitter;

use scribe_core::clock;
use scribe_core::focus::{self, FocusSnapshot};
use scribe_core::gesture::{GestureCommand, GestureDetector, KeyEvent, KeyRole, Mode};
use scribe_core::session::{self, Session, SessionAction};
use scribe_platform::audio_capture::{start_recording, LevelCallback, RecordingHandle};
use scribe_platform::RawKey;

use crate::dictation::{self, Captured, FOCUS_TIMEOUT_MS};
use crate::overlay::{self, OverlayEvent, ToastLevel};
use crate::services::Services;

pub enum ControllerMsg {
    Key(RawKey),
    Tick,
    ProcessingDone,
    SettingsChanged,
}

pub fn key_role(vk: u32, trigger_vk: u32, lock_vk: u32) -> KeyRole {
    if vk == trigger_vk {
        KeyRole::Trigger
    } else if lock_vk != 0 && vk == lock_vk {
        KeyRole::Lock
    } else {
        KeyRole::Other
    }
}

struct Controller {
    svc: Arc<Services>,
    tx: Sender<ControllerMsg>,
    gesture: GestureDetector,
    session: Session,
    mode: Mode,
    recording: Option<(RecordingHandle, FocusSnapshot)>,
}

pub fn spawn(svc: Arc<Services>, rx: Receiver<ControllerMsg>, tx: Sender<ControllerMsg>) {
    let tick_tx = tx.clone();
    std::thread::Builder::new()
        .name("scribe-ticker".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(30));
            if tick_tx.send(ControllerMsg::Tick).is_err() {
                break;
            }
        })
        .expect("ticker thread");
    let s = svc.settings.read().unwrap().clone();
    let mut c = Controller {
        svc,
        tx,
        gesture: GestureDetector::new(s.gesture.clone()),
        session: Session::new(s.max_recording_ms),
        mode: Mode::Hold,
        recording: None,
    };
    std::thread::Builder::new()
        .name("scribe-controller".into())
        .spawn(move || {
            for msg in rx {
                c.handle(msg);
            }
        })
        .expect("controller thread");
}

impl Controller {
    fn handle(&mut self, msg: ControllerMsg) {
        match msg {
            ControllerMsg::Key(k) => {
                if k.down {
                    if let Some(capture) = self.svc.key_capture.lock().unwrap().take() {
                        let _ = capture.send(k.vk);
                        return;
                    }
                }
                if self.svc.hook_cfg.paused.load(Ordering::Relaxed) {
                    return;
                }
                let (trigger, lock) = {
                    let s = self.svc.settings.read().unwrap();
                    (s.trigger_vk, s.lock_vk)
                };
                let ev = KeyEvent { role: key_role(k.vk, trigger, lock), down: k.down, t_ms: k.t_ms };
                let cmds = self.gesture.on_key(ev);
                self.apply_gestures(cmds, k.t_ms);
            }
            ControllerMsg::Tick => {
                let now = clock::now_ms();
                let cmds = self.gesture.on_tick(now);
                self.apply_gestures(cmds, now);
                if let Some(action) = self.session.on_tick(now) {
                    self.gesture.reset();
                    self.apply_action(action);
                }
            }
            ControllerMsg::ProcessingDone => self.session.on_processing_done(),
            ControllerMsg::SettingsChanged => {
                let s = self.svc.settings.read().unwrap().clone();
                self.gesture.set_config(s.gesture.clone());
                self.session.set_max_recording_ms(s.max_recording_ms);
                self.svc.hook_cfg.trigger_vk.store(s.trigger_vk, Ordering::Relaxed);
                self.svc.hook_cfg.lock_vk.store(s.lock_vk, Ordering::Relaxed);
            }
        }
    }

    /// `session::feed` keeps the detector in step when the session ignores a command
    /// (e.g. a double-tap during Processing).
    fn apply_gestures(&mut self, cmds: Vec<GestureCommand>, now: u64) {
        for action in session::feed(&mut self.gesture, &mut self.session, cmds, now) {
            self.apply_action(action);
        }
    }

    fn level_emitter(&self) -> LevelCallback {
        let app = self.svc.app.clone();
        let last = Arc::new(AtomicU64::new(0));
        Arc::new(move |rms: f32| {
            let now = clock::now_ms();
            if now.saturating_sub(last.load(Ordering::Relaxed)) >= 50 {
                last.store(now, Ordering::Relaxed);
                let _ = app.emit_to("overlay", "audio-level", rms);
            }
        })
    }

    fn apply_action(&mut self, action: SessionAction) {
        let app = self.svc.app.clone();
        match action {
            SessionAction::BeginRecording => match start_recording(self.level_emitter()) {
                Ok(handle) => {
                    self.mode = Mode::Hold;
                    overlay::emit(&app, OverlayEvent::Recording { locked: false });
                    let focus = focus::snapshot_with_timeout(self.svc.focus.clone(), FOCUS_TIMEOUT_MS);
                    self.recording = Some((handle, focus));
                }
                Err(e) => {
                    self.session.abort();
                    self.gesture.reset();
                    overlay::toast(&app, ToastLevel::Error, format!("Micro indisponible : {e}"), None, None);
                }
            },
            SessionAction::SetMode(mode) => {
                self.mode = mode;
                overlay::emit(&app, OverlayEvent::Recording { locked: mode == Mode::Locked });
            }
            SessionAction::DiscardRecording => {
                if let Some((handle, _)) = self.recording.take() {
                    let _ = handle.stop();
                }
                overlay::emit(&app, OverlayEvent::Idle);
            }
            SessionAction::FinishRecording => {
                let Some((handle, focus_start)) = self.recording.take() else {
                    self.session.on_processing_done();
                    return;
                };
                overlay::emit(&app, OverlayEvent::Processing);
                match handle.stop() {
                    Ok(clip) => {
                        let svc = self.svc.clone();
                        let tx = self.tx.clone();
                        let cap = Captured { clip, mode: self.mode, focus_start };
                        tauri::async_runtime::spawn(async move {
                            dictation::process(svc, cap).await;
                            let _ = tx.send(ControllerMsg::ProcessingDone);
                        });
                    }
                    Err(e) => {
                        self.session.on_processing_done();
                        overlay::toast(&app, ToastLevel::Error, format!("Enregistrement perdu : {e}"), None, None);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_virtual_keys_to_roles() {
        assert_eq!(key_role(0xA3, 0xA3, 0x20), KeyRole::Trigger);
        assert_eq!(key_role(0x20, 0xA3, 0x20), KeyRole::Lock);
        assert_eq!(key_role(0x41, 0xA3, 0x20), KeyRole::Other);
        assert_eq!(key_role(0x20, 0xA3, 0), KeyRole::Other);
    }
}
