//! Hog mode on the inputs through CoreAudio: one `HogBackend` call each, no decision here (which inputs to take, in
//! which order, and when to give them back is `exclusive_mic::HogGuard`). macOS gives them back if the process dies.
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{self, NonNull};

use objc2_core_audio::{
    kAudioDevicePropertyHogMode, kAudioDevicePropertyStreams, kAudioDevicePropertyTransportType,
    kAudioHardwarePropertyDefaultInputDevice, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeInput,
    kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject, AudioObjectID, AudioObjectSetPropertyData,
};

use super::audio_output::{check, data_size, devices, get, prop};
use crate::exclusive_mic::HogBackend;

#[derive(Clone, Copy)]
pub struct CoreAudioHog;

impl HogBackend for CoreAudioHog {
    fn default_input(&self) -> Result<u32, String> {
        let mut device: AudioObjectID = 0;
        let mut address = prop(kAudioHardwarePropertyDefaultInputDevice, kAudioObjectPropertyScopeGlobal);
        get(kAudioObjectSystemObject as AudioObjectID, &mut address, &mut device)?;
        if device == 0 {
            Err("aucun micro détecté".into())
        } else {
            Ok(device)
        }
    }

    fn set_default_input(&self, device: u32) -> Result<(), String> {
        let mut address = prop(kAudioHardwarePropertyDefaultInputDevice, kAudioObjectPropertyScopeGlobal);
        let status = unsafe {
            AudioObjectSetPropertyData(
                kAudioObjectSystemObject as AudioObjectID,
                NonNull::from(&mut address),
                0,
                ptr::null(),
                size_of::<AudioObjectID>() as u32,
                NonNull::from(&device).cast::<c_void>(),
            )
        };
        check(status)
    }

    fn inputs(&self) -> Result<Vec<u32>, String> {
        let mut inputs = Vec::new();
        for device in devices()? {
            if data_size(device, &mut prop(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeInput))? > 0 {
                inputs.push(device);
            }
        }
        Ok(inputs)
    }

    fn transport(&self, device: u32) -> Result<u32, String> {
        let mut transport: u32 = 0;
        get(device, &mut prop(kAudioDevicePropertyTransportType, kAudioObjectPropertyScopeGlobal), &mut transport)?;
        Ok(transport)
    }

    fn has_output(&self, device: u32) -> Result<bool, String> {
        data_size(device, &mut prop(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput)).map(|size| size > 0)
    }

    fn owner(&self, device: u32) -> Result<i32, String> {
        let mut pid: libc::pid_t = -1;
        get(device, &mut prop(kAudioDevicePropertyHogMode, kAudioObjectPropertyScopeGlobal), &mut pid)?;
        Ok(pid)
    }

    fn toggle(&self, device: u32) -> Result<(), String> {
        let mut address = prop(kAudioDevicePropertyHogMode, kAudioObjectPropertyScopeGlobal);
        let pid: libc::pid_t = unsafe { libc::getpid() };
        let status = unsafe {
            AudioObjectSetPropertyData(
                device,
                NonNull::from(&mut address),
                0,
                ptr::null(),
                size_of::<libc::pid_t>() as u32,
                NonNull::from(&pid).cast::<c_void>(),
            )
        };
        check(status)
    }
}
