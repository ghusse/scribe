//! Audio outputs through WASAPI (Core Audio): one `OutputBackend` call each, no decision here (which outputs to
//! mute and restore is `output_mute::OutputMuter`). Ids are the endpoint ids (`IMMDevice::GetId`), stable across
//! launches. Every call initialises COM itself: it runs on whatever thread the caller uses.
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::SystemInformation::GetTickCount64;

use crate::output_mute::OutputBackend;

pub struct WasapiOutputs;

impl OutputBackend for WasapiOutputs {
    fn active_outputs(&self) -> Result<Vec<String>, String> {
        unsafe { active_outputs() }.map_err(|e| e.to_string())
    }

    fn is_muted(&self, id: &str) -> Result<bool, String> {
        unsafe { endpoint_volume(id).and_then(|v| v.GetMute()) }.map(|b| b.as_bool()).map_err(|e| e.to_string())
    }

    fn set_muted(&self, id: &str, muted: bool) -> Result<(), String> {
        unsafe { endpoint_volume(id).and_then(|v| v.SetMute(muted, std::ptr::null())) }.map_err(|e| e.to_string())
    }
}

unsafe fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    // Ignore the result: S_FALSE / RPC_E_CHANGED_MODE just mean COM is already initialised on this thread.
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)
}

unsafe fn active_outputs() -> windows::core::Result<Vec<String>> {
    let devices = enumerator()?.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
    let mut ids = Vec::new();
    for i in 0..devices.GetCount()? {
        ids.push(device_id(&devices.Item(i)?)?);
    }
    Ok(ids)
}

unsafe fn device_id(device: &IMMDevice) -> windows::core::Result<String> {
    let raw: PWSTR = device.GetId()?;
    let id = raw.to_string();
    // Allocated by the endpoint with CoTaskMemAlloc.
    CoTaskMemFree(Some(raw.0 as *const _));
    id.map_err(|e| windows::core::Error::new(E_FAIL, e.to_string()))
}

/// A vanished endpoint makes `GetDevice` fail (`E_NOTFOUND`): reported as an error, which `OutputMuter` skips.
unsafe fn endpoint_volume(id: &str) -> windows::core::Result<IAudioEndpointVolume> {
    let device = enumerator()?.GetDevice(&HSTRING::from(id))?;
    device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
}

/// When the system booted (Unix ms): the wall clock minus the uptime (sleep included). Dates the boot session the
/// outputs were muted in, for the crash recovery.
pub fn boot_time_ms() -> Option<u64> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
    now.checked_sub(unsafe { GetTickCount64() })
}
