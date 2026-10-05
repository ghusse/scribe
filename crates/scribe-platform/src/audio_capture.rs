//! Microphone recording, minus the device: the recording-thread protocol, sample conversions, level metering
//! and the final 16 kHz mono clip. The cpal glue is `device::microphone` (excluded from coverage).
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

use scribe_core::audio::{self, AudioClip, TARGET_RATE};

pub use crate::device::microphone::start_recording;

/// Receives the RMS (0.0–1.0) of each captured buffer, from the audio thread.
pub type LevelCallback = Arc<dyn Fn(f32) + Send + Sync>;

pub struct RecordingHandle {
    stop_tx: mpsc::Sender<()>,
    join: Option<JoinHandle<Result<AudioClip, String>>>,
}

impl RecordingHandle {
    /// Stops capture and returns 16 kHz mono audio. Partial audio is kept if the device failed mid-way.
    pub fn stop(mut self) -> Result<AudioClip, String> {
        let _ = self.stop_tx.send(());
        self.join
            .take()
            .expect("recording already stopped")
            .join()
            .map_err(|_| "le thread d'enregistrement a paniqué".to_string())?
    }
}

/// The recording thread body: `(stop, ready) -> clip`. Boxed so that the device glue does not instantiate
/// `spawn_recorder` again (an untested generic copy would count as uncovered lines).
pub type RecorderBody =
    Box<dyn FnOnce(mpsc::Receiver<()>, mpsc::Sender<Result<(), String>>) -> Result<AudioClip, String> + Send>;

/// Runs `body` on a dedicated recording thread. `body` reports through `ready` whether capture started
/// (`Err` aborts the start), records until `stop` receives, then returns the clip.
pub fn spawn_recorder(body: RecorderBody) -> Result<RecordingHandle, String> {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let join = std::thread::Builder::new()
        .name("scribe-recorder".into())
        .spawn(move || body(stop_rx, ready_tx))
        .map_err(|e| e.to_string())?;
    match ready_rx.recv() {
        Ok(Ok(())) => Ok(RecordingHandle { stop_tx, join: Some(join) }),
        Ok(Err(e)) => {
            let _ = join.join();
            Err(e)
        }
        Err(_) => Err("le thread d'enregistrement s'est arrêté".into()),
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
    on_level: LevelCallback,
}

impl CaptureBuffer {
    pub fn new(on_level: LevelCallback) -> Self {
        Self { samples: Mutex::new(Vec::new()), error: Mutex::new(None), on_level }
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
        let mono = audio::downmix_to_mono(&interleaved, channels);
        let resampled = audio::resample_linear(&mono, rate, TARGET_RATE);
        let clip = AudioClip { samples: audio::to_i16(&resampled), sample_rate: TARGET_RATE };
        match self.error.lock().unwrap_or_else(|p| p.into_inner()).take() {
            Some(e) if clip.samples.is_empty() => Err(format!("micro déconnecté : {e}")),
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
        assert_eq!(buf.finish(1, TARGET_RATE), Err("micro déconnecté : débranché".into()));
    }

    #[test]
    fn nothing_captured_without_error_is_an_empty_clip() {
        let buf = CaptureBuffer::new(Arc::new(|_| {}));
        assert_eq!(buf.finish(1, 44_100), Ok(clip(vec![])));
    }

    #[test]
    fn recorder_runs_until_stopped_and_returns_the_clip() {
        let (stopped_tx, stopped_rx) = mpsc::channel();
        let h = spawn_recorder(Box::new(move |stop, ready| {
            ready.send(Ok(())).unwrap();
            stop.recv().unwrap();
            stopped_tx.send("stopped").unwrap();
            Ok(clip(vec![1, 2, 3]))
        }))
        .unwrap();
        assert!(stopped_rx.recv_timeout(Duration::from_millis(100)).is_err(), "still recording before stop");
        assert_eq!(h.stop(), Ok(clip(vec![1, 2, 3])));
        assert_eq!(stopped_rx.recv().unwrap(), "stopped");
    }

    #[test]
    fn start_failure_is_returned_and_the_thread_joined() {
        let finished = Arc::new(Mutex::new(false));
        let f = finished.clone();
        let r = spawn_recorder(Box::new(move |_, ready| {
            ready.send(Err("aucun micro détecté".into())).unwrap();
            std::thread::sleep(Duration::from_millis(20));
            *f.lock().unwrap() = true;
            Err("aucun micro détecté".into())
        }));
        assert_eq!(r.err(), Some("aucun micro détecté".to_string()));
        assert!(*finished.lock().unwrap(), "the recording thread has ended before start returns");
    }

    #[test]
    fn thread_ending_before_ready_is_an_error() {
        let r = spawn_recorder(Box::new(|_, _| Ok(clip(vec![]))));
        assert_eq!(r.err(), Some("le thread d'enregistrement s'est arrêté".to_string()));
        let r = spawn_recorder(Box::new(|_, _| panic!("device glue panicked")));
        assert_eq!(r.err(), Some("le thread d'enregistrement s'est arrêté".to_string()));
    }

    #[test]
    fn panic_while_recording_is_reported_by_stop() {
        let h = spawn_recorder(Box::new(|stop, ready| {
            ready.send(Ok(())).unwrap();
            stop.recv().unwrap();
            panic!("cpal callback panicked");
        }))
        .unwrap();
        assert_eq!(h.stop(), Err("le thread d'enregistrement a paniqué".into()));
    }

    #[test]
    fn device_failure_after_start_still_returns_its_audio() {
        // The thread may end on its own (stream error): stop must not hang and returns what it produced.
        let h = spawn_recorder(Box::new(|_, ready| {
            ready.send(Ok(())).unwrap();
            Ok(clip(vec![7]))
        }))
        .unwrap();
        assert_eq!(h.stop(), Ok(clip(vec![7])));
    }
}
