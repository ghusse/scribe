//! Exclusive microphone capture through WASAPI (`AUDCLNT_SHAREMODE_EXCLUSIVE`, event-driven) on the default
//! capture endpoint. Wiring only: format choice, buffer alignment retry, sample decoding and the capture thread
//! protocol are `wasapi_rules` (tested).
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use windows::core::{w, GUID, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, S_OK};
use windows::Win32::Media::Audio::{
    eCapture, eConsole, IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    PKEY_AudioEngine_DeviceFormat, AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, WaitForSingleObject,
};

use crate::audio_capture::CaptureBuffer;
use crate::exclusive_mic::{InputOpener, OpenStream};
use crate::wasapi_rules::{self, ExclusiveClient, PcmFormat, WaveDesc, WAVE_FORMAT_EXTENSIBLE};

const VT_BLOB: u16 = 65;

pub struct WasapiExclusiveInput;

impl InputOpener for WasapiExclusiveInput {
    fn open(&self, capture: Arc<CaptureBuffer>) -> Result<OpenStream, String> {
        wasapi_rules::spawn_capture(Box::new(move |ready, stop| run(&capture, ready, &stop)))
    }
}

fn run(capture: &CaptureBuffer, ready: mpsc::Sender<Result<(u16, u32), String>>, stop: &AtomicBool) {
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    // The exclusive buffer holds one device period (~10 ms): a starved thread drops samples.
    let mut task = 0;
    let mmcss = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task) }.ok();
    match unsafe { Session::start() } {
        Err(e) => {
            let _ = ready.send(Err(e));
        }
        Ok(session) => {
            let _ = ready.send(Ok((session.format.channels, session.format.rate)));
            if let Err(e) = unsafe { session.capture_until(stop, capture) } {
                capture.set_error(e);
            }
        }
    }
    if let Some(handle) = mmcss {
        let _ = unsafe { AvRevertMmThreadCharacteristics(handle) };
    }
    if com {
        unsafe { CoUninitialize() };
    }
}

fn text(e: windows::core::Error) -> String {
    e.to_string()
}

struct Client {
    device: IMMDevice,
    audio: IAudioClient,
}

impl Client {
    unsafe fn default_capture() -> windows::core::Result<Self> {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)?;
        let device = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole)?;
        let audio = device.Activate::<IAudioClient>(CLSCTX_ALL, None)?;
        Ok(Self { device, audio })
    }
}

impl ExclusiveClient for Client {
    fn native_formats(&self) -> Vec<WaveDesc> {
        unsafe {
            let mut formats: Vec<WaveDesc> = device_format(&self.device).into_iter().collect();
            if let Ok(mix) = self.audio.GetMixFormat() {
                formats.push(read_wave(mix, usize::MAX));
                CoTaskMemFree(Some(mix as *const _));
            }
            formats
        }
    }

    fn supports(&self, format: &PcmFormat) -> Result<(), (i32, String)> {
        let wave = extensible(format);
        let code = unsafe { self.audio.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &wave.Format, None) };
        if code == S_OK { Ok(()) } else { Err((code.0, code.message())) }
    }

    fn period(&self) -> Result<i64, String> {
        let mut period = 0;
        unsafe { self.audio.GetDevicePeriod(Some(&mut period), None) }.map_err(text)?;
        Ok(period)
    }

    fn initialize(&mut self, format: &PcmFormat, period: i64) -> Result<(), (i32, String)> {
        let wave = extensible(format);
        unsafe {
            self.audio.Initialize(
                AUDCLNT_SHAREMODE_EXCLUSIVE,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                period,
                period,
                &wave.Format,
                None,
            )
        }
        .map_err(|e| (e.code().0, e.message().to_string()))
    }

    fn buffer_frames(&self) -> Result<u32, String> {
        unsafe { self.audio.GetBufferSize() }.map_err(text)
    }

    fn renew(&mut self) -> Result<(), String> {
        self.audio = unsafe { self.device.Activate::<IAudioClient>(CLSCTX_ALL, None) }.map_err(text)?;
        Ok(())
    }
}

/// The format chosen in the Sound control panel (`PKEY_AudioEngine_DeviceFormat`), the likeliest to be native.
unsafe fn device_format(device: &IMMDevice) -> Option<WaveDesc> {
    let value = device.OpenPropertyStore(STGM_READ).ok()?.GetValue(&PKEY_AudioEngine_DeviceFormat).ok()?;
    let raw = &value.as_raw().Anonymous.Anonymous;
    if raw.vt != VT_BLOB {
        return None;
    }
    let blob = raw.Anonymous.blob;
    if blob.pBlobData.is_null() || (blob.cbSize as usize) < std::mem::size_of::<WAVEFORMATEX>() {
        return None;
    }
    Some(read_wave(blob.pBlobData as *const WAVEFORMATEX, blob.cbSize as usize))
}

/// `available`: bytes readable at `p`, so the extensible part is read only when present.
unsafe fn read_wave(p: *const WAVEFORMATEX, available: usize) -> WaveDesc {
    let w = std::ptr::read_unaligned(p);
    let mut desc = WaveDesc {
        tag: w.wFormatTag,
        channels: w.nChannels,
        rate: w.nSamplesPerSec,
        avg_bytes_per_sec: w.nAvgBytesPerSec,
        block_align: w.nBlockAlign,
        bits: w.wBitsPerSample,
        ..WaveDesc::default()
    };
    let size = std::mem::size_of::<WAVEFORMATEXTENSIBLE>();
    let extra = size - std::mem::size_of::<WAVEFORMATEX>();
    if w.wFormatTag == WAVE_FORMAT_EXTENSIBLE && w.cbSize as usize >= extra && available >= size {
        let x = std::ptr::read_unaligned(p as *const WAVEFORMATEXTENSIBLE);
        let samples = x.Samples;
        let sub_format = x.SubFormat;
        desc.valid_bits = samples.wValidBitsPerSample;
        desc.channel_mask = x.dwChannelMask;
        desc.sub_format = sub_format.to_u128();
    }
    desc
}

fn extensible(format: &PcmFormat) -> WAVEFORMATEXTENSIBLE {
    let w = format.to_wave();
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: w.tag,
            nChannels: w.channels,
            nSamplesPerSec: w.rate,
            nAvgBytesPerSec: w.avg_bytes_per_sec,
            nBlockAlign: w.block_align,
            wBitsPerSample: w.bits,
            cbSize: (std::mem::size_of::<WAVEFORMATEXTENSIBLE>() - std::mem::size_of::<WAVEFORMATEX>()) as u16,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 { wValidBitsPerSample: w.valid_bits },
        dwChannelMask: w.channel_mask,
        SubFormat: GUID::from_u128(w.sub_format),
    }
}

struct Event(HANDLE);

impl Drop for Event {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// A started exclusive stream; stopped and released on drop.
struct Session {
    capture: IAudioCaptureClient,
    audio: IAudioClient,
    event: Event,
    format: PcmFormat,
}

impl Session {
    unsafe fn start() -> Result<Self, String> {
        let mut client = Client::default_capture().map_err(text)?;
        let format = wasapi_rules::negotiate(&mut client)?;
        let event = Event(CreateEventW(None, false, false, PCWSTR::null()).map_err(text)?);
        client.audio.SetEventHandle(event.0).map_err(text)?;
        let capture: IAudioCaptureClient = client.audio.GetService().map_err(text)?;
        client.audio.Start().map_err(text)?;
        Ok(Self { capture, audio: client.audio, event, format })
    }

    unsafe fn capture_until(&self, stop: &AtomicBool, capture: &CaptureBuffer) -> Result<(), String> {
        while !stop.load(Ordering::Relaxed) {
            // Bounded wait: a stop is seen even when the device no longer signals.
            WaitForSingleObject(self.event.0, 100);
            while self.capture.GetNextPacketSize().map_err(text)? > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                self.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None).map_err(text)?;
                let len = frames as usize * self.format.block_align() as usize;
                let bytes = if data.is_null() { &[][..] } else { std::slice::from_raw_parts(data, len) };
                capture.push(wasapi_rules::packet_samples(&self.format, frames, flags, bytes));
                self.capture.ReleaseBuffer(frames).map_err(text)?;
            }
        }
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = unsafe { self.audio.Stop() };
    }
}
