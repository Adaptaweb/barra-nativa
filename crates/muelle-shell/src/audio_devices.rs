//! Audio devices for the "Micrófono y salida" widget: listing the active
//! outputs and inputs, making one the default, and muting the default
//! microphone.
//!
//! Every function is self-contained: it makes sure COM is initialized on the
//! calling thread, so it works from a background thread or the UI thread.

// The COM interface methods keep their Windows names.
#![allow(non_snake_case)]

use std::ffi::c_void;
use windows::{
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Foundation::RPC_E_CHANGED_MODE,
        Media::Audio::{
            DEVICE_STATE_ACTIVE, EDataFlow, ERole,
            Endpoints::IAudioEndpointVolume,
            IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eCommunications,
            eConsole, eMultimedia, eRender,
        },
        System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
            CoUninitialize, STGM_READ,
            StructuredStorage::{PropVariantClear, PropVariantToStringAlloc},
        },
    },
    core::{GUID, HRESULT, HSTRING, PCWSTR, Result},
};

/// An active audio endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// The default device for the console role.
    pub default: bool,
}

/// `CPolicyConfigClient`.
const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

/// The undocumented `IPolicyConfig` that Windows' own Sound settings use to
/// change the default device. EarTrumpet and SoundSwitch use it too, and its
/// layout has been stable since Windows 7. Only `SetDefaultEndpoint` is
/// called; the other methods are declared with opaque parameters so the
/// vtable has the right size and order.
#[windows_core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: windows_core::IUnknown {
    fn GetMixFormat(&self, device: PCWSTR, format: *mut c_void) -> HRESULT;
    fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, device: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
    fn GetProcessingPeriod(
        &self,
        device: PCWSTR,
        default: i32,
        default_period: *mut c_void,
        minimum_period: *mut c_void,
    ) -> HRESULT;
    fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut c_void) -> HRESULT;
    fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn GetPropertyValue(&self, device: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
    fn SetPropertyValue(&self, device: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
    fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
    fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
}

/// Runs `action` with COM initialized on this thread (or already, on a UI
/// thread), and releases it afterwards. Everything COM that `action` creates
/// must be dropped before it returns.
fn with_com<T>(action: impl FnOnce() -> Result<T>) -> Result<T> {
    // SAFETY: Balanced with `CoUninitialize` only when this call initialized
    // it; a thread already in another apartment keeps its own.
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
        return Err(windows::core::Error::from_hresult(initialized));
    }
    let result = action();
    if initialized.is_ok() {
        // SAFETY: Pairs with the successful `CoInitializeEx` above.
        unsafe { CoUninitialize() };
    }
    result
}

fn enumerator() -> Result<IMMDeviceEnumerator> {
    // SAFETY: COM is initialized by `with_com`; the object is released on drop.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

fn flow(output: bool) -> EDataFlow {
    if output { eRender } else { eCapture }
}

fn device_id(device: &IMMDevice) -> Result<String> {
    // SAFETY: `GetId` returns a CoTaskMem string that is freed after copying.
    unsafe {
        let raw = device.GetId()?;
        let id = raw.to_string().unwrap_or_default();
        CoTaskMemFree(Some(raw.0.cast()));
        Ok(id)
    }
}

fn friendly_name(device: &IMMDevice) -> Result<String> {
    // SAFETY: Read-only property store; the variant is cleared and the
    // allocated string freed before returning.
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ)?;
        let mut value = store.GetValue(&PKEY_Device_FriendlyName)?;
        let text = PropVariantToStringAlloc(&value);
        let _ = PropVariantClear(&mut value);
        let text = text?;
        let name = text.to_string().unwrap_or_default();
        CoTaskMemFree(Some(text.0.cast()));
        Ok(name)
    }
}

/// The default device of that flow for the console role.
fn default_device(enumerator: &IMMDeviceEnumerator, output: bool) -> Result<IMMDevice> {
    // SAFETY: Plain query on a live enumerator.
    unsafe { enumerator.GetDefaultAudioEndpoint(flow(output), eConsole) }
}

/// The active outputs (`output`) or inputs, with the default marked.
/// Empty when Windows cannot list them.
#[must_use]
pub fn list(output: bool) -> Vec<Device> {
    with_com(|| {
        let enumerator = enumerator()?;
        let default = default_device(&enumerator, output)
            .and_then(|device| device_id(&device))
            .unwrap_or_default();
        // SAFETY: Plain queries on live COM objects of this thread.
        let devices = unsafe { enumerator.EnumAudioEndpoints(flow(output), DEVICE_STATE_ACTIVE)? };
        // SAFETY: As above.
        let count = unsafe { devices.GetCount()? };
        let mut out = Vec::new();
        for index in 0..count {
            // SAFETY: `index` is below the collection's count.
            let Ok(device) = (unsafe { devices.Item(index) }) else {
                continue;
            };
            let Ok(id) = device_id(&device) else {
                continue;
            };
            let name = friendly_name(&device).unwrap_or_default();
            out.push(Device {
                default: id == default,
                id,
                name,
            });
        }
        Ok(out)
    })
    .unwrap_or_default()
}

/// The name of the default device, if there is one.
#[must_use]
pub fn default_name(output: bool) -> Option<String> {
    with_com(|| {
        let enumerator = enumerator()?;
        friendly_name(&default_device(&enumerator, output)?)
    })
    .ok()
}

/// Makes the device the default for every role (console, multimedia and
/// communications), as Windows' Sound settings do.
pub fn set_default(id: &str) -> Result<()> {
    with_com(|| {
        // SAFETY: COM is initialized by `with_com`; the object is released on drop.
        let policy: IPolicyConfig =
            unsafe { CoCreateInstance(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)? };
        let id = HSTRING::from(id);
        for role in [eConsole, eMultimedia, eCommunications] {
            // SAFETY: `id` lives for the call and is a NUL-terminated wide
            // string; the vtable matches the Windows 7+ `IPolicyConfig`.
            unsafe { policy.SetDefaultEndpoint(PCWSTR(id.as_ptr()), role).ok()? };
        }
        Ok(())
    })
}

fn mic_volume(enumerator: &IMMDeviceEnumerator) -> Result<IAudioEndpointVolume> {
    let device = default_device(enumerator, false)?;
    // SAFETY: Activating the endpoint volume interface on a live device.
    unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }
}

/// Whether the default microphone is muted, or `None` without one.
#[must_use]
pub fn mic_muted() -> Option<bool> {
    with_com(|| {
        let volume = mic_volume(&enumerator()?)?;
        // SAFETY: Plain query on a live COM object.
        Ok(unsafe { volume.GetMute()? }.as_bool())
    })
    .ok()
}

/// Mutes or unmutes the default microphone; returns whether it is now muted.
pub fn toggle_mic() -> Result<bool> {
    with_com(|| {
        let volume = mic_volume(&enumerator()?)?;
        // SAFETY: Reads and updates a live endpoint of this thread; the
        // event context is optional.
        unsafe {
            let muted = !volume.GetMute()?.as_bool();
            volume.SetMute(muted, std::ptr::null())?;
            Ok(muted)
        }
    })
}
