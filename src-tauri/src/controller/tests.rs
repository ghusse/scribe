use std::collections::HashSet;
use std::sync::atomic::AtomicU32;
use std::sync::Mutex;

use scribe_core::audio::TARGET_RATE;

use scribe_core::chord;
use scribe_platform::key_filter::KeyFilter;
use scribe_platform::RawKey;

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
    /// Turns raw keys into hook events as the real hook does.
    filter: KeyFilter,
    /// Physical key state, as `GetAsyncKeyState` reports it.
    pressed: Arc<Mutex<HashSet<u32>>>,
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
        let pressed = Arc::new(Mutex::new(HashSet::new()));
        let state = pressed.clone();
        let deps = ControllerDeps {
            recorder: rec.clone(),
            clock: Arc::new(move || clock_now.load(Ordering::SeqCst)),
            spawn_processing: Box::new(move |_svc, job, _tx| spawned.lock().unwrap().push(job)),
            is_pressed: Arc::new(move |vk| state.lock().unwrap().contains(&vk)),
        };
        let (tx, done_rx) = mpsc::channel();
        let c = Controller::new(f.svc.clone(), tx, deps);
        Self { f, c, rec, now, jobs, filter: KeyFilter::new(), pressed, _done_rx: done_rx }
    }

    fn key(&mut self, vk: u32, down: bool, t_ms: u64) {
        self.now.store(t_ms, Ordering::SeqCst);
        // The hook runs before the key state is updated.
        let d = {
            let pressed = self.pressed.lock().unwrap();
            self.filter.on_event(&self.f.svc.hook_cfg, vk, 0, down, false, t_ms, &|k| pressed.contains(&k))
        };
        self.set_pressed(vk, down);
        self.c.handle(ControllerMsg::Key(d.event.unwrap()));
    }

    /// Changes the physical state only: the hook never sees it (key-up lost).
    fn set_pressed(&self, vk: u32, down: bool) {
        let mut pressed = self.pressed.lock().unwrap();
        if down {
            pressed.insert(vk);
        } else {
            pressed.remove(&vk);
        }
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
fn a_combination_works_like_a_single_trigger_key() {
    let (ctrl, shift, a) = (0xA2, 0xA0, 0x41);
    let mut h = Harness::with(Fixture::with_settings(Settings { trigger_keys: vec![ctrl, shift, a], ..Default::default() }));
    h.key(ctrl, true, 0);
    h.key(shift, true, 10);
    assert_eq!(h.started(), 0, "incomplete combination");
    h.key(a, true, 20);
    assert_eq!(h.started(), 1);
    h.key(a, true, 60); // auto-repeat
    h.key(LOCK, true, 100);
    h.key(LOCK, false, 120);
    h.key(shift, false, 900); // releasing any key of the combination
    h.key(a, false, 910);
    h.key(ctrl, false, 920);
    assert_eq!(h.stopped(), 0, "locked by the lock key");
    for (vk, t) in [(ctrl, 2_000), (shift, 2_010), (a, 2_020)] {
        h.key(vk, true, t);
    }
    assert_eq!(h.job_modes(), vec![Mode::Locked], "pressing the combination again stops");
}

#[test]
fn hold_records_then_processes_in_hold_mode() {
    let mut h = Harness::new();
    h.key(TRIGGER, true, 0);
    assert_eq!(h.started(), 1);
    assert_eq!(h.last_overlay(), Some(OverlayEvent::Recording { locked: false }));
    h.key(0x41, true, 100); // other keys do nothing
    h.key(0x41, false, 150);
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
fn microphone_failure_resets_the_detector_for_a_quick_retry() {
    let mut h = Harness::new();
    *h.rec.start_error.lock().unwrap() = Some("aucun périphérique".into());
    h.key(TRIGGER, true, 0);
    h.key(TRIGGER, false, 100);
    // Microphone back; the user presses again inside the double-tap window.
    // A detector left in its tap state would read this as a lock gesture and
    // swallow the press instead of starting a recording.
    *h.rec.start_error.lock().unwrap() = None;
    h.key(TRIGGER, true, 200);
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
    h.key(TRIGGER, false, 1_500);
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
fn a_press_whose_key_up_is_lost_is_discarded_after_a_grace_period() {
    let mut h = Harness::new();
    h.key(TRIGGER, true, 0);
    h.tick(400);
    h.set_pressed(TRIGGER, false); // Win+L: the key-up goes to the secure desktop
    h.tick(1_000);
    assert_eq!(h.stopped(), 0, "a real key-up may still be queued");
    h.tick(1_499);
    assert_eq!(h.stopped(), 0);
    h.tick(1_500);
    assert_eq!(h.stopped(), 1, "discarded");
    assert!(h.jobs.lock().unwrap().is_empty(), "never pasted");
    assert_eq!(h.last_overlay(), Some(OverlayEvent::Idle));
    // After the unlock, the next press records again.
    h.key(TRIGGER, true, 60_000);
    assert_eq!(h.started(), 2);
}

#[test]
fn the_lost_release_timer_restarts_when_the_key_is_seen_down_again() {
    let mut h = Harness::new();
    h.key(TRIGGER, true, 0);
    h.set_pressed(TRIGGER, false);
    h.tick(100);
    h.set_pressed(TRIGGER, true);
    h.tick(400);
    h.set_pressed(TRIGGER, false);
    h.tick(700);
    h.tick(1_100);
    assert_eq!(h.stopped(), 0, "up for 400 ms only since it was last seen down");
    h.tick(1_200);
    assert_eq!(h.stopped(), 1);
}

#[test]
fn a_locked_recording_survives_a_released_trigger() {
    let mut h = Harness::new();
    h.key(TRIGGER, true, 0);
    h.key(TRIGGER, false, 100);
    h.key(TRIGGER, true, 200);
    h.key(TRIGGER, false, 250); // locked, hands free
    h.tick(2_000);
    h.tick(3_000);
    assert_eq!(h.stopped(), 0);
}

#[test]
fn a_hotkey_capture_completes_while_paused() {
    let mut h = Harness::new();
    h.f.svc.hook_cfg.paused.store(true, Ordering::Relaxed);
    let (_, rx) = h.f.svc.key_capture.begin();
    h.key(0x77, true, 0);
    h.key(0x77, false, 10);
    assert_eq!(rx.try_recv(), Ok(vec![0x77]));
    assert!(!h.f.svc.hook_cfg.capturing.load(Ordering::Relaxed), "the keyboard is given back");
}

#[test]
fn releasing_a_trigger_held_before_a_capture_still_stops_the_dictation() {
    let mut h = Harness::new();
    h.key(TRIGGER, true, 0);
    let (_, _rx) = h.f.svc.key_capture.begin();
    h.key(TRIGGER, false, 800);
    assert_eq!(h.job_modes(), vec![Mode::Hold]);
    assert!(h.f.svc.key_capture.is_pending());
}

#[test]
fn a_pending_hotkey_capture_takes_every_key_until_the_combination_is_released() {
    let mut h = Harness::new();
    h.f.svc.hook_cfg.paused.store(true, Ordering::Relaxed);
    let (_, rx) = h.f.svc.key_capture.begin();
    // The release of the key that started the capture (Enter on the button) must not end it.
    h.key(0x0D, false, 0);
    assert!(h.f.svc.key_capture.is_pending(), "a stray key-up does not end the capture");
    h.f.svc.hook_cfg.paused.store(false, Ordering::Relaxed);
    h.key(TRIGGER, true, 10);
    h.key(0x41, true, 20);
    assert_eq!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    h.key(TRIGGER, false, 30);
    h.key(0x41, false, 40);
    assert_eq!(rx.try_recv(), Ok(vec![0xA2, 0x41]), "Ctrl droit + A is stored as side-less Ctrl + A");
    assert!(!h.f.svc.key_capture.is_pending());
    assert_eq!(h.started(), 0, "the current trigger pressed during a capture records nothing");
    h.key(TRIGGER, true, 1_000);
    assert_eq!(h.started(), 1, "keys are gestures again");
}

#[test]
fn settings_change_updates_the_hook_without_breaking_the_recording() {
    let mut h = Harness::new();
    h.key(TRIGGER, true, 0);
    h.key(TRIGGER, false, 100);
    h.key(TRIGGER, true, 200);
    h.key(TRIGGER, false, 250); // locked
    *h.f.svc.settings.write().unwrap() =
        Settings { trigger_keys: vec![0xA2], lock_vk: 0, max_recording_ms: 20_000, ..Default::default() };
    h.c.handle(ControllerMsg::SettingsChanged);
    assert_eq!(h.f.svc.hook_cfg.trigger.load(Ordering::Relaxed), chord::pack(&[0xA2]));
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
        is_pressed: Arc::new(|_| true),
    };
    spawn(f.svc.clone(), rx, tx.clone(), deps);
    let (_, capture) = f.svc.key_capture.begin();
    for down in [true, false] {
        tx.send(ControllerMsg::Key(HookEvent { key: RawKey { vk: 0x42, down, repeat: false, t_ms: 0 }, gestures: vec![] })).unwrap();
    }
    assert_eq!(capture.recv_timeout(Duration::from_secs(5)), Ok(vec![0x42]));
}
