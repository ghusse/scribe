//! WASAPI exclusive capture, minus the device: which format to ask for, how to read its samples, the buffer
//! alignment retry and the capture thread protocol. The WASAPI calls are `windows::exclusive_capture`
//! (excluded from coverage). Plain code, compiled and tested on every platform.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use crate::audio_capture;
use crate::exclusive_mic::OpenStream;

pub const WAVE_FORMAT_PCM: u16 = 1;
pub const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
pub const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
pub const KSDATAFORMAT_SUBTYPE_PCM: u128 = 0x00000001_0000_0010_8000_00aa00389b71;
pub const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: u128 = 0x00000003_0000_0010_8000_00aa00389b71;
pub const AUDCLNT_BUFFERFLAGS_SILENT: u32 = 2;
pub const AUDCLNT_E_DEVICE_INVALIDATED: i32 = 0x88890004_u32 as i32;
pub const AUDCLNT_E_UNSUPPORTED_FORMAT: i32 = 0x88890008_u32 as i32;
pub const AUDCLNT_E_DEVICE_IN_USE: i32 = 0x8889000A_u32 as i32;
pub const AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED: i32 = 0x8889000E_u32 as i32;
pub const AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED: i32 = 0x88890019_u32 as i32;

const SPEAKER_FRONT_CENTER: u32 = 0x4;
const SPEAKER_FRONT_LEFT_RIGHT: u32 = 0x3;

/// The fields of a `WAVEFORMATEX` / `WAVEFORMATEXTENSIBLE` (`valid_bits`, `sub_format` and `channel_mask` are
/// 0 for a plain `WAVEFORMATEX`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WaveDesc {
    pub tag: u16,
    pub channels: u16,
    pub rate: u32,
    pub avg_bytes_per_sec: u32,
    pub block_align: u16,
    pub bits: u16,
    pub valid_bits: u16,
    pub channel_mask: u32,
    pub sub_format: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleKind {
    I16,
    /// Packed 3-byte samples.
    I24,
    /// 32-bit container, left-justified (any valid bits).
    I32,
    F32,
}

impl SampleKind {
    fn bytes(self) -> u16 {
        match self {
            SampleKind::I16 => 2,
            SampleKind::I24 => 3,
            SampleKind::I32 | SampleKind::F32 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmFormat {
    pub kind: SampleKind,
    pub valid_bits: u16,
    pub channels: u16,
    pub rate: u32,
    pub channel_mask: u32,
}

impl PcmFormat {
    fn new(kind: SampleKind, valid_bits: u16, channels: u16, rate: u32) -> Self {
        let channel_mask = match channels {
            1 => SPEAKER_FRONT_CENTER,
            2 => SPEAKER_FRONT_LEFT_RIGHT,
            _ => 0,
        };
        Self { kind, valid_bits, channels, rate, channel_mask }
    }

    /// The capture formats we can decode; `None` for anything else (8-bit, compressed, unknown sub-format).
    pub fn from_wave(w: &WaveDesc) -> Option<Self> {
        let float = match (w.tag, w.sub_format) {
            (WAVE_FORMAT_PCM, _) | (WAVE_FORMAT_EXTENSIBLE, KSDATAFORMAT_SUBTYPE_PCM) => false,
            (WAVE_FORMAT_IEEE_FLOAT, _) | (WAVE_FORMAT_EXTENSIBLE, KSDATAFORMAT_SUBTYPE_IEEE_FLOAT) => true,
            _ => return None,
        };
        let kind = match (float, w.bits) {
            (false, 16) => SampleKind::I16,
            (false, 24) => SampleKind::I24,
            (false, 32) => SampleKind::I32,
            (true, 32) => SampleKind::F32,
            _ => return None,
        };
        if w.channels == 0 || w.rate == 0 {
            return None;
        }
        let valid_bits = if w.tag == WAVE_FORMAT_EXTENSIBLE && w.valid_bits != 0 { w.valid_bits } else { w.bits };
        let mut format = Self::new(kind, valid_bits, w.channels, w.rate);
        if w.tag == WAVE_FORMAT_EXTENSIBLE {
            format.channel_mask = w.channel_mask;
        }
        Some(format)
    }

    pub fn block_align(&self) -> u16 {
        self.channels * self.kind.bytes()
    }

    /// Always `WAVEFORMATEXTENSIBLE`: the only form that tells valid bits and channel layout.
    pub fn to_wave(&self) -> WaveDesc {
        let block_align = self.block_align();
        WaveDesc {
            tag: WAVE_FORMAT_EXTENSIBLE,
            channels: self.channels,
            rate: self.rate,
            avg_bytes_per_sec: self.rate * block_align as u32,
            block_align,
            bits: self.kind.bytes() * 8,
            valid_bits: self.valid_bits,
            channel_mask: self.channel_mask,
            sub_format: if self.kind == SampleKind::F32 {
                KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
            } else {
                KSDATAFORMAT_SUBTYPE_PCM
            },
        }
    }
}

/// Formats to try, in order: the device's own (`native`, e.g. its default format then its mix format), then
/// common capture formats. Exclusive mode gets no conversion: only a format the hardware takes as is works.
pub fn candidate_formats(native: &[PcmFormat]) -> Vec<PcmFormat> {
    let samples = [(SampleKind::I16, 16), (SampleKind::I24, 24), (SampleKind::I32, 24), (SampleKind::I32, 32), (SampleKind::F32, 32)];
    let mut all = native.to_vec();
    for rate in [48_000, 44_100] {
        for channels in [1, 2] {
            all.extend(samples.iter().map(|&(kind, valid)| PcmFormat::new(kind, valid, channels, rate)));
        }
    }
    let mut unique = Vec::with_capacity(all.len());
    for f in all {
        if !unique.contains(&f) {
            unique.push(f);
        }
    }
    unique
}

/// Interleaved little-endian samples to floats in -1.0..1.0; a trailing partial sample is dropped.
pub fn decode(kind: SampleKind, bytes: &[u8]) -> Vec<f32> {
    const I32_SCALE: f32 = 2_147_483_648.0;
    match kind {
        SampleKind::I16 => audio_capture::i16_to_f32(&bytes.as_chunks().0.iter().map(|b| i16::from_le_bytes(*b)).collect::<Vec<_>>()),
        SampleKind::I24 => bytes.as_chunks::<3>().0.iter().map(|[a, b, c]| i32::from_le_bytes([0, *a, *b, *c]) as f32 / I32_SCALE).collect(),
        SampleKind::I32 => bytes.as_chunks().0.iter().map(|b| i32::from_le_bytes(*b) as f32 / I32_SCALE).collect(),
        SampleKind::F32 => bytes.as_chunks().0.iter().map(|b| f32::from_le_bytes(*b)).collect(),
    }
}

/// One `IAudioCaptureClient::GetBuffer` packet as floats: silence when flagged so (its data must be ignored).
pub fn packet_samples(format: &PcmFormat, frames: u32, flags: u32, data: &[u8]) -> Vec<f32> {
    let len = frames as usize * format.block_align() as usize;
    if flags & AUDCLNT_BUFFERFLAGS_SILENT != 0 {
        return vec![0.0; frames as usize * format.channels as usize];
    }
    decode(format.kind, &data[..len.min(data.len())])
}

/// Duration (100 ns units) of `frames` at `rate`, as `IAudioClient::Initialize` wants it after
/// `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED`.
pub fn aligned_duration(frames: u32, rate: u32) -> i64 {
    (10_000_000.0 * frames as f64 / rate as f64 + 0.5) as i64
}

/// Why exclusive mode was refused, in the user's words.
pub fn describe_failure(code: i32, message: &str) -> String {
    match code {
        AUDCLNT_E_DEVICE_IN_USE => "micro déjà réservé par une autre application".into(),
        AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED => "mode exclusif désactivé pour ce micro dans les réglages Son de Windows".into(),
        AUDCLNT_E_DEVICE_INVALIDATED => "micro débranché ou désactivé".into(),
        AUDCLNT_E_UNSUPPORTED_FORMAT => "format audio refusé par le micro".into(),
        _ => format!("{message} ({:#010x})", code as u32),
    }
}

/// One `IAudioClient` on the default capture endpoint. A client initialises once: `renew` activates a new one.
pub trait ExclusiveClient {
    /// The device's own formats, preferred first (unreadable ones left out).
    fn native_formats(&self) -> Vec<WaveDesc>;
    /// `IsFormatSupported` in exclusive mode; `Err((HRESULT, message))`.
    fn supports(&self, format: &PcmFormat) -> Result<(), (i32, String)>;
    /// Default device period (100 ns units).
    fn period(&self) -> Result<i64, String>;
    /// Event-driven exclusive initialisation; `Err((HRESULT, message))`.
    fn initialize(&mut self, format: &PcmFormat, period: i64) -> Result<(), (i32, String)>;
    /// Buffer size (frames) a failed initialisation settled on.
    fn buffer_frames(&self) -> Result<u32, String>;
    fn renew(&mut self) -> Result<(), String>;
}

/// Errors telling about the device, not the format: no other format can work.
fn device_refused(code: i32) -> bool {
    matches!(
        code,
        AUDCLNT_E_DEVICE_IN_USE | AUDCLNT_E_DEVICE_INVALIDATED | AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED
    )
}

/// Initialises `client` in exclusive mode on the first format the device takes, retrying once with an aligned
/// buffer when the driver asks for it. A format accepted by `supports` but refused by `initialize` moves on to
/// the next one; a refusal from the device itself stops.
pub fn negotiate(client: &mut dyn ExclusiveClient) -> Result<PcmFormat, String> {
    let refused = |(code, message): (i32, String)| describe_failure(code, &message);
    let native: Vec<PcmFormat> = client.native_formats().iter().filter_map(PcmFormat::from_wave).collect();
    let period = client.period()?;
    let mut used = false;
    for format in candidate_formats(&native) {
        match client.supports(&format) {
            Ok(()) => {}
            Err((code, message)) if device_refused(code) => return Err(refused((code, message))),
            Err(_) => continue,
        }
        if std::mem::replace(&mut used, true) {
            client.renew()?;
        }
        let mut result = client.initialize(&format, period);
        if let Err((AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, _)) = result {
            let frames = client.buffer_frames()?;
            client.renew()?;
            result = client.initialize(&format, aligned_duration(frames, format.rate));
        }
        match result {
            Ok(()) => return Ok(format),
            Err((AUDCLNT_E_UNSUPPORTED_FORMAT, _)) => continue,
            Err(e) => return Err(refused(e)),
        }
    }
    Err(if used {
        describe_failure(AUDCLNT_E_UNSUPPORTED_FORMAT, "")
    } else {
        "aucun format audio accepté par le micro en mode exclusif".into()
    })
}

/// The capture thread body: reports `(channels, rate)` through `ready` (`Err` aborts the start), then captures
/// until `stop` is set.
pub type CaptureBody = Box<dyn FnOnce(mpsc::Sender<Result<(u16, u32), String>>, Arc<AtomicBool>) + Send>;

/// Stops the capture thread and waits for it to give the device back.
struct CaptureThread {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for CaptureThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Runs `body` on its own thread (COM objects stay on the thread that made them) and returns once it started.
pub fn spawn_capture(body: CaptureBody) -> Result<OpenStream, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel();
    let s = stop.clone();
    let join = std::thread::Builder::new()
        .name("scribe-exclusive-capture".into())
        .spawn(move || body(ready_tx, s))
        .map_err(|e| e.to_string())?;
    let thread = CaptureThread { stop, join: Some(join) };
    match ready_rx.recv() {
        Ok(Ok((channels, rate))) => Ok(OpenStream { channels, rate, stream: Box::new(thread) }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("le thread de capture exclusive s'est arrêté".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn extensible(sub_format: u128, bits: u16, valid_bits: u16, channels: u16, mask: u32) -> WaveDesc {
        WaveDesc {
            tag: WAVE_FORMAT_EXTENSIBLE,
            channels,
            rate: 48_000,
            bits,
            valid_bits,
            channel_mask: mask,
            sub_format,
            ..WaveDesc::default()
        }
    }

    fn plain(tag: u16, bits: u16) -> WaveDesc {
        WaveDesc { tag, channels: 2, rate: 44_100, bits, ..WaveDesc::default() }
    }

    #[test]
    fn reads_decodable_wave_formats() {
        let f = PcmFormat::from_wave(&plain(WAVE_FORMAT_PCM, 16)).unwrap();
        assert_eq!(f, PcmFormat { kind: SampleKind::I16, valid_bits: 16, channels: 2, rate: 44_100, channel_mask: 3 });
        assert_eq!(PcmFormat::from_wave(&plain(WAVE_FORMAT_PCM, 24)).unwrap().kind, SampleKind::I24);
        assert_eq!(PcmFormat::from_wave(&plain(WAVE_FORMAT_PCM, 32)).unwrap().kind, SampleKind::I32);
        assert_eq!(PcmFormat::from_wave(&plain(WAVE_FORMAT_IEEE_FLOAT, 32)).unwrap().kind, SampleKind::F32);

        let f = PcmFormat::from_wave(&extensible(KSDATAFORMAT_SUBTYPE_PCM, 32, 24, 4, 0x33)).unwrap();
        assert_eq!(f, PcmFormat { kind: SampleKind::I32, valid_bits: 24, channels: 4, rate: 48_000, channel_mask: 0x33 });
        let f = PcmFormat::from_wave(&extensible(KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, 32, 0, 1, 4)).unwrap();
        assert_eq!((f.kind, f.valid_bits), (SampleKind::F32, 32));
    }

    #[test]
    fn rejects_formats_it_cannot_decode() {
        assert_eq!(PcmFormat::from_wave(&plain(WAVE_FORMAT_PCM, 8)), None);
        assert_eq!(PcmFormat::from_wave(&plain(WAVE_FORMAT_IEEE_FLOAT, 64)), None);
        assert_eq!(PcmFormat::from_wave(&plain(0x55, 16)), None);
        assert_eq!(PcmFormat::from_wave(&extensible(0x1234, 16, 16, 1, 4)), None);
        assert_eq!(PcmFormat::from_wave(&WaveDesc { channels: 0, ..plain(WAVE_FORMAT_PCM, 16) }), None);
        assert_eq!(PcmFormat::from_wave(&WaveDesc { rate: 0, ..plain(WAVE_FORMAT_PCM, 16) }), None);
    }

    #[test]
    fn writes_an_extensible_wave_format() {
        let w = PcmFormat::new(SampleKind::I24, 24, 2, 48_000).to_wave();
        assert_eq!(w, WaveDesc {
            tag: WAVE_FORMAT_EXTENSIBLE,
            channels: 2,
            rate: 48_000,
            avg_bytes_per_sec: 288_000,
            block_align: 6,
            bits: 24,
            valid_bits: 24,
            channel_mask: 3,
            sub_format: KSDATAFORMAT_SUBTYPE_PCM,
        });
        let w = PcmFormat::new(SampleKind::F32, 32, 1, 44_100).to_wave();
        assert_eq!((w.sub_format, w.block_align, w.channel_mask), (KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, 4, 4));
        assert_eq!(PcmFormat::new(SampleKind::I16, 16, 6, 48_000).channel_mask, 0);
        let native = extensible(KSDATAFORMAT_SUBTYPE_PCM, 32, 24, 4, 0x33);
        assert_eq!(PcmFormat::from_wave(&native).unwrap().to_wave(), WaveDesc { avg_bytes_per_sec: 768_000, block_align: 16, ..native });
    }

    #[test]
    fn native_formats_come_first_then_common_ones_without_duplicates() {
        let device = PcmFormat::new(SampleKind::I32, 24, 4, 96_000);
        let common = PcmFormat::new(SampleKind::I16, 16, 1, 48_000);
        let list = candidate_formats(&[device, common, device]);
        assert_eq!(&list[..2], &[device, common]);
        assert_eq!(list[2], PcmFormat::new(SampleKind::I24, 24, 1, 48_000));
        assert_eq!(list.len(), 2 + 19);
        assert_eq!(*list.last().unwrap(), PcmFormat::new(SampleKind::F32, 32, 2, 44_100));
        assert_eq!(candidate_formats(&[]).len(), 20);
    }

    #[test]
    fn decodes_each_sample_kind() {
        assert_eq!(decode(SampleKind::I16, &[0x00, 0x40, 0x00, 0x80, 0xFF]), vec![0.5, -1.0]);
        assert_eq!(decode(SampleKind::I24, &[0x00, 0x00, 0x40, 0x00, 0x00, 0x80]), vec![0.5, -1.0]);
        assert_eq!(decode(SampleKind::I32, &[0, 0, 0, 0x40, 0, 0, 0, 0xC0]), vec![0.5, -0.5]);
        assert_eq!(decode(SampleKind::F32, &[0.25f32.to_le_bytes(), (-1.0f32).to_le_bytes()].concat()), vec![0.25, -1.0]);
        assert!(decode(SampleKind::F32, &[1, 2, 3]).is_empty());
    }

    #[test]
    fn packets_are_decoded_up_to_their_frame_count_or_silent() {
        let stereo16 = PcmFormat::new(SampleKind::I16, 16, 2, 48_000);
        let data = [0x00, 0x40, 0x00, 0xC0, 0x00, 0x40, 0x00, 0x40];
        assert_eq!(packet_samples(&stereo16, 1, 0, &data), vec![0.5, -0.5]);
        assert_eq!(packet_samples(&stereo16, 2, 0, &data[..6]), vec![0.5, -0.5, 0.5]);
        assert_eq!(packet_samples(&stereo16, 2, AUDCLNT_BUFFERFLAGS_SILENT, &[]), vec![0.0; 4]);
    }

    #[test]
    fn aligned_duration_rounds_to_100ns() {
        assert_eq!(aligned_duration(480, 48_000), 100_000);
        assert_eq!(aligned_duration(441, 44_100), 100_000);
        assert_eq!(aligned_duration(1, 48_000), 208);
    }

    #[test]
    fn failures_are_told_in_the_users_words() {
        assert_eq!(describe_failure(AUDCLNT_E_DEVICE_IN_USE, "x"), "micro déjà réservé par une autre application");
        assert!(describe_failure(AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED, "x").contains("désactivé"));
        assert_eq!(describe_failure(AUDCLNT_E_UNSUPPORTED_FORMAT, "x"), "format audio refusé par le micro");
        assert_eq!(describe_failure(AUDCLNT_E_DEVICE_INVALIDATED, "x"), "micro débranché ou désactivé");
        assert_eq!(describe_failure(0x80070005_u32 as i32, "Accès refusé."), "Accès refusé. (0x80070005)");
    }

    /// A client accepting `accepted` formats; `init` answers each `initialize` call in turn.
    #[derive(Default)]
    struct FakeClient {
        native: Vec<WaveDesc>,
        accepted: Vec<PcmFormat>,
        /// Answered by `supports` instead of `AUDCLNT_E_UNSUPPORTED_FORMAT` for formats not accepted.
        refusal: Option<i32>,
        init: Vec<Result<(), i32>>,
        broken: Vec<&'static str>,
        log: Vec<String>,
    }

    impl FakeClient {
        fn fails(&self, call: &str) -> Result<(), String> {
            if self.broken.contains(&call) { Err(format!("{call} failed")) } else { Ok(()) }
        }
    }

    impl ExclusiveClient for FakeClient {
        fn native_formats(&self) -> Vec<WaveDesc> {
            self.native.clone()
        }
        fn supports(&self, format: &PcmFormat) -> Result<(), (i32, String)> {
            if self.accepted.contains(format) {
                return Ok(());
            }
            Err((self.refusal.unwrap_or(AUDCLNT_E_UNSUPPORTED_FORMAT), "refusé".into()))
        }
        fn period(&self) -> Result<i64, String> {
            self.fails("period").map(|()| 30_000)
        }
        fn initialize(&mut self, format: &PcmFormat, period: i64) -> Result<(), (i32, String)> {
            self.log.push(format!("init {:?} {} {period}", format.kind, format.rate));
            self.init.remove(0).map_err(|code| (code, "boom".into()))
        }
        fn buffer_frames(&self) -> Result<u32, String> {
            self.fails("buffer_frames").map(|()| 1_440)
        }
        fn renew(&mut self) -> Result<(), String> {
            self.fails("renew")?;
            self.log.push("renew".into());
            Ok(())
        }
    }

    fn mono16() -> PcmFormat {
        PcmFormat::new(SampleKind::I16, 16, 1, 48_000)
    }

    #[test]
    fn negotiates_the_first_accepted_format_preferring_the_devices_own() {
        let native = PcmFormat::new(SampleKind::I24, 24, 2, 96_000);
        let mut c = FakeClient {
            native: vec![plain(WAVE_FORMAT_PCM, 8), native.to_wave()],
            accepted: vec![mono16(), native],
            init: vec![Ok(())],
            ..FakeClient::default()
        };
        assert_eq!(negotiate(&mut c), Ok(native));
        assert_eq!(c.log, ["init I24 96000 30000"]);
    }

    #[test]
    fn no_accepted_format_is_refused_before_initialising() {
        let mut c = FakeClient::default();
        assert_eq!(negotiate(&mut c), Err("aucun format audio accepté par le micro en mode exclusif".into()));
        assert!(c.log.is_empty());
    }

    #[test]
    fn an_unaligned_buffer_is_retried_once_on_a_new_client() {
        let mut c = FakeClient {
            accepted: vec![mono16()],
            init: vec![Err(AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED), Ok(())],
            ..FakeClient::default()
        };
        assert_eq!(negotiate(&mut c), Ok(mono16()));
        assert_eq!(c.log, ["init I16 48000 30000", "renew", "init I16 48000 300000"]);
    }

    #[test]
    fn initialisation_failures_are_refusals() {
        let mut c = FakeClient { accepted: vec![mono16()], init: vec![Err(AUDCLNT_E_DEVICE_IN_USE)], ..FakeClient::default() };
        assert_eq!(negotiate(&mut c), Err("micro déjà réservé par une autre application".into()));

        let mut c = FakeClient {
            accepted: vec![mono16()],
            init: vec![Err(AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED), Err(AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED)],
            ..FakeClient::default()
        };
        assert!(negotiate(&mut c).unwrap_err().contains("désactivé"));

        for call in ["period", "buffer_frames", "renew"] {
            let mut c = FakeClient {
                accepted: vec![mono16()],
                init: vec![Err(AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED)],
                broken: vec![call],
                ..FakeClient::default()
            };
            assert_eq!(negotiate(&mut c), Err(format!("{call} failed")));
        }
    }

    fn stereo16() -> PcmFormat {
        PcmFormat::new(SampleKind::I16, 16, 2, 48_000)
    }

    #[test]
    fn a_format_refused_at_initialisation_moves_on_to_the_next_one_on_a_new_client() {
        let mut c = FakeClient {
            accepted: vec![mono16(), stereo16()],
            init: vec![Err(AUDCLNT_E_UNSUPPORTED_FORMAT), Ok(())],
            ..FakeClient::default()
        };
        assert_eq!(negotiate(&mut c), Ok(stereo16()));
        assert_eq!(c.log, ["init I16 48000 30000", "renew", "init I16 48000 30000"]);
    }

    #[test]
    fn every_accepted_format_refused_at_initialisation_is_a_format_refusal() {
        let mut c = FakeClient {
            accepted: vec![mono16(), stereo16()],
            init: vec![Err(AUDCLNT_E_UNSUPPORTED_FORMAT), Err(AUDCLNT_E_UNSUPPORTED_FORMAT)],
            ..FakeClient::default()
        };
        assert_eq!(negotiate(&mut c), Err("format audio refusé par le micro".into()));
    }

    #[test]
    fn a_device_refusal_while_probing_formats_stops_with_its_reason() {
        for (code, reason) in [
            (AUDCLNT_E_DEVICE_IN_USE, "micro déjà réservé par une autre application"),
            (AUDCLNT_E_DEVICE_INVALIDATED, "micro débranché ou désactivé"),
            (AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED, "mode exclusif désactivé pour ce micro dans les réglages Son de Windows"),
        ] {
            let mut c = FakeClient { accepted: vec![stereo16()], refusal: Some(code), ..FakeClient::default() };
            assert_eq!(negotiate(&mut c), Err(reason.into()));
            assert!(c.log.is_empty());
        }
    }

    #[test]
    fn other_probing_errors_only_skip_the_format() {
        let mut c = FakeClient {
            accepted: vec![stereo16()],
            refusal: Some(0x80070057_u32 as i32),
            init: vec![Ok(())],
            ..FakeClient::default()
        };
        assert_eq!(negotiate(&mut c), Ok(stereo16()));
    }

    #[test]
    fn capture_runs_until_its_stream_drops() {
        let log = Arc::new(Mutex::new(vec![]));
        let l = log.clone();
        let open = spawn_capture(Box::new(move |ready, stop| {
            ready.send(Ok((2, 48_000))).unwrap();
            while !stop.load(Ordering::Relaxed) {
                std::thread::yield_now();
            }
            l.lock().unwrap().push("released");
        }))
        .unwrap();
        assert_eq!((open.channels, open.rate), (2, 48_000));
        assert!(log.lock().unwrap().is_empty());
        drop(open.stream);
        assert_eq!(*log.lock().unwrap(), ["released"]);
    }

    #[test]
    fn capture_start_failures_are_returned() {
        let r = spawn_capture(Box::new(|ready, _| ready.send(Err("micro déjà réservé".into())).unwrap()));
        assert_eq!(r.err(), Some("micro déjà réservé".into()));
        let r = spawn_capture(Box::new(|_, _| panic!("WASAPI glue panicked")));
        assert_eq!(r.err(), Some("le thread de capture exclusive s'est arrêté".into()));
    }
}
