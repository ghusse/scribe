use std::sync::atomic::AtomicU32;
use std::sync::Mutex;

use scribe_core::audio::TARGET_RATE;

use super::*;
use crate::overlay::OverlayEvent;
use crate::settings::Settings;
use crate::testing::{editable, Fixture, UiCall};

const TRIGGER: u32 = 0xA3;
const LOCK: u32 = 0x20;

#[derive(Default)]
struct FakeRecorder {
    start_error: Mutex<Option<String>>,
    stop_error: Mutex<Option<String>>,
    started: AtomicU32,
    stopped: Arc<AtomicU32>,
    on_level: Mutex<Option<LevelCallback>>,
}

struct FakeRecording {
    error: Option<String>,
    stopped: Arc<AtomicU32>,
}

impl Recording for FakeRecording {
    fn stop(self: Box<Self>) -> Result<AudioClip, String> {
        self.stopped.fetch_add(1, Ordering::SeqCst);
        match self.error {
            Some(e) => Err(e),
            None => Ok(AudioClip { samples: vec![1; 16], sample_rate: TARGET_RATE }),
        }
    }
}

impl Recorder for FakeRecorder {
    fn start(&self, on_level: LevelCallback) -> Result<Box<dyn Recording>, String> {
        if let Some(e) = self.start_error.lock().unwrap().clone() {
            return Err(e);
        }
        self.started.fetch_add(1, Ordering::SeqCst);
        *self.on_level.lock().unwrap() = Some(on_level);
        Ok(Box::new(FakeRecording { error: self.stop_error.lock().unwrap().clone(), stopped: self.stopped.clone() }))
    }
}

struct Harness {
    f: Fixture,
    c: Controller,
    rec: Arc<FakeRecorder>,
    now: Arc<AtomicU64>,
    jobs: Arc<Mutex<Vec<Job>>>,
    _done_rx: Receiver<ControllerMsg>,
}

impl Harness {
    fn new() -> Self {
        Self::with(Fixture::new())
    }

    fn with(f: Fixture) -> Self {
        let rec = Arc::new(FakeRecorder::default());
        let now = Arc::new(AtomicU64::new(0));
        let jobs = Arc::new(Mutex::new(Vec::new()));
        let clock_now = now.clone();
        let spawned = jobs.clone();
        let deps = ControllerDeps {
            recorder: rec.clone(),
            clock: Arc::new(move || clock_now.load(Ordering::SeqCst)),
            spawn_processing: Box::new(move |_svc, job, _tx| spawned.lock().unwrap().push(job)),
        };
        let (tx, done_rx) = mpsc::channel();
        let c = Controller::new(f.svc.clone(), tx, deps);
        Self { f, c, rec, now, jobs, _done_rx: done_rx }
    }

    fn key(&mut self, vk: u32, down: bool, t_ms: u64) {
        self.now.store(t_ms, Ordering::SeqCst);
        self.c.handle(ControllerMsg::Key(RawKey { vk, down, t_ms }));
    }

    fn tick(&mut self, t_ms: u64) {
        self.now.store(t_ms, Ordering::SeqCst);
        self.c.handle(ControllerMsg::Tick);
    }

    fn started(&self) -> u32 {
        self.rec.started.load(Ordering::SeqCst)
    }

    fn stopped(&self) -> u32 {
        self.rec.stopped.load(Ordering::SeqCst)
    }

    fn last_overlay(&self) -> Option<OverlayEvent> {
        self.f.window.last_event()
    }

    fn job_modes(&self) -> Vec<Mode> {
        self.jobs.lock().unwrap().iter().map(|j| j.mode).collect()
    }
}

#[test]
fn processing_done_is_sent_even_on_panic() {
    let (tx, rx) = mpsc::channel();
    let r = std::panic::catch_unwind(move || {
        let _done = ProcessingDoneGuard(tx);
        panic!("process failed");
    });
    assert!(r.is_err());
    assert!(matches!(rx.try_recv(), Ok(ControllerMsg::ProcessingDone)));
    assert!(rx.try_recv().is_err());
}

#[test]
fn maps_virtual_keys_to_roles() {
    assert_eq!(key_role(0xA3, 0xA3, 0x20), KeyRole::Trigger);
    assert_eq!(key_role(0x20, 0xA3, 0x20), KeyRole::Lock);
    assert_eq!(key_role(0x41, 0xA3, 0x20), KeyRole::Other);
    assert_eq!(key_role(0x20, 0xA3, 0), KeyRole::Other);
}

#[test]
fn hold_records_then_processes_in_hold_mode() {
    let mut h = Harness::new();
    h.key(TRIGGER, true, 0);
    assert_eq!(h.started(), 1);
    assert_eq!(h.last_overlay(), Some(OverlayEvent::Recording { locked: false }));
    h.key(0x41, true, 100); // other keys do nothing
    h.key(TRIGGER, false, 800);
    assert_eq!(h.stopped(), 1);
    assert_eq!(h.last_overlay(), Some(OverlayEvent::Processing));
    assert_eq!(h.job_modes(), vec![Mode::Hold]);
    let job = h.jobs.lock().unwrap().pop().unwrap();
    assert_eq!(job.clip.samples.len(), 16);
    assert_eq!(job.focus_start.recv_timeout(Duration::from_secs(5)).unwrap(), editable("Notepad"), "start focus resolved off-thread");

    // A press during processing is ignored; once done, the next press records again.
    h.key(TRIGGER, true, 1_000);
    h.key(TRIGGER, false, 1_900);
    assert_eq!(h.started(), 1);
    h.c.handle(ControllerMsg::ProcessingDone);
    h.key(TRIGGER, true, 3_000);
    assert_eq!(h.started(), 2);
}

#[test]
fn lock_key_switches_to_locked_mode() {
    let mut h = Harness::new();
    h.key(TRIGGER, true, 0);
    h.key(LOCK, true, 50);
    assert_eq!(h.last_overlay(), Some(OverlayEvent::Recording { locked: true }));
    h.key(LOCK, false, 60);
    h.key(TRIGGER, false, 900);
    assert_eq!(h.stopped(), 0, "still recording once locked");
    h.key(TRIGGER, true, 5_000);
    assert_eq!(h.job_modes(), vec![Mode::Locked]);
}

#[test]
fn unavailable_microphone_aborts_and_resets() {
    let mut h = Harness::new();
    *h.rec.start_error.lock().unwrap() = Some("aucun périphérique".into());
    h.key(TRIGGER, true, 0);
    assert_eq!(
        h.last_overlay(),
        Some(OverlayEvent::Toast { level: ToastLevel::Error, message: "Micro indisponible : aucun périphérique".into(), preview: None, dictation_id: None })
    );
    h.key(TRIGGER, false, 800);
    assert!(h.jobs.lock().unwrap().is_empty());
    // Session and detector were reset: the next press starts a recording.
    *h.rec.start_error.lock().unwrap() = None;
    h.key(TRIGGER, true, 2_000);
    assert_eq!(h.started(), 1);
    assert_eq!(h.last_overlay(), Some(OverlayEvent::Recording { locked: false }));
}

#[test]
fn lost_recording_is_reported_and_the_session_goes_idle() {
    let mut h = Harness::new();
    *h.rec.stop_error.lock().unwrap() = Some("micro débranché".into());
    h.key(TRIGGER, true, 0);
    h.key(TRIGGER, false, 800);
    assert_eq!(
        h.last_overlay(),
        Some(OverlayEvent::Toast { level: ToastLevel::Error, message: "Enregistrement perdu : micro débranché".into(), preview: None, dictation_id: None })
    );
    assert!(h.jobs.lock().unwrap().is_empty());
    h.key(TRIGGER, true, 2_000);
    assert_eq!(h.started(), 2, "not stuck in Processing");
}

#[test]
fn recording_stops_at_the_maximum_duration() {
    let f = Fixture::with_settings(Settings { max_recording_ms: 10_000, ..Default::default() });
    let mut h = Harness::with(f);
    h.key(TRIGGER, true, 0);
    h.key(TRIGGER, false, 100);
    h.key(TRIGGER, true, 200); // double tap: locked
    h.key(TRIGGER, false, 250);
    h.tick(9_999);
    assert!(h.jobs.lock().unwrap().is_empty());
    h.tick(10_000);
    assert_eq!(h.job_modes(), vec![Mode::Locked]);
    assert_eq!(h.last_overlay(), Some(OverlayEvent::Processing));
    // The detector was reset: the next press after processing starts a new recording.
    h.c.handle(ControllerMsg::ProcessingDone);
    h.key(TRIGGER, true, 11_000);
    assert_eq!(h.started(), 2);
}

#[test]
fn a_short_tap_alone_is_discarded_after_the_double_tap_window() {
    let mut h = Harness::new();
    h.key(TRIGGER, true, 0);
    h.key(TRIGGER, false, 100);
    h.tick(300);
    assert_eq!(h.stopped(), 0, "still waiting for a second tap");
    h.tick(500);
    assert_eq!(h.stopped(), 1);
    assert_eq!(h.last_overlay(), Some(OverlayEvent::Idle));
    assert!(h.jobs.lock().unwrap().is_empty());
}

#[test]
fn pausing_discards_the_recording_and_ignores_keys() {
    let mut h = Harness::new();
    h.key(TRIGGER, true, 0);
    h.f.svc.hook_cfg.paused.store(true, Ordering::Relaxed);
    h.c.handle(ControllerMsg::PauseChanged(true));
    assert_eq!(h.stopped(), 1);
    assert_eq!(h.last_overlay(), Some(OverlayEvent::Idle));
    h.key(TRIGGER, false, 800);
    h.key(TRIGGER, true, 1_000);
    assert_eq!(h.started(), 1, "keys are ignored while paused");
    assert!(h.jobs.lock().unwrap().is_empty());

    // Pausing when idle, and resuming, change nothing on screen.
    let shown = h.f.window.events().len();
    h.c.handle(ControllerMsg::PauseChanged(true));
    h.f.svc.hook_cfg.paused.store(false, Ordering::Relaxed);
    h.c.handle(ControllerMsg::PauseChanged(false));
    assert_eq!(h.f.window.events().len(), shown);
    h.key(TRIGGER, true, 2_000);
    assert_eq!(h.started(), 2);
}

#[test]
fn a_pending_hotkey_capture_takes_the_press() {
    let mut h = Harness::new();
    h.f.svc.hook_cfg.paused.store(true, Ordering::Relaxed);
    let (_, rx) = h.f.svc.key_capture.begin();
    h.key(TRIGGER, false, 0); // key-ups are not captured
    h.key(TRIGGER, true, 10);
    assert_eq!(rx.try_recv(), Ok(TRIGGER));
    assert_eq!(h.started(), 0);
}

#[test]
fn settings_change_updates_the_hook_without_breaking_the_recording() {
    let mut h = Harness::new();
    h.key(TRIGGER, true, 0);
    h.key(TRIGGER, false, 100);
    h.key(TRIGGER, true, 200);
    h.key(TRIGGER, false, 250); // locked
    *h.f.svc.settings.write().unwrap() = Settings { trigger_vk: 0xA2, lock_vk: 0, max_recording_ms: 20_000, ..Default::default() };
    h.c.handle(ControllerMsg::SettingsChanged);
    assert_eq!(h.f.svc.hook_cfg.trigger_vk.load(Ordering::Relaxed), 0xA2);
    assert_eq!(h.f.svc.hook_cfg.lock_vk.load(Ordering::Relaxed), 0);
    assert_eq!(h.stopped(), 0);
    h.tick(19_999);
    assert!(h.jobs.lock().unwrap().is_empty());
    // The new trigger stops the locked recording.
    h.key(0xA2, true, 15_000);
    assert_eq!(h.job_modes(), vec![Mode::Locked]);
}

#[test]
fn finishing_without_a_recording_releases_the_session() {
    let mut h = Harness::new();
    h.c.apply_action(SessionAction::FinishRecording);
    assert!(h.jobs.lock().unwrap().is_empty());
    assert!(h.f.window.events().is_empty());
    h.key(TRIGGER, true, 0);
    assert_eq!(h.started(), 1);
}

#[test]
fn recording_levels_reach_the_ui_throttled() {
    let mut h = Harness::new();
    h.key(TRIGGER, true, 0);
    let on_level = h.rec.on_level.lock().unwrap().clone().unwrap();
    on_level(0.5);
    h.now.store(20, Ordering::SeqCst);
    on_level(0.6);
    h.now.store(50, Ordering::SeqCst);
    on_level(0.7);
    assert_eq!(h.f.ui.calls(), vec![UiCall::Level(0.5), UiCall::Level(0.7)]);
}

#[test]
fn level_emitter_lets_one_level_through_per_interval() {
    let f = Fixture::new();
    let now = Arc::new(AtomicU64::new(1_000));
    let n = now.clone();
    let emit = level_emitter(f.ui.clone(), Arc::new(move || n.load(Ordering::SeqCst)), 50);
    for (t, rms) in [(1_000, 0.1), (1_049, 0.2), (1_050, 0.3), (1_100, 0.4), (1_120, 0.5)] {
        now.store(t, Ordering::SeqCst);
        emit(rms);
    }
    assert_eq!(f.ui.calls(), vec![UiCall::Level(0.1), UiCall::Level(0.3), UiCall::Level(0.4)]);
}

#[test]
fn toggle_pause_flips_the_hook_and_tells_the_controller() {
    let f = Fixture::new();
    assert!(toggle_pause(&f.svc));
    assert!(f.svc.hook_cfg.paused.load(Ordering::Relaxed));
    assert!(matches!(f.ctrl_rx.try_recv(), Ok(ControllerMsg::PauseChanged(true))));
    assert!(!toggle_pause(&f.svc));
    assert!(!f.svc.hook_cfg.paused.load(Ordering::Relaxed));
    assert!(matches!(f.ctrl_rx.try_recv(), Ok(ControllerMsg::PauseChanged(false))));
}

#[test]
fn spawn_processing_runs_the_dictation_and_reports_done() {
    let f = Fixture::new();
    let (tx, rx) = mpsc::channel();
    let (focus_tx, focus_rx) = mpsc::channel();
    drop(focus_tx); // unresolved focus: falls back to unknown
    let clip = AudioClip { samples: vec![0; 160], sample_rate: TARGET_RATE };
    spawn_processing(f.svc.clone(), Job { clip, mode: Mode::Hold, focus_start: focus_rx }, tx);
    assert!(matches!(rx.recv_timeout(Duration::from_secs(10)), Ok(ControllerMsg::ProcessingDone)));
    assert_eq!(f.window.last_event(), Some(OverlayEvent::Idle), "too short: dropped");
}

#[test]
fn spawned_controller_handles_messages_on_its_thread() {
    let f = Fixture::new();
    let (tx, rx) = mpsc::channel();
    let deps = ControllerDeps {
        recorder: Arc::new(FakeRecorder::default()),
        clock: Arc::new(|| 0),
        spawn_processing: Box::new(|_, _, _| {}),
    };
    spawn(f.svc.clone(), rx, tx.clone(), deps);
    let (_, capture) = f.svc.key_capture.begin();
    tx.send(ControllerMsg::Key(RawKey { vk: 0x42, down: true, t_ms: 0 })).unwrap();
    assert_eq!(capture.recv_timeout(Duration::from_secs(5)), Ok(0x42));
}
