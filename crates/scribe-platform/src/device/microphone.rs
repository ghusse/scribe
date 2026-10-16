//! The default microphone through cpal. Only device glue: the recording thread protocol, sample
//! conversions, level metering and the final clip are in `crate::audio_capture` (tested).
use std::sync::{mpsc, Arc};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use scribe_core::audio::AudioClip;

use crate::audio_capture::{self, CaptureBuffer, LevelCallback, RecordingHandle};

/// cpal streams are not `Send` on every platform, so each recording owns a dedicated thread.
pub fn start_recording(on_level: LevelCallback) -> Result<RecordingHandle, String> {
    let capture = Arc::new(CaptureBuffer::new(on_level));
    let c = capture.clone();
    audio_capture::spawn_recorder(capture, Box::new(move |stop_rx, ready_tx| record(stop_rx, ready_tx, c)))
}

fn record(
    stop_rx: mpsc::Receiver<()>,
    ready_tx: mpsc::Sender<Result<(), String>>,
    capture: Arc<CaptureBuffer>,
) -> Result<AudioClip, String> {
    let (stream, channels, rate) = match open(capture.clone()) {
        Ok(opened) => opened,
        Err(e) => {
            let _ = ready_tx.send(Err(e.clone()));
            return Err(e);
        }
    };
    capture.set_format(channels, rate);
    let _ = ready_tx.send(Ok(()));

    let _ = stop_rx.recv();
    drop(stream);
    capture.finish(channels, rate)
}

/// Opens and plays the default input; returns the stream with its channel count and sample rate.
fn open(capture: Arc<CaptureBuffer>) -> Result<(cpal::Stream, u16, u32), String> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("no microphone found")?;
    let supported = device.default_input_config().map_err(|e| format!("unusable microphone: {e}"))?;
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
        other => return Err(format!("unsupported sample format: {other:?}")),
    };
    let stream = stream.map_err(|e| format!("cannot open the microphone: {e}"))?;
    stream.play().map_err(|e| format!("cannot start the microphone: {e}"))?;
    Ok((stream, channels, rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use scribe_core::audio::{self, TARGET_RATE};

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
