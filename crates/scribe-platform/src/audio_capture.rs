use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use scribe_core::audio::{self, AudioClip, TARGET_RATE};

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

/// cpal streams are not `Send` on every platform, so each recording owns a dedicated thread.
pub fn start_recording(on_level: LevelCallback) -> Result<RecordingHandle, String> {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let join = std::thread::Builder::new()
        .name("scribe-recorder".into())
        .spawn(move || record_thread(stop_rx, ready_tx, on_level))
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

fn push(buffer: &Mutex<Vec<f32>>, on_level: &LevelCallback, samples: Vec<f32>) {
    if samples.is_empty() {
        return;
    }
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    on_level(rms);
    buffer.lock().unwrap().extend(samples);
}

fn record_thread(
    stop_rx: mpsc::Receiver<()>,
    ready_tx: mpsc::Sender<Result<(), String>>,
    on_level: LevelCallback,
) -> Result<AudioClip, String> {
    let fail = |e: String| {
        let _ = ready_tx.send(Err(e.clone()));
        Err(e)
    };
    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else { return fail("aucun micro détecté".into()) };
    let supported = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => return fail(format!("micro inutilisable : {e}")),
    };
    let channels = supported.channels();
    let rate = supported.sample_rate().0;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
    let stream_error = Arc::new(Mutex::new(None::<String>));
    let err_slot = stream_error.clone();
    let err_fn = move |e: cpal::StreamError| {
        *err_slot.lock().unwrap() = Some(e.to_string());
    };
    let (b, l) = (buffer.clone(), on_level.clone());
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| push(&b, &l, data.to_vec()),
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                push(&b, &l, data.iter().map(|s| *s as f32 / 32768.0).collect())
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                push(&b, &l, data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0).collect())
            },
            err_fn,
            None,
        ),
        other => return fail(format!("format audio non pris en charge : {other:?}")),
    };
    let stream = match stream {
        Ok(s) => s,
        Err(e) => return fail(format!("ouverture du micro impossible : {e}")),
    };
    if let Err(e) = stream.play() {
        return fail(format!("démarrage du micro impossible : {e}"));
    }
    let _ = ready_tx.send(Ok(()));

    let _ = stop_rx.recv();
    drop(stream);

    let interleaved = std::mem::take(&mut *buffer.lock().unwrap());
    let mono = audio::downmix_to_mono(&interleaved, channels);
    let resampled = audio::resample_linear(&mono, rate, TARGET_RATE);
    let clip = AudioClip { samples: audio::to_i16(&resampled), sample_rate: TARGET_RATE };
    let stream_error = stream_error.lock().unwrap().take();
    match stream_error {
        Some(e) if clip.samples.is_empty() => Err(format!("micro déconnecté : {e}")),
        _ => Ok(clip),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a real microphone: `cargo test -p scribe-platform -- --ignored records_from_default_mic`
    #[test]
    #[ignore]
    fn records_from_default_mic() {
        let h = start_recording(Arc::new(|_| {})).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1_000));
        let clip = h.stop().unwrap();
        assert_eq!(clip.sample_rate, TARGET_RATE);
        assert!(audio::duration_ms(&clip) >= 900);
    }
}
