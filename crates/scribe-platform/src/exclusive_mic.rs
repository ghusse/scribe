//! Keeping the microphone to ourselves while recording: which capture path opens the input (exclusive, else
//! shared) and the macOS hog-mode rules. Opening a device is an [`InputOpener`]: shared cpal in
//! `device::microphone`; exclusive access per platform (`exclusive_input()`: CoreAudio hog mode on macOS, WASAPI
//! exclusive mode on Windows), all excluded from coverage.
use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use crate::audio_capture::{CaptureBuffer, RecordingStart};

/// An open input feeding a [`CaptureBuffer`]. Capture stops (and any exclusive access is given back) when
/// `stream` drops.
pub struct OpenStream {
    pub channels: u16,
    pub rate: u32,
    pub stream: Box<dyn Any>,
}

/// Opens the default input on the recording thread (cpal streams are not `Send`).
pub trait InputOpener {
    fn open(&self, capture: Arc<CaptureBuffer>) -> Result<OpenStream, String>;
}

/// Reserves the device the shared input already runs on; given back when the returned value drops.
pub trait ExclusiveHold {
    fn hold(&self) -> Result<Box<dyn Any>, String>;
}

/// How a platform keeps the microphone to itself.
pub enum ExclusiveAccess {
    /// Its own exclusive input, replacing the shared one (WASAPI exclusive mode).
    Input(Box<dyn InputOpener>),
    /// A reservation taken on the running shared input (CoreAudio hog mode).
    Hold(Box<dyn ExclusiveHold>),
    Unsupported,
}

/// After a hold is given back the device restarts for a moment, and opening it then fails.
const SETTLE_ATTEMPTS: u32 = 5;
const SETTLE_DELAY: Duration = Duration::from_millis(50);

/// Opens the input exclusively when asked to, else (or when exclusive access fails) shared: exclusive access
/// never costs a dictation. The start tells which one was obtained.
pub fn open_input(
    exclusive: bool,
    access: &ExclusiveAccess,
    shared: &dyn InputOpener,
    capture: Arc<CaptureBuffer>,
) -> Result<(OpenStream, RecordingStart), String> {
    let refused = |e: String| tracing::warn!("accès exclusif au micro impossible, enregistrement partagé : {e}");
    let held = RecordingStart { exclusive_microphone: true };
    let not_held = RecordingStart { exclusive_microphone: false };
    match (exclusive, access) {
        (false, _) => {}
        (true, ExclusiveAccess::Input(opener)) => match opener.open(capture.clone()) {
            Ok(stream) => return Ok((stream, held)),
            Err(e) => refused(e),
        },
        (true, ExclusiveAccess::Hold(hold)) => {
            let open = open_retrying(shared, capture, SETTLE_ATTEMPTS, SETTLE_DELAY)?;
            return Ok(match hold.hold() {
                // Tuple fields drop in order: the stream stops before the hold is given back.
                Ok(guard) => (OpenStream { stream: Box::new((open.stream, guard)), ..open }, held),
                Err(e) => {
                    refused(e);
                    (open, not_held)
                }
            });
        }
        (true, ExclusiveAccess::Unsupported) => refused("non disponible sur cette plateforme".into()),
    }
    shared.open(capture).map(|stream| (stream, not_held))
}

fn open_retrying(
    opener: &dyn InputOpener,
    capture: Arc<CaptureBuffer>,
    attempts: u32,
    delay: Duration,
) -> Result<OpenStream, String> {
    let mut left = attempts;
    loop {
        left -= 1;
        match opener.open(capture.clone()) {
            Err(_) if left > 0 => std::thread::sleep(delay),
            result => return result,
        }
    }
}

/// CoreAudio hog mode (`kAudioDevicePropertyHogMode`), one raw call per method. Setting the property toggles
/// it: taken by us when free, given back when ours, unchanged when another process holds it.
pub trait HogBackend {
    fn default_input(&self) -> Result<u32, String>;
    /// Every device with an input stream.
    fn inputs(&self) -> Result<Vec<u32>, String>;
    /// `kAudioDevicePropertyTransportType`.
    fn transport(&self, device: u32) -> Result<u32, String>;
    /// Whether the device also plays sound (headset, interface): hog mode reserves the whole device.
    fn has_output(&self, device: u32) -> Result<bool, String>;
    /// Pid of the process holding the device, -1 when free.
    fn owner(&self, device: u32) -> Result<i32, String>;
    fn toggle(&self, device: u32) -> Result<(), String>;
}

/// `kAudioDeviceTransportTypeAggregate`, `kAudioDeviceTransportTypeVirtual`: no microphone of their own, and no
/// hog mode.
const UNHOGGABLE_TRANSPORTS: [u32; 2] = [u32::from_be_bytes(*b"grup"), u32::from_be_bytes(*b"virt")];

fn own_pid() -> i32 {
    std::process::id() as i32
}

/// Every physical input held in hog mode, so that an app losing our microphone cannot switch to another one;
/// each given back on drop, only if we still hold it.
pub struct HogGuard<B: HogBackend> {
    backend: B,
    held: Vec<u32>,
}

impl<B: HogBackend> HogGuard<B> {
    /// The unused inputs first and ours last: an app cut off from ours finds no free microphone to switch to.
    /// On any refusal, what was taken is given back.
    pub fn acquire(backend: B) -> Result<Self, String> {
        let ours = backend.default_input()?;
        let mut order = Vec::new();
        for device in backend.inputs()? {
            if device != ours && !UNHOGGABLE_TRANSPORTS.contains(&backend.transport(device)?) {
                order.push(device);
            }
        }
        order.push(ours);
        let mut guard = Self { backend, held: Vec::new() };
        for device in order {
            guard.take(device)?;
        }
        Ok(guard)
    }

    fn take(&mut self, device: u32) -> Result<(), String> {
        if self.backend.has_output(device)? {
            return Err("micro intégré à un appareil de sortie audio, dont le son serait coupé aussi".into());
        }
        let owner = self.backend.owner(device)?;
        if owner != own_pid() {
            if owner != -1 {
                return Err(format!("micro déjà réservé par le processus {owner}"));
            }
            self.backend.toggle(device)?;
        }
        // Recorded before checking, so that a hog taken but misreported is still given back.
        self.held.push(device);
        match self.backend.owner(device)? {
            pid if pid == own_pid() => Ok(()),
            _ => Err("le micro refuse l'accès exclusif".into()),
        }
    }
}

impl<B: HogBackend> Drop for HogGuard<B> {
    fn drop(&mut self) {
        for &device in self.held.iter().rev() {
            match self.backend.owner(device) {
                Ok(pid) if pid == own_pid() => {
                    if let Err(e) = self.backend.toggle(device) {
                        tracing::warn!("impossible de rendre le micro {device} aux autres applications : {e}");
                    }
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("état du micro {device} illisible, accès exclusif non rendu : {e}"),
            }
        }
    }
}

/// Hog mode as an [`ExclusiveHold`]: taken once the shared stream runs, since an open right after taking the hog
/// fails about half the time (the device restarts).
pub struct HogMode<B>(pub B);

impl<B: HogBackend + Clone + 'static> ExclusiveHold for HogMode<B> {
    fn hold(&self) -> Result<Box<dyn Any>, String> {
        Ok(Box::new(HogGuard::acquire(self.0.clone())?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    type Log = Arc<Mutex<Vec<String>>>;

    fn capture() -> Arc<CaptureBuffer> {
        Arc::new(CaptureBuffer::new(Arc::new(|_| {})))
    }

    fn entries(log: &Log) -> Vec<String> {
        log.lock().unwrap().clone()
    }

    /// Opens a stream described by `result` (after `failures` refused opens), logging `name` on each call and
    /// `drop:name` when it stops. Each open pushes one sample.
    struct Opener {
        name: &'static str,
        result: Result<(u16, u32), &'static str>,
        failures: Mutex<u32>,
        log: Log,
    }

    struct Logged(&'static str, Log);

    impl Drop for Logged {
        fn drop(&mut self) {
            self.1.lock().unwrap().push(format!("drop:{}", self.0));
        }
    }

    impl InputOpener for Opener {
        fn open(&self, capture: Arc<CaptureBuffer>) -> Result<OpenStream, String> {
            self.log.lock().unwrap().push(self.name.into());
            let mut failures = self.failures.lock().unwrap();
            if *failures > 0 {
                *failures -= 1;
                return Err("le micro redémarre".into());
            }
            let (channels, rate) = self.result.map_err(String::from)?;
            capture.push(vec![0.5]);
            Ok(OpenStream { channels, rate, stream: Box::new(Logged(self.name, self.log.clone())) })
        }
    }

    fn opener(name: &'static str, result: Result<(u16, u32), &'static str>, log: &Log) -> Opener {
        Opener { name, result, failures: Mutex::new(0), log: log.clone() }
    }

    /// An exclusive input and a shared one, sharing one log.
    fn openers(
        exclusive: Result<(u16, u32), &'static str>,
        shared: Result<(u16, u32), &'static str>,
    ) -> (ExclusiveAccess, Opener, Log) {
        let log = Log::default();
        (ExclusiveAccess::Input(Box::new(opener("exclusive", exclusive, &log))), opener("shared", shared, &log), log)
    }

    #[test]
    fn shared_input_when_exclusive_is_not_asked() {
        let (ex, sh, log) = openers(Ok((2, 48_000)), Ok((1, 44_100)));
        let (s, start) = open_input(false, &ex, &sh, capture()).unwrap();
        assert_eq!((s.channels, s.rate, start.exclusive_microphone), (1, 44_100, false));
        assert_eq!(entries(&log), ["shared"]);
    }

    #[test]
    fn exclusive_input_when_asked_and_granted() {
        let (ex, sh, log) = openers(Ok((2, 48_000)), Ok((1, 44_100)));
        let (s, start) = open_input(true, &ex, &sh, capture()).unwrap();
        assert_eq!((s.channels, s.rate, start.exclusive_microphone), (2, 48_000, true));
        drop(s.stream);
        assert_eq!(entries(&log), ["exclusive", "drop:exclusive"]);
    }

    #[test]
    fn falls_back_to_shared_input_when_exclusive_access_fails() {
        let (ex, sh, log) = openers(Err("device in use"), Ok((1, 44_100)));
        let (s, start) = open_input(true, &ex, &sh, capture()).unwrap();
        assert_eq!((s.channels, s.rate, start.exclusive_microphone), (1, 44_100, false));
        assert_eq!(entries(&log), ["exclusive", "shared"]);
    }

    #[test]
    fn the_shared_error_is_returned_when_nothing_opens() {
        let (ex, sh, _) = openers(Err("device in use"), Err("aucun micro détecté"));
        assert_eq!(open_input(true, &ex, &sh, capture()).err(), Some("aucun micro détecté".into()));
    }

    #[test]
    fn unsupported_exclusive_access_records_shared() {
        let (_, sh, log) = openers(Ok((1, 1)), Ok((1, 16_000)));
        let (s, start) = open_input(true, &ExclusiveAccess::Unsupported, &sh, capture()).unwrap();
        assert_eq!((s.rate, start.exclusive_microphone), (16_000, false));
        assert_eq!(entries(&log), ["shared"]);
    }

    /// A hold logging `hold` when taken and `release` when given back, or refused with `refusal`.
    struct FakeHold {
        refusal: Option<&'static str>,
        log: Log,
    }

    struct Released(Log);

    impl Drop for Released {
        fn drop(&mut self) {
            self.0.lock().unwrap().push("release".into());
        }
    }

    impl ExclusiveHold for FakeHold {
        fn hold(&self) -> Result<Box<dyn Any>, String> {
            if let Some(e) = self.refusal {
                return Err(e.into());
            }
            self.log.lock().unwrap().push("hold".into());
            Ok(Box::new(Released(self.log.clone())))
        }
    }

    fn held(refusal: Option<&'static str>, shared: Result<(u16, u32), &'static str>) -> (ExclusiveAccess, Opener, Log) {
        let log = Log::default();
        (ExclusiveAccess::Hold(Box::new(FakeHold { refusal, log: log.clone() })), opener("shared", shared, &log), log)
    }

    #[test]
    fn a_hold_is_taken_on_the_shared_stream_and_given_back_after_it_stops() {
        let (access, sh, log) = held(None, Ok((2, 48_000)));
        let c = capture();
        let (s, start) = open_input(true, &access, &sh, c.clone()).unwrap();
        assert_eq!((s.channels, s.rate, start.exclusive_microphone), (2, 48_000, true));
        drop(s.stream);
        assert_eq!(entries(&log), ["shared", "hold", "drop:shared", "release"]);
        assert_eq!(c.finish(1, 16_000).unwrap().samples.len(), 1);
    }

    #[test]
    fn a_refused_hold_keeps_the_shared_stream_without_reopening_it() {
        let (access, sh, log) = held(Some("micro déjà réservé"), Ok((1, 44_100)));
        let c = capture();
        let (s, start) = open_input(true, &access, &sh, c.clone()).unwrap();
        assert_eq!((s.rate, start.exclusive_microphone), (44_100, false));
        assert_eq!(entries(&log), ["shared"]);
        assert_eq!(c.finish(1, 16_000).unwrap().samples.len(), 1, "one open, no stray samples");
    }

    #[test]
    fn a_device_restarting_after_a_hold_is_waited_for() {
        let (access, sh, log) = held(None, Ok((1, 48_000)));
        *sh.failures.lock().unwrap() = SETTLE_ATTEMPTS - 1;
        let (_, start) = open_input(true, &access, &sh, capture()).unwrap();
        assert!(start.exclusive_microphone);
        assert_eq!(entries(&log).iter().filter(|e| *e == "shared").count(), SETTLE_ATTEMPTS as usize);
    }

    #[test]
    fn a_shared_input_that_never_opens_gives_up_without_holding() {
        let (access, sh, log) = held(None, Ok((1, 48_000)));
        *sh.failures.lock().unwrap() = SETTLE_ATTEMPTS;
        assert_eq!(open_input(true, &access, &sh, capture()).err(), Some("le micro redémarre".into()));
        assert_eq!(entries(&log), vec!["shared"; SETTLE_ATTEMPTS as usize]);
    }

    const OURS: u32 = 7;
    const OTHER: u32 = 8;
    const PHYSICAL: u32 = u32::from_be_bytes(*b"bltn");

    /// One input in hog mode: `owner` is its pid (-1 = free). `toggle` follows CoreAudio unless `stuck` (accepted,
    /// nothing changes).
    #[derive(Clone)]
    struct Dev {
        owner: i32,
        output: bool,
        stuck: bool,
        transport: u32,
    }

    impl Default for Dev {
        fn default() -> Self {
            Dev { owner: -1, output: false, stuck: false, transport: PHYSICAL }
        }
    }

    /// Inputs by id, `OURS` being the default. `broken` fails the given call names; `toggles` logs each toggled id.
    #[derive(Clone, Default)]
    struct FakeHog(Arc<Mutex<HogState>>);

    #[derive(Default)]
    struct HogState {
        devices: std::collections::BTreeMap<u32, Dev>,
        broken: Vec<&'static str>,
        toggles: Vec<u32>,
    }

    impl FakeHog {
        fn with(devices: &[(u32, Dev)]) -> Self {
            let hog = FakeHog::default();
            hog.state().devices = devices.iter().cloned().collect();
            hog
        }
        /// Our input alone, held by `owner`.
        fn owned_by(owner: i32) -> Self {
            Self::with(&[(OURS, Dev { owner, ..Dev::default() })])
        }
        fn free() -> Self {
            Self::owned_by(-1)
        }
        fn state(&self) -> std::sync::MutexGuard<'_, HogState> {
            self.0.lock().unwrap()
        }
        fn owner(&self, device: u32) -> i32 {
            self.state().devices[&device].owner
        }
        fn toggles(&self) -> Vec<u32> {
            self.state().toggles.clone()
        }
        fn dev(&self, call: &str, device: u32) -> Result<Dev, String> {
            let s = self.state();
            if s.broken.contains(&call) {
                return Err(format!("{call} failed"));
            }
            Ok(s.devices[&device].clone())
        }
    }

    impl HogBackend for FakeHog {
        fn default_input(&self) -> Result<u32, String> {
            if self.state().broken.contains(&"default_input") {
                Err("default_input failed".into())
            } else {
                Ok(OURS)
            }
        }
        fn inputs(&self) -> Result<Vec<u32>, String> {
            let s = self.state();
            if s.broken.contains(&"inputs") {
                Err("inputs failed".into())
            } else {
                Ok(s.devices.keys().copied().collect())
            }
        }
        fn transport(&self, device: u32) -> Result<u32, String> {
            self.dev("transport", device).map(|d| d.transport)
        }
        fn has_output(&self, device: u32) -> Result<bool, String> {
            self.dev("has_output", device).map(|d| d.output)
        }
        fn owner(&self, device: u32) -> Result<i32, String> {
            self.dev("owner", device).map(|d| d.owner)
        }
        fn toggle(&self, device: u32) -> Result<(), String> {
            self.dev("toggle", device)?;
            let mut s = self.state();
            s.toggles.push(device);
            let d = s.devices.get_mut(&device).unwrap();
            if !d.stuck {
                d.owner = match d.owner {
                    -1 => own_pid(),
                    pid if pid == own_pid() => -1,
                    other => other,
                };
            }
            Ok(())
        }
    }

    #[test]
    fn a_free_microphone_is_held_then_given_back() {
        let hog = FakeHog::free();
        let guard = HogGuard::acquire(hog.clone()).unwrap();
        assert_eq!(hog.owner(OURS), own_pid());
        drop(guard);
        assert_eq!((hog.owner(OURS), hog.toggles()), (-1, vec![OURS, OURS]));
    }

    #[test]
    fn the_unused_microphones_are_held_before_ours_and_given_back_after_it() {
        let hog = FakeHog::with(&[(OURS, Dev::default()), (OTHER, Dev::default()), (9, Dev::default())]);
        let guard = HogGuard::acquire(hog.clone()).unwrap();
        assert_eq!(hog.toggles(), [OTHER, 9, OURS]);
        drop(guard);
        assert_eq!(hog.toggles(), [OTHER, 9, OURS, OURS, 9, OTHER]);
        assert!([OURS, OTHER, 9].iter().all(|d| hog.owner(*d) == -1));
    }

    #[test]
    fn virtual_and_aggregate_inputs_are_skipped() {
        let hog = FakeHog::with(&[
            (OURS, Dev::default()),
            (OTHER, Dev { transport: u32::from_be_bytes(*b"virt"), ..Dev::default() }),
            (9, Dev { transport: u32::from_be_bytes(*b"grup"), ..Dev::default() }),
        ]);
        let _guard = HogGuard::acquire(hog.clone()).unwrap();
        assert_eq!(hog.toggles(), [OURS]);
    }

    #[test]
    fn a_refused_unused_microphone_gives_back_what_was_taken_and_leaves_ours_free() {
        let hog = FakeHog::with(&[
            (OURS, Dev::default()),
            (OTHER, Dev::default()),
            (9, Dev { owner: own_pid() + 1, ..Dev::default() }),
        ]);
        assert_eq!(
            HogGuard::acquire(hog.clone()).err(),
            Some(format!("micro déjà réservé par le processus {}", own_pid() + 1))
        );
        assert_eq!(hog.toggles(), [OTHER, OTHER]);
        assert_eq!((hog.owner(OURS), hog.owner(OTHER)), (-1, -1));
    }

    #[test]
    fn an_unused_headset_that_also_plays_sound_is_a_refusal() {
        let hog = FakeHog::with(&[(OURS, Dev::default()), (OTHER, Dev { output: true, ..Dev::default() })]);
        assert!(HogGuard::acquire(hog.clone()).err().unwrap().contains("sortie audio"));
        assert!(hog.toggles().is_empty());
    }

    #[test]
    fn a_microphone_held_by_another_process_is_left_alone() {
        let hog = FakeHog::owned_by(own_pid() + 1);
        assert_eq!(
            HogGuard::acquire(hog.clone()).err(),
            Some(format!("micro déjà réservé par le processus {}", own_pid() + 1))
        );
        assert!(hog.toggles().is_empty());
    }

    #[test]
    fn a_microphone_already_ours_is_not_toggled_but_given_back() {
        let hog = FakeHog::owned_by(own_pid());
        let guard = HogGuard::acquire(hog.clone()).unwrap();
        assert!(hog.toggles().is_empty());
        drop(guard);
        assert_eq!((hog.owner(OURS), hog.toggles()), (-1, vec![OURS]));
    }

    #[test]
    fn a_microphone_that_also_plays_sound_is_not_held() {
        let hog = FakeHog::with(&[(OURS, Dev { output: true, ..Dev::default() })]);
        assert!(HogGuard::acquire(hog.clone()).err().unwrap().contains("sortie audio"));
        assert!(hog.toggles().is_empty());
    }

    #[test]
    fn a_device_without_hog_mode_is_refused_and_nothing_is_given_back() {
        let hog = FakeHog::with(&[(OURS, Dev { stuck: true, ..Dev::default() })]);
        assert_eq!(HogGuard::acquire(hog.clone()).err(), Some("le micro refuse l'accès exclusif".into()));
        assert_eq!((hog.owner(OURS), hog.toggles()), (-1, vec![OURS]));
    }

    #[test]
    fn errors_while_acquiring_are_returned() {
        for call in ["default_input", "inputs", "transport", "has_output", "owner", "toggle"] {
            let hog = FakeHog::with(&[(OURS, Dev::default()), (OTHER, Dev::default())]);
            hog.state().broken.push(call);
            assert_eq!(HogGuard::acquire(hog.clone()).err(), Some(format!("{call} failed")));
            assert_eq!((hog.owner(OURS), hog.owner(OTHER)), (-1, -1));
        }
    }

    #[test]
    fn a_microphone_released_meanwhile_is_not_taken_back() {
        let hog = FakeHog::free();
        let guard = HogGuard::acquire(hog.clone()).unwrap();
        hog.state().devices.get_mut(&OURS).unwrap().owner = -1;
        drop(guard);
        assert_eq!(hog.toggles(), [OURS]);
    }

    #[test]
    fn release_failures_are_only_logged() {
        for call in ["owner", "toggle"] {
            let hog = FakeHog::free();
            let guard = HogGuard::acquire(hog.clone()).unwrap();
            hog.state().broken.push(call);
            drop(guard);
            assert_eq!(hog.owner(OURS), own_pid());
        }
    }

    #[test]
    fn hog_mode_holds_the_microphone_until_the_stream_stops() {
        let hog = FakeHog::free();
        let log = Log::default();
        let access = ExclusiveAccess::Hold(Box::new(HogMode(hog.clone())));
        let (s, start) = open_input(true, &access, &opener("shared", Ok((2, 48_000)), &log), capture()).unwrap();
        assert!(start.exclusive_microphone);
        assert_eq!(hog.owner(OURS), own_pid());
        drop(s.stream);
        assert_eq!(hog.owner(OURS), -1);
    }

    #[test]
    fn hog_mode_refusals_are_returned() {
        assert!(HogMode(FakeHog::owned_by(own_pid() + 1)).hold().is_err());
    }
}
