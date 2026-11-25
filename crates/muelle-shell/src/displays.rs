//! The connected displays, named as people know them ("Pantalla 2 ·
//! SAMSUNG"), and which one this dock process belongs to.
//!
//! Barra runs one dock process per display: the first one owns the primary
//! display (and the taskbar, the tray and updates); each other display gets
//! its own process started with `--screen <device>`. Every process shows the
//! apps whose windows are on its display, and its own widgets.

use std::sync::OnceLock;

use windows::Win32::{
    Devices::Display::{
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
        DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
        DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes,
        QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
    },
    Foundation::{HWND, LPARAM, RECT},
    Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST,
        MONITORINFO, MONITORINFOEXW, MonitorFromRect,
    },
    UI::WindowsAndMessaging::{GetWindowPlacement, GetWindowRect, IsIconic, WINDOWPLACEMENT},
};
use windows::core::BOOL;

use crate::system::{Monitor, monitor_from_handle};

/// A connected display.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    /// Windows' device name, `\\.\DISPLAY2`: the key in the settings.
    pub device: String,
    /// The number Windows shows for it in Settings › Display.
    pub number: u32,
    /// The model the display reports (EDID), or its resolution.
    pub model: String,
    pub primary: bool,
    pub monitor: Monitor,
}

impl Display {
    /// "Pantalla 2 · SAMSUNG", with " (principal)" for the primary one.
    pub fn label(&self) -> String {
        let main = if self.primary { " (principal)" } else { "" };
        format!("Pantalla {} · {}{main}", self.number, self.model)
    }
}

fn text(buffer: &[u16]) -> String {
    let n = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..n])
}

/// Model names by device name, from the display configuration.
fn models() -> Vec<(String, String)> {
    let mut found = Vec::new();
    // SAFETY: Buffers are sized as Windows asks; structures carry their sizes.
    unsafe {
        let (mut paths, mut modes) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes).is_err() {
            return found;
        }
        let mut path_info = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
        let mut mode_info = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut paths,
            path_info.as_mut_ptr(),
            &mut modes,
            mode_info.as_mut_ptr(),
            None,
        )
        .is_err()
        {
            return found;
        }
        for path in &path_info[..paths as usize] {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = path.sourceInfo.adapterId;
            source.header.id = path.sourceInfo.id;
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target.header.adapterId = path.targetInfo.adapterId;
            target.header.id = path.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut source.header) == 0
                && DisplayConfigGetDeviceInfo(&mut target.header) == 0
            {
                let mut model = text(&target.monitorFriendlyDeviceName);
                // Many displays report only a model code ("LC24RG50"): put
                // the maker in front, from the EDID manufacturer id.
                if let Some(maker) = maker(target.edidManufactureId)
                    && !model.to_lowercase().contains(&maker.to_lowercase())
                    && !model.is_empty()
                {
                    model = format!("{maker} {model}");
                }
                if !model.is_empty() {
                    found.push((text(&source.viewGdiDeviceName), model));
                }
            }
        }
    }
    found
}

/// The maker from an EDID manufacturer id (three letters, five bits each,
/// big-endian).
fn maker(id: u16) -> Option<&'static str> {
    let id = id.swap_bytes();
    let letter = |shift: u16| char::from(b'@' + ((id >> shift) & 0x1F) as u8);
    let code: String = [letter(10), letter(5), letter(0)].iter().collect();
    Some(match code.as_str() {
        "SAM" | "SEC" => "Samsung",
        "GSM" | "LGD" => "LG",
        "DEL" => "Dell",
        "ACR" => "Acer",
        "AUS" => "ASUS",
        "BNQ" => "BenQ",
        "HWP" | "HPN" => "HP",
        "LEN" => "Lenovo",
        "AOC" => "AOC",
        "PHL" => "Philips",
        "MSI" => "MSI",
        "XMI" => "Xiaomi",
        "VSC" => "ViewSonic",
        "GBT" => "Gigabyte",
        "SNY" => "Sony",
        "APP" => "Apple",
        _ => return None,
    })
}

fn device_of(handle: HMONITOR) -> Option<(String, bool)> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: The extended structure is passed with its own size.
    unsafe {
        if !GetMonitorInfoW(handle, (&raw mut info).cast::<MONITORINFO>()).as_bool() {
            return None;
        }
    }
    Some((
        text(&info.szDevice),
        info.monitorInfo.dwFlags & 1 != 0, // MONITORINFOF_PRIMARY
    ))
}

/// The connected displays, by Windows' number.
#[must_use]
pub fn list() -> Vec<Display> {
    unsafe extern "system" fn append(handle: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        // SAFETY: The vector lives through the synchronous enumeration.
        unsafe { (&mut *(data.0 as *mut Vec<HMONITOR>)).push(handle) };
        BOOL(1)
    }
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: The callback only appends to the vector it is given.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(append), LPARAM((&raw mut handles) as isize));
    }
    let models = models();
    let mut displays: Vec<Display> = handles
        .into_iter()
        .filter_map(|handle| {
            let (device, primary) = device_of(handle)?;
            let monitor = monitor_from_handle(handle)?;
            let number = device
                .rsplit("DISPLAY")
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            let model = models
                .iter()
                .find(|(d, _)| *d == device)
                .map(|(_, m)| m.clone())
                .unwrap_or_else(|| {
                    let s = monitor.screen;
                    format!("{} × {}", s.right - s.left, s.bottom - s.top)
                });
            Some(Display {
                device,
                number,
                model,
                primary,
                monitor,
            })
        })
        .collect();
    displays.sort_by_key(|d| d.number);
    displays
}

static HOME: OnceLock<String> = OnceLock::new();

/// This process docks on `device` instead of the primary display.
pub fn set_home(device: &str) {
    let _ = HOME.set(device.to_owned());
}

/// The display this process docks on, when it is not the primary one.
#[must_use]
pub fn home_device() -> Option<&'static str> {
    HOME.get().map(String::as_str)
}

/// Whether this is a dock for a display other than the primary one.
#[must_use]
pub fn is_secondary_process() -> bool {
    HOME.get().is_some()
}

/// The display this process docks on: its own, or the primary one.
#[must_use]
pub fn home_monitor() -> Option<Monitor> {
    match home_device() {
        Some(device) => list()
            .into_iter()
            .find(|d| d.device == device)
            .map(|d| d.monitor),
        None => crate::system::primary_monitor(),
    }
}

/// The device name of the display this process docks on.
#[must_use]
pub fn home_device_name() -> Option<String> {
    match home_device() {
        Some(device) => Some(device.to_owned()),
        None => list().into_iter().find(|d| d.primary).map(|d| d.device),
    }
}

/// The display a window is on; for a minimized window, where it will come back.
#[must_use]
pub fn window_device(hwnd: isize) -> Option<String> {
    let hwnd = HWND(hwnd as *mut _);
    let mut rect = RECT::default();
    // SAFETY: Read-only window queries into locals.
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let mut placement = WINDOWPLACEMENT {
                length: size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            GetWindowPlacement(hwnd, &mut placement).ok()?;
            rect = placement.rcNormalPosition;
        } else {
            GetWindowRect(hwnd, &mut rect).ok()?;
        }
        device_of(MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST)).map(|(device, _)| device)
    }
}
