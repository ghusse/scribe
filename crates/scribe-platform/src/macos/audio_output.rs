//! Audio outputs through CoreAudio (`AudioObject*` properties): one `OutputBackend` call each, no decision here
//! (which outputs to mute and restore is `output_mute::OutputMuter`). Ids are the device UIDs
//! (`kAudioDevicePropertyDeviceUID`), stable across launches. Outputs are the devices with output streams; the
//! mute switch is the output scope's main element (a device without one is an error, skipped by `OutputMuter`).
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{self, NonNull};

use objc2_core_audio::{
    kAudioDevicePropertyDeviceUID, kAudioDevicePropertyMute, kAudioDevicePropertyStreams,
    kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject, AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize, AudioObjectHasProperty, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectPropertyScope, AudioObjectPropertySelector, AudioObjectSetPropertyData,
};
use objc2_core_foundation::{CFRetained, CFString};

use crate::output_mute::OutputBackend;

pub struct CoreAudioOutputs;

impl OutputBackend for CoreAudioOutputs {
    fn active_outputs(&self) -> Result<Vec<String>, String> {
        let mut ids = Vec::new();
        for device in devices()? {
            if has_output_streams(device) {
                ids.push(uid(device)?);
            }
        }
        Ok(ids)
    }

    fn is_muted(&self, id: &str) -> Result<bool, String> {
        let device = find(id)?;
        let mut muted: u32 = 0;
        get(device, &mut mute_address(device)?, &mut muted)?;
        Ok(muted != 0)
    }

    fn set_muted(&self, id: &str, muted: bool) -> Result<(), String> {
        let device = find(id)?;
        let mut address = mute_address(device)?;
        let value: u32 = muted.into();
        let status = unsafe {
            AudioObjectSetPropertyData(
                device,
                NonNull::from(&mut address),
                0,
                ptr::null(),
                size_of::<u32>() as u32,
                NonNull::from(&value).cast::<c_void>(),
            )
        };
        check(status)
    }
}

fn prop(selector: AudioObjectPropertySelector, scope: AudioObjectPropertyScope) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress { mSelector: selector, mScope: scope, mElement: kAudioObjectPropertyElementMain }
}

fn check(status: i32) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("erreur CoreAudio {status}")) }
}

/// Reads a fixed-size property into `out`.
fn get<T>(object: AudioObjectID, address: &mut AudioObjectPropertyAddress, out: &mut T) -> Result<(), String> {
    let mut size = size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(address),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(out).cast::<c_void>(),
        )
    };
    check(status)
}

fn data_size(object: AudioObjectID, address: &mut AudioObjectPropertyAddress) -> Result<u32, String> {
    let mut size: u32 = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(object, NonNull::from(address), 0, ptr::null(), NonNull::from(&mut size))
    };
    check(status).map(|()| size)
}

/// Every audio device (inputs and outputs).
fn devices() -> Result<Vec<AudioObjectID>, String> {
    let system = kAudioObjectSystemObject as AudioObjectID;
    let mut address = prop(kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal);
    let mut size = data_size(system, &mut address)?;
    let mut devices: Vec<AudioObjectID> = vec![0; size as usize / size_of::<AudioObjectID>()];
    if devices.is_empty() {
        return Ok(devices);
    }
    let status = unsafe {
        AudioObjectGetPropertyData(
            system,
            NonNull::from(&mut address),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::new_unchecked(devices.as_mut_ptr()).cast::<c_void>(),
        )
    };
    check(status)?;
    // A device may have gone between the two calls: keep only what was written.
    devices.truncate(size as usize / size_of::<AudioObjectID>());
    Ok(devices)
}

fn has_output_streams(device: AudioObjectID) -> bool {
    let mut address = prop(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput);
    data_size(device, &mut address).is_ok_and(|size| size > 0)
}

fn uid(device: AudioObjectID) -> Result<String, String> {
    let mut address = prop(kAudioDevicePropertyDeviceUID, kAudioObjectPropertyScopeGlobal);
    let mut raw: *const CFString = ptr::null();
    get(device, &mut address, &mut raw)?;
    let raw = NonNull::new(raw.cast_mut()).ok_or_else(|| format!("périphérique audio {device} sans UID"))?;
    // The UID follows the Create rule: +1 retain count, released when `uid` drops.
    let uid = unsafe { CFRetained::from_raw(raw) };
    Ok(uid.to_string())
}

/// The device with this UID among the current ones; a device unplugged meanwhile is an error.
fn find(id: &str) -> Result<AudioObjectID, String> {
    for device in devices()? {
        if uid(device).is_ok_and(|uid| uid == id) {
            return Ok(device);
        }
    }
    Err(format!("sortie audio {id} introuvable"))
}

fn mute_address(device: AudioObjectID) -> Result<AudioObjectPropertyAddress, String> {
    let address = prop(kAudioDevicePropertyMute, kAudioObjectPropertyScopeOutput);
    if unsafe { AudioObjectHasProperty(device, NonNull::from(&address)) } {
        Ok(address)
    } else {
        Err(format!("la sortie audio {device} n'a pas de réglage muet"))
    }
}
