use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use scribe_core::audio::AudioClip;
use scribe_core::chord;
use scribe_core::focus::{self, FocusSnapshot};
use scribe_core::gesture::{GestureCommand, GestureDetector, Mode};
use scribe_core::session::{self, Session, SessionAction};
use scribe_platform::audio_capture::LevelCallback;
use scribe_platform::HookEvent;

use crate::audio_mute::{AudioMute, MuteGuard};
use crate::dictation::{self, Captured, FOCUS_TIMEOUT_MS};
use crate::overlay::{OverlayEvent, ToastLevel};
use crate::services::{Services, UiSink};

pub enum ControllerMsg {
    /// A real key event, with its trigger/lock edges decided by the hook (`scribe_platform::key_filter`).
    Key(HookEvent),
    Tick,
    ProcessingDone,
    SettingsChanged,
    /// Sent by the tray after flipping `hook_cfg.paused`; carries the new value so a quick
    /// on/off toggle still discards the recording that was in flight when pause was set.
    PauseChanged(bool),
}

/// Microphone access (cpal in the app).
pub trait Recorder: Send + Sync {
    fn start(&self, on_level: LevelCallback) -> Result<Box<dyn Recording>, String>;
}

/// A recording in progress.
pub trait Recording: Send {
    /// Stops capture and returns 16 kHz mono audio.
    fn stop(self: Box<Self>) -> Result<AudioClip, String>;
}

/// Monotonic milliseconds, same clock as the keyboard hook timestamps.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// A finished recording handed over to processing.
pub struct Job {
    pub clip: AudioClip,
    pub mode: Mode,
    /// Resolves to the focus snapshot taken when the recording began.
    pub focus_start: Receiver<FocusSnapshot>,
}

/// Runs a job off the controller thread and sends `ProcessingDone` when it is over.
pub type SpawnProcessing = Box<dyn Fn(Arc<Services>, Job, Sender<ControllerMsg>) + Send>;

/// Physical key state (`scribe_platform::is_key_pressed` in the app).
pub type KeyState = Arc<dyn Fn(u32) -> bool + Send + Sync>;

pub struct ControllerDeps {
    pub recorder: Arc<dyn Recorder>,
    pub clock: Clock,
    pub spawn_processing: SpawnProcessing,
    pub is_pressed: KeyState,
}

/// How long the trigger must look physically up, while the detector still sees it held, before the press
/// is considered lost (Win+L sends the key-up to the secure desktop, never to the hook). Long enough that a
/// real key-up, already queued behind a tick, always wins.
pub const LOST_RELEASE_MS: u64 = 500;

/// Tray « Pause »: flips the hook pause flag, tells the controller, returns the new state.
pub fn toggle_pause(svc: &Services) -> bool {
    let paused = !svc.hook_cfg.paused.load(Ordering::Relaxed);
    svc.hook_cfg.paused.store(paused, Ordering::Relaxed);
    // The controller drops an in-flight recording on pause.
    svc.send_ctrl(ControllerMsg::PauseChanged(paused));
    paused
}

/// Forwards the microphone level to the UI at most every `min_interval_ms`.
pub fn level_emitter(ui: Arc<dyn UiSink>, clock: Clock, min_interval_ms: u64) -> LevelCallback {
    // u64::MAX = « never emitted »: the first level always goes through.
    let last = Arc::new(AtomicU64::new(u64::MAX));
    Arc::new(move |rms: f32| {
        let now = clock();
        let prev = last.load(Ordering::Relaxed);
        if prev == u64::MAX || now.saturating_sub(prev) >= min_interval_ms {
            last.store(now, Ordering::Relaxed);
            ui.audio_level(rms);
        }
    })
}

const LEVEL_INTERVAL_MS: u64 = 50;

/// A recording in progress, with what it holds until it ends.
struct Active {
    handle: Box<dyn Recording>,
    /// The start focus snapshot is resolved off the controller thread (it can take up to
    /// FOCUS_TIMEOUT_MS): blocking here would delay queued ticks and break double-tap timing.
    focus_start: Receiver<FocusSnapshot>,
    /// Restores the sound when dropped, so every path that ends the recording restores it. Dropped after the
    /// microphone is stopped.
    mute: Option<MuteGuard>,
}

pub struct Controller {
    svc: Arc<Services>,
    tx: Sender<ControllerMsg>,
    deps: ControllerDeps,
    gesture: GestureDetector,
    session: Session,
    mode: Mode,
    recording: Option<Active>,
    /// Mutes and restores the audio outputs off the controller thread.
    audio_mute: AudioMute,
    /// Since when the trigger looks physically up while the detector sees it held.
    released_since: Option<u64>,
}

fn spawn_focus_snapshot(svc: &Services) -> Receiver<FocusSnapshot> {
    let (tx, rx) = mpsc::channel();
    let detector = svc.focus.clone();
    std::thread::spawn(move || {
        let _ = tx.send(focus::snapshot_with_timeout(detector, FOCUS_TIMEOUT_MS));
    });
    rx
}

/// The app's `SpawnProcessing`: runs `dictation::process` on the async runtime.
pub fn spawn_processing(svc: Arc<Services>, job: Job, tx: Sender<ControllerMsg>) {
    tauri::async_runtime::spawn(async move {
        // Sent on drop, so a panic in process() cannot leave the session stuck in Processing.
        let _done = ProcessingDoneGuard(tx);
        let rx = job.focus_start;
        // Bounded by FOCUS_TIMEOUT_MS inside the snapshot thread.
        let focus_start = tauri::async_runtime::spawn_blocking(move || rx.recv().unwrap_or_else(|_| FocusSnapshot::unknown()))
            .await
            .unwrap_or_else(|_| FocusSnapshot::unknown());
        dictation::process(svc, Captured { clip: job.clip, mode: job.mode, focus_start }).await;
    });
}

/// Starts the ticker and the controller threads.
pub fn spawn(svc: Arc<Services>, rx: Receiver<ControllerMsg>, tx: Sender<ControllerMsg>, deps: ControllerDeps) {
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
    let mut c = Controller::new(svc, tx, deps);
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
    pub fn new(svc: Arc<Services>, tx: Sender<ControllerMsg>, deps: ControllerDeps) -> Self {
        let s = svc.settings.read().unwrap().clone();
        Self {
            audio_mute: AudioMute::spawn(svc.mute.clone(), svc.paths.muted_outputs_path.clone()),
            svc,
            tx,
            deps,
            gesture: GestureDetector::new(s.gesture.clone()),
            session: Session::new(s.max_recording_ms),
            mode: Mode::Hold,
            recording: None,
            released_since: None,
        }
    }

    pub fn handle(&mut self, msg: ControllerMsg) {
        match msg {
            ControllerMsg::Key(ev) => {
                // A pending hotkey capture takes every key until the combination is released, even while paused.
                if self.svc.key_capture.offer(ev.key) {
                    return;
                }
                if self.svc.hook_cfg.paused.load(Ordering::Relaxed) {
                    return;
                }
                for edge in ev.gestures {
                    let cmds = self.gesture.on_key(edge);
                    self.apply_gestures(cmds, edge.t_ms);
                }
            }
            ControllerMsg::Tick => {
                let now = (self.deps.clock)();
                let cmds = self.gesture.on_tick(now);
                self.apply_gestures(cmds, now);
                self.check_lost_release(now);
                if let Some(action) = self.session.on_tick(now) {
                    self.gesture.reset();
                    self.apply_action(action);
                }
            }
            ControllerMsg::ProcessingDone => self.session.on_processing_done(),
            ControllerMsg::PauseChanged(paused) => {
                if paused {
                    self.gesture.reset();
                    if let Some(action) = self.session.on_paused() {
                        self.apply_action(action);
                    }
                }
            }
            ControllerMsg::SettingsChanged => {
                let s = self.svc.settings.read().unwrap().clone();
                self.gesture.set_config(s.gesture.clone());
                self.session.set_max_recording_ms(s.max_recording_ms);
                self.svc.hook_cfg.set_trigger(&s.trigger_keys);
                self.svc.hook_cfg.lock_vk.store(s.lock_vk, Ordering::Relaxed);
            }
        }
    }

    /// A press whose key-up never reached the hook (session locked while dictating) is dropped: the
    /// recording is discarded rather than pasted into whatever has the focus after the unlock.
    fn check_lost_release(&mut self, now: u64) {
        let trigger = self.svc.hook_cfg.trigger.load(Ordering::Relaxed);
        let is_pressed = self.deps.is_pressed.as_ref();
        let physically_held = chord::keys(trigger).all(|k| chord::key_pressed(trigger, k, is_pressed));
        if !self.gesture.trigger_held() || physically_held {
            self.released_since = None;
            return;
        }
        let since = *self.released_since.get_or_insert(now);
        if now.saturating_sub(since) < LOST_RELEASE_MS {
            return;
        }
        self.released_since = None;
        self.gesture.reset();
        if let Some(action) = self.session.on_paused() {
            self.apply_action(action);
        }
    }

    /// `session::feed` keeps the detector in step when the session ignores a command
    /// (e.g. a double-tap during Processing).
    fn apply_gestures(&mut self, cmds: Vec<GestureCommand>, now: u64) {
        for action in session::feed(&mut self.gesture, &mut self.session, cmds, now) {
            self.apply_action(action);
        }
    }

    fn apply_action(&mut self, action: SessionAction) {
        let overlay = &self.svc.overlay;
        match action {
            SessionAction::BeginRecording => {
                let on_level = level_emitter(self.svc.ui.clone(), self.deps.clock.clone(), LEVEL_INTERVAL_MS);
                match self.deps.recorder.start(on_level) {
                    Ok(handle) => {
                        self.mode = Mode::Hold;
                        overlay.emit(OverlayEvent::Recording { locked: false });
                        let mute_on = self.svc.settings.read().unwrap().mute_audio_during_dictation;
                        let mute = mute_on.then(|| self.audio_mute.guard());
                        self.recording = Some(Active { handle, focus_start: spawn_focus_snapshot(&self.svc), mute });
                    }
                    Err(e) => {
                        self.session.abort();
                        self.gesture.reset();
                        overlay.toast(ToastLevel::Error, format!("Micro indisponible : {e}"), None, None);
                    }
                }
            }
            SessionAction::SetMode(mode) => {
                self.mode = mode;
                overlay.emit(OverlayEvent::Recording { locked: mode == Mode::Locked });
            }
            SessionAction::DiscardRecording => {
                if let Some(Active { handle, mute, .. }) = self.recording.take() {
                    let _ = handle.stop();
                    drop(mute);
                }
                overlay.emit(OverlayEvent::Idle);
            }
            SessionAction::FinishRecording => {
                let Some(Active { handle, focus_start, mute }) = self.recording.take() else {
                    self.session.on_processing_done();
                    return;
                };
                overlay.emit(OverlayEvent::Processing);
                let stopped = handle.stop();
                // Not before: the speakers would be heard at the end of the recording.
                drop(mute);
                match stopped {
                    Ok(clip) => {
                        let job = Job { clip, mode: self.mode, focus_start };
                        (self.deps.spawn_processing)(self.svc.clone(), job, self.tx.clone());
                    }
                    Err(e) => {
                        self.session.on_processing_done();
                        overlay.toast(ToastLevel::Error, format!("Enregistrement perdu : {e}"), None, None);
                    }
                }
            }
        }
    }
}

/// Sends `ProcessingDone` when dropped, including during a panic unwind.
struct ProcessingDoneGuard(Sender<ControllerMsg>);

impl Drop for ProcessingDoneGuard {
    fn drop(&mut self) {
        let _ = self.0.send(ControllerMsg::ProcessingDone);
    }
}

#[cfg(test)]
mod tests;
