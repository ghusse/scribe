//! The default microphone through cpal. Only device glue: the recording thread protocol, sample
//! conversions, level metering and the final clip are in `crate::audio_capture`, the choice between exclusive
//! and shared input in `crate::exclusive_mic` (tested).
use std::sync::{mpsc, Arc};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use scribe_core::audio::AudioClip;

use crate::audio_capture::{self, CaptureBuffer, LevelCallback, RecordOptions, RecordingHandle, RecordingStart};
use crate::exclusive_mic::{self, InputOpener, OpenStream};

/// cpal streams are not `Send` on every platform, so each recording owns a dedicated thread.
pub fn start_recording(options: RecordOptions, on_level: LevelCallback) -> Result<RecordingHandle, String> {
    audio_capture::spawn_recorder(Box::new(move |stop_rx, ready_tx| record(options, stop_rx, ready_tx, on_level)))
}

fn record(
    options: RecordOptions,
    stop_rx: mpsc::Receiver<()>,
    ready_tx: mpsc::Sender<Result<RecordingStart, String>>,
    on_level: LevelCallback,
) -> Result<AudioClip, String> {
    let capture = Arc::new(CaptureBuffer::new(on_level));
    let exclusive = crate::exclusive_input();
    let (open, started) =
        match exclusive_mic::open_input(options.exclusive_microphone, &exclusive, &CpalInput, capture.clone()) {
            Ok(opened) => opened,
            Err(e) => {
                let _ = ready_tx.send(Err(e.clone()));
                return Err(e);
            }
        };
    let _ = ready_tx.send(Ok(started));

    let _ = stop_rx.recv();
    drop(open.stream);
    capture.finish(open.channels, open.rate)
}

/// Shared access to the default input, through cpal.
pub struct CpalInput;

impl InputOpener for CpalInput {
    fn open(&self, capture: Arc<CaptureBuffer>) -> Result<OpenStream, String> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or("aucun micro détecté")?;
        let supported = device.default_input_config().map_err(|e| format!("micro inutilisable : {e}"))?;
        let channels = supported.channels();
        let rate = supported.sample_rate().0;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();

        let c = capture.clone();
        let err_fn = move |e: cpal::StreamError| c.set_error(e.to_string());
        let b = capture;
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| b.push(data.to_vec()),
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| b.push(audio_capture::i16_to_f32(data)),
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| b.push(audio_capture::u16_to_f32(data)),
                err_fn,
                None,
            ),
            other => return Err(format!("format audio non pris en charge : {other:?}")),
        };
        let stream = stream.map_err(|e| format!("ouverture du micro impossible : {e}"))?;
        stream.play().map_err(|e| format!("démarrage du micro impossible : {e}"))?;
        Ok(OpenStream { channels, rate, stream: Box::new(stream) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scribe_core::audio::{self, TARGET_RATE};

    fn records_one_second(options: RecordOptions) -> RecordingStart {
        let h = start_recording(options, Arc::new(|_| {})).unwrap();
        let started = h.started();
        std::thread::sleep(std::time::Duration::from_millis(1_000));
        let clip = h.stop().unwrap();
        assert_eq!(clip.sample_rate, TARGET_RATE);
        assert!(audio::duration_ms(&clip) >= 900);
        started
    }

    /// Needs a real microphone: `cargo test -p scribe-platform -- --ignored records_from_default_mic`
    #[test]
    #[ignore]
    fn records_from_default_mic() {
        assert_eq!(records_one_second(RecordOptions::default()), RecordingStart::default());
    }

    /// Needs a real microphone: `cargo test -p scribe-platform -- --ignored records_with_exclusive_mic`
    #[test]
    #[ignore]
    fn records_with_exclusive_mic() {
        let started = records_one_second(RecordOptions { exclusive_microphone: true });
        assert_eq!(started.exclusive_microphone, cfg!(any(target_os = "macos", windows)));
    }

    /// Needs a real microphone: `cargo test -p scribe-platform -- --ignored --test-threads=1 exclusive_mic_holds_the_hog`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn exclusive_mic_holds_the_hog_while_recording() {
        use crate::exclusive_mic::HogBackend;
        use crate::macos::audio_input::CoreAudioHog;
        let inputs: Vec<u32> = CoreAudioHog.inputs().unwrap();
        let owners = || inputs.iter().map(|d| (*d, CoreAudioHog.owner(*d))).collect::<Vec<_>>();
        let physical = |d: &u32| ![*b"grup", *b"virt"].map(u32::from_be_bytes).contains(&CoreAudioHog.transport(*d).unwrap());
        assert!(owners().iter().all(|(_, o)| *o == Ok(-1)), "{:?}", owners());
        let h = start_recording(RecordOptions { exclusive_microphone: true }, Arc::new(|_| {})).unwrap();
        let ours = Ok(std::process::id() as i32);
        assert!(owners().iter().all(|(d, o)| !physical(d) || *o == ours), "{:?}", owners());
        std::thread::sleep(std::time::Duration::from_millis(3_000));
        assert!(!h.stop().unwrap().samples.is_empty());
        assert!(owners().iter().all(|(_, o)| *o == Ok(-1)), "{:?}", owners());
        // The device restarts for a few ms once given back: the next test would find it unusable.
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
