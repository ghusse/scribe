use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use scribe_core::audio::AudioClip;
use scribe_core::chord;
use scribe_core::focus::{self, FocusSnapshot};
use scribe_core::gesture::{GestureCommand, GestureDetector, Mode};
use scribe_core::peek::{PeekTimer, PEEK_CLIP_MS};
use scribe_core::session::{self, Session, SessionAction, SessionState};
use scribe_core::style;
use scribe_platform::audio_capture::{LevelCallback, RecordOptions, RecordingStart};
use scribe_platform::HookEvent;

use crate::audio_mute::MuteGuard;
use crate::dictation::{self, Captured, FOCUS_TIMEOUT_MS};
use crate::overlay::{Notice, OverlayEvent, RecordingWarning, StyleBadge, ToastLevel};
use crate::services::{Services, UiSink};
use crate::settings::Settings;

pub enum ControllerMsg {
    /// A real key event, with its trigger/lock edges decided by the hook (`scribe_platform::key_filter`).
    Key(HookEvent),
    Tick,
    ProcessingDone,
    SettingsChanged,
    /// Sent by the tray after flipping `hook_cfg.paused`; carries the new value so a quick
    /// on/off toggle still discards the recording that was in flight when pause was set.
    PauseChanged(bool),
    /// The start of the transcript of recording `id`, to show its style while the user still speaks.
    Prefix {
        id: u64,
        text: String,
    },
}

/// Microphone access (cpal in the app).
pub trait Recorder: Send + Sync {
    fn start(&self, options: RecordOptions, on_level: LevelCallback) -> Result<Box<dyn Recording>, String>;
}

/// A recording in progress.
pub trait Recording: Send {
    fn started(&self) -> RecordingStart;
    /// The first `max_ms` captured so far, as 16 kHz mono; recording goes on.
    fn peek(&self, max_ms: u64) -> Option<AudioClip>;
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

/// The start of a recording, to transcribe while it goes on.
pub struct PrefixJob {
    pub id: u64,
    pub clip: AudioClip,
}

/// A prefix transcription in flight; aborted when dropped, so it ends with its recording.
pub struct PrefixTask(Option<Box<dyn FnOnce() + Send>>);

impl PrefixTask {
    pub fn new(abort: impl FnOnce() + Send + 'static) -> Self {
        Self(Some(Box::new(abort)))
    }
}

impl Drop for PrefixTask {
    fn drop(&mut self) {
        if let Some(abort) = self.0.take() {
            abort();
        }
    }
}

/// Starts producing transcript prefixes of a recording, sent back as `ControllerMsg::Prefix`.
pub type SpawnPrefix = Box<dyn Fn(Arc<Services>, PrefixJob, Sender<ControllerMsg>) -> PrefixTask + Send>;

/// Physical key state (`scribe_platform::is_key_pressed` in the app).
pub type KeyState = Arc<dyn Fn(u32) -> bool + Send + Sync>;

pub struct ControllerDeps {
    pub recorder: Arc<dyn Recorder>,
    pub clock: Clock,
    pub spawn_processing: SpawnProcessing,
    pub spawn_prefix: SpawnPrefix,
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

/// The overlay warning of a recording: exclusive access was asked for but not obtained.
pub fn microphone_warning(options: RecordOptions, started: RecordingStart) -> Option<RecordingWarning> {
    (options.exclusive_microphone && !started.exclusive_microphone).then_some(RecordingWarning::SharedMicrophone)
}

/// Whether a recording peeks at its style: only a keyword style can show, and only when it will be applied.
pub fn peeks(settings: &Settings) -> bool {
    settings.correction_enabled && settings.styles.iter().any(|s| s.keyword.is_some())
}

/// Also stores each level in `latest`: the audio thread only writes an atomic, the controller reads it on ticks.
fn keep_latest_level(latest: Arc<AtomicU32>, inner: LevelCallback) -> LevelCallback {
    Arc::new(move |rms: f32| {
        latest.store(rms.to_bits(), Ordering::Relaxed);
        inner(rms);
    })
}

/// The batch peek of a recording: when to transcribe its start, and the transcription in flight.
struct Peek {
    timer: PeekTimer,
    level: Arc<AtomicU32>,
    task: Option<PrefixTask>,
}

/// A recording in progress, with what it holds until it ends.
struct Active {
    id: u64,
    handle: Box<dyn Recording>,
    /// The start focus snapshot is resolved off the controller thread (it can take up to
    /// FOCUS_TIMEOUT_MS): blocking here would delay queued ticks and break double-tap timing.
    focus_start: Receiver<FocusSnapshot>,
    /// Restores the sound when dropped, so every path that ends the recording restores it. Dropped after the
    /// microphone is stopped.
    mute: Option<MuteGuard>,
    /// Kept on the pill when the recording locks.
    warning: Option<RecordingWarning>,
    /// The keyword style peeked at, shown on the pill and then on the processing state.
    style: Option<StyleBadge>,
    peek: Option<Peek>,
}

pub struct Controller {
    svc: Arc<Services>,
    tx: Sender<ControllerMsg>,
    deps: ControllerDeps,
    gesture: GestureDetector,
    session: Session,
    mode: Mode,
    recording: Option<Active>,
    /// Id of the last recording started, so a late prefix of a previous one is ignored.
    recording_id: u64,
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
        let focus_start =
            tauri::async_runtime::spawn_blocking(move || rx.recv().unwrap_or_else(|_| FocusSnapshot::unknown()))
                .await
                .unwrap_or_else(|_| FocusSnapshot::unknown());
        dictation::process(svc, Captured { clip: job.clip, mode: job.mode, focus_start }).await;
    });
}

/// The app's `SpawnPrefix`: transcribes the clip on the async runtime and reports the text, if any.
pub fn spawn_prefix(svc: Arc<Services>, job: PrefixJob, tx: Sender<ControllerMsg>) -> PrefixTask {
    let task = tauri::async_runtime::spawn(async move {
        if let Some(text) = dictation::transcribe_prefix(&svc, job.clip).await {
            let _ = tx.send(ControllerMsg::Prefix { id: job.id, text });
        }
    });
    PrefixTask::new(move || task.abort())
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
            svc,
            tx,
            deps,
            gesture: GestureDetector::new(s.gesture.clone()),
            session: Session::new(s.max_recording_ms),
            mode: Mode::Hold,
            recording: None,
            recording_id: 0,
            released_since: None,
        }
    }

    pub fn handle(&mut self, msg: ControllerMsg) {
        self.dispatch(msg);
        self.svc.session_busy.store(self.session.state() != SessionState::Idle, Ordering::Relaxed);
    }

    fn dispatch(&mut self, msg: ControllerMsg) {
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
                self.peek_if_due(now);
            }
            ControllerMsg::Prefix { id, text } => self.show_prefix_style(id, &text),
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

    fn peek_if_due(&mut self, now: u64) {
        let Some(active) = self.recording.as_mut() else { return };
        let Some(peek) = active.peek.as_mut() else { return };
        peek.timer.on_level(now, f32::from_bits(peek.level.load(Ordering::Relaxed)));
        if !peek.timer.take_due(now) {
            return;
        }
        if let Some(clip) = active.handle.peek(PEEK_CLIP_MS) {
            let job = PrefixJob { id: active.id, clip };
            peek.task = Some((self.deps.spawn_prefix)(self.svc.clone(), job, self.tx.clone()));
        }
    }

    /// The latest prefix of the current recording decides its badge; a prefix of an ended recording is ignored.
    fn show_prefix_style(&mut self, id: u64, text: &str) {
        let Some(active) = self.recording.as_mut().filter(|a| a.id == id) else { return };
        let language = self.svc.language();
        let badge = {
            let settings = self.svc.settings.read().unwrap();
            style::match_prefix(text, &settings.styles).map(|d| StyleBadge::of(d.style, language))
        };
        if badge == active.style {
            return;
        }
        active.style = badge;
        let (warning, style) = (active.warning, active.style.clone());
        self.svc.overlay.emit(OverlayEvent::Recording { locked: self.mode == Mode::Locked, warning, style });
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
                let mut on_level = level_emitter(self.svc.ui.clone(), self.deps.clock.clone(), LEVEL_INTERVAL_MS);
                let (exclusive_microphone, peek) = {
                    let s = self.svc.settings.read().unwrap();
                    let peek = peeks(&s).then(|| Peek {
                        timer: PeekTimer::new((self.deps.clock)(), s.silence_threshold_dbfs),
                        level: Arc::new(AtomicU32::new(0)),
                        task: None,
                    });
                    (s.exclusive_microphone_during_dictation, peek)
                };
                if let Some(p) = &peek {
                    on_level = keep_latest_level(p.level.clone(), on_level);
                }
                let options = RecordOptions { exclusive_microphone };
                match self.deps.recorder.start(options, on_level) {
                    Ok(handle) => {
                        (self.svc.warm_up)(&self.svc.settings.read().unwrap());
                        self.mode = Mode::Hold;
                        let warning = microphone_warning(options, handle.started());
                        overlay.emit(OverlayEvent::Recording { locked: false, warning, style: None });
                        let mute_on = self.svc.settings.read().unwrap().mute_audio_during_dictation;
                        let mute = mute_on.then(|| self.svc.audio_mute.guard());
                        let focus_start = spawn_focus_snapshot(&self.svc);
                        self.recording_id += 1;
                        let id = self.recording_id;
                        self.recording = Some(Active { id, handle, focus_start, mute, warning, style: None, peek });
                    }
                    Err(detail) => {
                        self.session.abort();
                        self.gesture.reset();
                        overlay.toast(ToastLevel::Error, Notice::MicrophoneUnavailable { detail }, None, None);
                    }
                }
            }
            SessionAction::SetMode(mode) => {
                self.mode = mode;
                let (warning, style) = self.recording.as_ref().map_or((None, None), |a| (a.warning, a.style.clone()));
                overlay.emit(OverlayEvent::Recording { locked: mode == Mode::Locked, warning, style });
            }
            SessionAction::DiscardRecording => {
                if let Some(Active { handle, mute, peek, .. }) = self.recording.take() {
                    drop(peek);
                    let _ = handle.stop();
                    drop(mute);
                }
                overlay.emit(OverlayEvent::Idle);
            }
            SessionAction::FinishRecording => {
                let Some(Active { handle, focus_start, mute, style, peek, .. }) = self.recording.take() else {
                    self.session.on_processing_done();
                    return;
                };
                // From here only `dictation::process` decides the style.
                drop(peek);
                overlay.emit(OverlayEvent::Processing { style });
                let stopped = handle.stop();
                // Not before: the speakers would be heard at the end of the recording.
                drop(mute);
                match stopped {
                    Ok(clip) => {
                        let job = Job { clip, mode: self.mode, focus_start };
                        (self.deps.spawn_processing)(self.svc.clone(), job, self.tx.clone());
                    }
                    Err(detail) => {
                        self.session.on_processing_done();
                        overlay.toast(ToastLevel::Error, Notice::RecordingLost { detail }, None, None);
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
