//! Microphone recording, minus the device: the recording-thread protocol, sample conversions, level metering
//! and the final 16 kHz mono clip. The cpal glue is `device::microphone` (excluded from coverage).
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

use scribe_core::audio::{self, AudioClip, TARGET_RATE};

pub use crate::device::microphone::start_recording;

/// Receives the RMS (0.0–1.0) of each captured buffer, from the audio thread.
pub type LevelCallback = Arc<dyn Fn(f32) + Send + Sync>;

/// How a recording uses the microphone.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RecordOptions {
    /// Other apps get no microphone input while recording (`exclusive_mic`); falls back to shared input.
    pub exclusive_microphone: bool,
}

/// What a recording got from the microphone once started.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RecordingStart {
    /// Other apps get no microphone input (false when not asked or refused).
    pub exclusive_microphone: bool,
}

pub struct RecordingHandle {
    started: RecordingStart,
    capture: Arc<CaptureBuffer>,
    stop_tx: mpsc::Sender<()>,
    join: Option<JoinHandle<Result<AudioClip, String>>>,
}

impl RecordingHandle {
    pub fn started(&self) -> RecordingStart {
        self.started
    }

    /// The first `max_ms` captured so far, as 16 kHz mono; recording goes on.
    pub fn peek(&self, max_ms: u64) -> Option<AudioClip> {
        self.capture.peek(max_ms)
    }

    /// Stops capture and returns 16 kHz mono audio. Partial audio is kept if the device failed mid-way.
    pub fn stop(mut self) -> Result<AudioClip, String> {
        let _ = self.stop_tx.send(());
        self.join
            .take()
            .expect("recording already stopped")
            .join()
            .map_err(|_| "the recording thread panicked".to_string())?
    }
}

/// The recording thread body: `(stop, ready) -> clip`. Boxed so that the device glue does not instantiate
/// `spawn_recorder` again (an untested generic copy would count as uncovered lines).
pub type RecorderBody = Box<
    dyn FnOnce(mpsc::Receiver<()>, mpsc::Sender<Result<RecordingStart, String>>) -> Result<AudioClip, String> + Send,
>;

/// Runs `body` on a dedicated recording thread. `body` reports through `ready` how capture started
/// (`Err` aborts the start), records into `capture` until `stop` receives, then returns the clip.
pub fn spawn_recorder(capture: Arc<CaptureBuffer>, body: RecorderBody) -> Result<RecordingHandle, String> {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<RecordingStart, String>>();
    let join = std::thread::Builder::new()
        .name("scribe-recorder".into())
        .spawn(move || body(stop_rx, ready_tx))
        .map_err(|e| e.to_string())?;
    match ready_rx.recv() {
        Ok(Ok(started)) => Ok(RecordingHandle { started, capture, stop_tx, join: Some(join) }),
        Ok(Err(e)) => {
            let _ = join.join();
            Err(e)
        }
        Err(_) => Err("the recording thread stopped".into()),
    }
}

pub fn i16_to_f32(data: &[i16]) -> Vec<f32> {
    data.iter().map(|s| *s as f32 / 32768.0).collect()
}

pub fn u16_to_f32(data: &[u16]) -> Vec<f32> {
    data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0).collect()
}

/// Root mean square of a buffer (0 for an empty one).
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Shared between the device callbacks (samples, stream errors) and the recording thread (final clip).
pub struct CaptureBuffer {
    samples: Mutex<Vec<f32>>,
    error: Mutex<Option<String>>,
    /// (channels, rate) of the open input, once known.
    format: Mutex<Option<(u16, u32)>>,
    on_level: LevelCallback,
}

fn to_clip(interleaved: &[f32], channels: u16, rate: u32) -> AudioClip {
    let mono = audio::downmix_to_mono(interleaved, channels);
    let resampled = audio::resample_linear(&mono, rate, TARGET_RATE);
    AudioClip { samples: audio::to_i16(&resampled), sample_rate: TARGET_RATE }
}

impl CaptureBuffer {
    pub fn new(on_level: LevelCallback) -> Self {
        Self { samples: Mutex::new(Vec::new()), error: Mutex::new(None), format: Mutex::new(None), on_level }
    }

    pub fn set_format(&self, channels: u16, rate: u32) {
        *self.format.lock().unwrap_or_else(|p| p.into_inner()) = Some((channels, rate));
    }

    /// Non-draining: the first `max_ms` captured, as 16 kHz mono. None before the format is known or when empty.
    pub fn peek(&self, max_ms: u64) -> Option<AudioClip> {
        let (channels, rate) = (*self.format.lock().unwrap_or_else(|p| p.into_inner()))?;
        let frames = (rate as u64).saturating_mul(max_ms) / 1000;
        let len = usize::try_from(frames.saturating_mul(channels.max(1) as u64)).unwrap_or(usize::MAX);
        let head = {
            let samples = self.samples.lock().unwrap_or_else(|p| p.into_inner());
            samples[..len.min(samples.len())].to_vec()
        };
        let clip = to_clip(&head, channels, rate);
        (!clip.samples.is_empty()).then_some(clip)
    }

    /// Appends interleaved samples and reports their level; empty buffers are ignored.
    pub fn push(&self, samples: Vec<f32>) {
        if samples.is_empty() {
            return;
        }
        (self.on_level)(rms(&samples));
        self.samples.lock().unwrap_or_else(|p| p.into_inner()).extend(samples);
    }

    /// Records a stream error (e.g. the microphone was unplugged); the last one wins.
    pub fn set_error(&self, e: String) {
        *self.error.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
    }

    /// Downmixes and resamples everything captured to 16 kHz mono. After a stream error, the partial audio
    /// is kept; the error is returned only if nothing was captured.
    pub fn finish(&self, channels: u16, rate: u32) -> Result<AudioClip, String> {
        let interleaved = std::mem::take(&mut *self.samples.lock().unwrap_or_else(|p| p.into_inner()));
        let clip = to_clip(&interleaved, channels, rate);
        match self.error.lock().unwrap_or_else(|p| p.into_inner()).take() {
            Some(e) if clip.samples.is_empty() => Err(format!("microphone disconnected: {e}")),
            _ => Ok(clip),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn levels() -> (LevelCallback, Arc<Mutex<Vec<f32>>>) {
        let seen = Arc::new(Mutex::new(vec![]));
        let s = seen.clone();
        (Arc::new(move |l| s.lock().unwrap().push(l)), seen)
    }

    fn buffer() -> Arc<CaptureBuffer> {
        Arc::new(CaptureBuffer::new(Arc::new(|_| {})))
    }

    fn clip(samples: Vec<i16>) -> AudioClip {
        AudioClip { samples, sample_rate: TARGET_RATE }
    }

    #[test]
    fn converts_integer_samples_to_unit_floats() {
        assert_eq!(i16_to_f32(&[0, 16384, -32768, 32767]), vec![0.0, 0.5, -1.0, 32767.0 / 32768.0]);
        assert_eq!(u16_to_f32(&[32768, 49152, 0, 65535]), vec![0.0, 0.5, -1.0, 32767.0 / 32768.0]);
        assert!(i16_to_f32(&[]).is_empty());
    }

    #[test]
    fn rms_of_buffers() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.5, -0.5]), 0.5);
        assert!((rms(&[1.0, 0.0]) - 0.5f32.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn push_reports_one_level_per_non_empty_buffer() {
        let (on_level, seen) = levels();
        let buf = CaptureBuffer::new(on_level);
        buf.push(vec![0.5, -0.5]);
        buf.push(vec![]);
        buf.push(vec![0.0, 0.0]);
        assert_eq!(*seen.lock().unwrap(), vec![0.5, 0.0]);
        let c = buf.finish(1, TARGET_RATE).unwrap();
        assert_eq!(c.samples, vec![16384, -16384, 0, 0]);
    }

    #[test]
    fn finish_downmixes_and_resamples_to_16k_mono() {
        let buf = CaptureBuffer::new(Arc::new(|_| {}));
        // 48 kHz stereo, 6 frames: left 1.0 / right 0.0 → mono 0.5, decimated by 3 → 2 samples.
        buf.push([1.0, 0.0].repeat(6));
        let c = buf.finish(2, 48_000).unwrap();
        assert_eq!(c.sample_rate, TARGET_RATE);
        assert_eq!(c.samples, vec![16384, 16384]);
        assert_eq!(buf.finish(2, 48_000).unwrap().samples, Vec::<i16>::new(), "finish drains the buffer");
    }

    #[test]
    fn stream_error_keeps_partial_audio() {
        let buf = CaptureBuffer::new(Arc::new(|_| {}));
        buf.push(vec![0.5]);
        buf.set_error("débranché".into());
        assert_eq!(buf.finish(1, TARGET_RATE), Ok(clip(vec![16384])));
    }

    #[test]
    fn stream_error_with_nothing_captured_is_an_error() {
        let buf = CaptureBuffer::new(Arc::new(|_| {}));
        buf.set_error("premier".into());
        buf.set_error("débranché".into());
        assert_eq!(buf.finish(1, TARGET_RATE), Err("microphone disconnected: débranché".into()));
    }

    #[test]
    fn nothing_captured_without_error_is_an_empty_clip() {
        let buf = CaptureBuffer::new(Arc::new(|_| {}));
        assert_eq!(buf.finish(1, 44_100), Ok(clip(vec![])));
    }

    #[test]
    fn peek_is_none_before_the_format_is_known_or_when_empty() {
        let buf = buffer();
        buf.push(vec![0.5; 32]);
        assert_eq!(buf.peek(1_000), None);
        let empty = buffer();
        empty.set_format(1, TARGET_RATE);
        assert_eq!(empty.peek(1_000), None);
    }

    #[test]
    fn peek_returns_the_start_as_16k_mono_without_draining() {
        let buf = buffer();
        buf.set_format(2, 32_000);
        // 1 ms at 32 kHz stereo = 32 frames: left 1.0 / right 0.0, then 32 silent frames.
        buf.push([1.0, 0.0].repeat(32));
        buf.push(vec![0.0; 64]);
        let head = buf.peek(1).unwrap();
        assert_eq!(head, clip(vec![16384; 16]), "capped to max_ms, downmixed and resampled");
        assert_eq!(buf.peek(10).unwrap().samples.len(), 32, "everything when shorter than max_ms");
        assert_eq!(buf.finish(2, 32_000).unwrap().samples.len(), 32, "peek does not drain");
    }

    #[test]
    fn the_handle_peeks_into_the_recording_buffer() {
        let capture = buffer();
        let c = capture.clone();
        let h = spawn_recorder(
            capture,
            Box::new(move |stop, ready| {
                c.set_format(1, TARGET_RATE);
                c.push(vec![0.5; 4]);
                ready.send(Ok(RecordingStart::default())).unwrap();
                stop.recv().unwrap();
                c.finish(1, TARGET_RATE)
            }),
        )
        .unwrap();
        assert_eq!(h.peek(3_000), Some(clip(vec![16384; 4])));
        assert_eq!(h.stop(), Ok(clip(vec![16384; 4])));
    }

    #[test]
    fn recorder_runs_until_stopped_and_returns_the_clip() {
        let (stopped_tx, stopped_rx) = mpsc::channel();
        let h = spawn_recorder(
            buffer(),
            Box::new(move |stop, ready| {
                ready.send(Ok(RecordingStart::default())).unwrap();
                stop.recv().unwrap();
                stopped_tx.send("stopped").unwrap();
                Ok(clip(vec![1, 2, 3]))
            }),
        )
        .unwrap();
        assert!(stopped_rx.recv_timeout(Duration::from_millis(100)).is_err(), "still recording before stop");
        assert_eq!(h.stop(), Ok(clip(vec![1, 2, 3])));
        assert_eq!(stopped_rx.recv().unwrap(), "stopped");
    }

    #[test]
    fn the_handle_tells_how_the_recording_started() {
        let exclusive = RecordingStart { exclusive_microphone: true };
        let h = spawn_recorder(
            buffer(),
            Box::new(move |stop, ready| {
                ready.send(Ok(exclusive)).unwrap();
                stop.recv().unwrap();
                Ok(clip(vec![]))
            }),
        )
        .unwrap();
        assert_eq!(h.started(), exclusive);
        h.stop().unwrap();
    }

    #[test]
    fn start_failure_is_returned_and_the_thread_joined() {
        let finished = Arc::new(Mutex::new(false));
        let f = finished.clone();
        let r = spawn_recorder(
            buffer(),
            Box::new(move |_, ready| {
                ready.send(Err("no microphone found".into())).unwrap();
                std::thread::sleep(Duration::from_millis(20));
                *f.lock().unwrap() = true;
                Err("no microphone found".into())
            }),
        );
        assert_eq!(r.err(), Some("no microphone found".to_string()));
        assert!(*finished.lock().unwrap(), "the recording thread has ended before start returns");
    }

    #[test]
    fn thread_ending_before_ready_is_an_error() {
        let r = spawn_recorder(buffer(), Box::new(|_, _| Ok(clip(vec![]))));
        assert_eq!(r.err(), Some("the recording thread stopped".to_string()));
        let r = spawn_recorder(buffer(), Box::new(|_, _| panic!("device glue panicked")));
        assert_eq!(r.err(), Some("the recording thread stopped".to_string()));
    }

    #[test]
    fn panic_while_recording_is_reported_by_stop() {
        let h = spawn_recorder(
            buffer(),
            Box::new(|stop, ready| {
                ready.send(Ok(RecordingStart::default())).unwrap();
                stop.recv().unwrap();
                panic!("cpal callback panicked");
            }),
        )
        .unwrap();
        assert_eq!(h.stop(), Err("the recording thread panicked".into()));
    }

    #[test]
    fn device_failure_after_start_still_returns_its_audio() {
        // The thread may end on its own (stream error): stop must not hang and returns what it produced.
        let h = spawn_recorder(
            buffer(),
            Box::new(|_, ready| {
                ready.send(Ok(RecordingStart::default())).unwrap();
                Ok(clip(vec![7]))
            }),
        )
        .unwrap();
        assert_eq!(h.stop(), Ok(clip(vec![7])));
    }
}
