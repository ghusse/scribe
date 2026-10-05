pub const TARGET_RATE: u32 = 16_000;

#[derive(Debug, Clone, PartialEq)]
pub struct AudioClip {
    /// Mono PCM samples.
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels as usize)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

/// Linear-interpolation resampler; good enough for speech-to-text input.
pub fn resample_linear(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from_hz as f64 / to_hz as f64;
    let out_len = (input.len() as f64 / ratio).floor() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos.floor() as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx];
            let b = *input.get(idx + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

pub fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16).collect()
}

pub fn duration_ms(clip: &AudioClip) -> u64 {
    if clip.sample_rate == 0 {
        return 0;
    }
    clip.samples.len() as u64 * 1000 / clip.sample_rate as u64
}

/// Energy of the loudest 20 ms frame, in dBFS. Using the loudest frame (not the global RMS)
/// keeps short utterances surrounded by silence from being classified as silent.
pub fn loudest_frame_dbfs(clip: &AudioClip) -> f32 {
    let frame = (clip.sample_rate / 50).max(1) as usize;
    clip.samples.chunks(frame).map(frame_dbfs).fold(f32::NEG_INFINITY, f32::max)
}

/// Minimum number of 20 ms frames above the threshold (60 ms in total) for a clip to count as
/// speech: a single key click or mouse click is shorter and must not reach the transcriber,
/// which tends to answer "Merci." on near-silence.
pub const MIN_VOICED_FRAMES: usize = 3;

fn frame_dbfs(f: &[i16]) -> f32 {
    let mean_sq = f.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / f.len() as f64;
    let rms = mean_sq.sqrt();
    if rms <= 0.0 { f32::NEG_INFINITY } else { (20.0 * (rms / 32768.0).log10()) as f32 }
}

pub fn is_silent(clip: &AudioClip, threshold_dbfs: f32) -> bool {
    let frame = (clip.sample_rate / 50).max(1) as usize;
    let voiced = clip.samples.chunks(frame).filter(|f| frame_dbfs(f) >= threshold_dbfs).count();
    voiced < MIN_VOICED_FRAMES
}

pub fn encode_wav(clip: &AudioClip) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: clip.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).map_err(|e| e.to_string())?;
        for &s in &clip.samples {
            writer.write_sample(s).map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;
    }
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(amplitude: f32, hz: f32, rate: u32, ms: u32) -> Vec<f32> {
        let n = (rate as u64 * ms as u64 / 1000) as usize;
        (0..n).map(|i| amplitude * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin()).collect()
    }

    fn clip(samples: &[f32]) -> AudioClip {
        AudioClip { samples: to_i16(samples), sample_rate: TARGET_RATE }
    }

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(downmix_to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix_to_mono(&[0.1, 0.2], 1), vec![0.1, 0.2]);
    }

    #[test]
    fn resample_48k_to_16k_divides_length_by_three() {
        let input = sine(0.5, 440.0, 48_000, 1000);
        let out = resample_linear(&input, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
        assert_eq!(resample_linear(&input, 16_000, 16_000).len(), input.len());
        assert!(resample_linear(&[], 48_000, 16_000).is_empty());
    }

    #[test]
    fn to_i16_clamps() {
        assert_eq!(to_i16(&[2.0, -2.0, 0.0]), vec![32767, -32767, 0]);
    }

    #[test]
    fn silence_detection() {
        assert!(is_silent(&clip(&[]), -45.0));
        assert!(is_silent(&clip(&vec![0.0; 16_000]), -45.0));
        assert!(is_silent(&clip(&sine(0.001, 300.0, TARGET_RATE, 1000)), -45.0)); // ~ -63 dBFS
        assert!(!is_silent(&clip(&sine(0.3, 300.0, TARGET_RATE, 1000)), -45.0));
    }

    #[test]
    fn short_speech_burst_inside_silence_is_not_silent() {
        let mut s = vec![0.0; 16_000];
        s.extend(sine(0.2, 300.0, TARGET_RATE, 200));
        s.extend(vec![0.0; 16_000]);
        assert!(!is_silent(&clip(&s), -45.0));
    }

    #[test]
    fn single_click_inside_silence_is_silent() {
        let mut s = vec![0.0; 16_000];
        s.extend(sine(0.5, 2_000.0, TARGET_RATE, 15)); // ~15 ms click
        s.extend(vec![0.0; 16_000]);
        let c = clip(&s);
        assert!(loudest_frame_dbfs(&c) > -45.0);
        assert!(is_silent(&c, -45.0));
    }

    #[test]
    fn duration_is_computed_from_rate() {
        assert_eq!(duration_ms(&clip(&vec![0.0; 8_000])), 500);
    }

    #[test]
    fn wav_roundtrip() {
        let c = clip(&sine(0.3, 300.0, TARGET_RATE, 100));
        let bytes = encode_wav(&c).unwrap();
        let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
        let spec = reader.spec();
        assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (1, 16_000, 16));
        let read: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(read, c.samples);
    }

    #[test]
    fn duration_of_a_clip_without_rate_is_zero() {
        assert_eq!(duration_ms(&AudioClip { samples: vec![0; 16_000], sample_rate: 0 }), 0);
    }
}
