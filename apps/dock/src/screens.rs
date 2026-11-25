//! One dock per display. The first process docks on the primary display and
//! starts a process for each other display that has its dock turned on
//! (`muelle --screen \\.\DISPLAY2 <parent pid>`). Each shows only the apps
//! whose windows are on its display, and its own widgets. A display's dock
//! closes when its display is unplugged, when it is turned off in Settings,
//! or when the primary dock exits.

use anyhow::Result;
use muelle_config::Widgets;
use muelle_shell::displays;
use std::{
    os::windows::process::CommandExt,
    process::{Child, Command},
    sync::Mutex,
};
use windows::{
    Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
        System::Threading::{
            CreateMutexW, INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        },
    },
    core::{HSTRING, Owned},
};

pub const FLAG: &str = "--screen";

/// The docks this process started, by display.
static CHILDREN: Mutex<Vec<(String, Child)>> = Mutex::new(Vec::new());
/// This dock's display, refreshed when displays change.
static HOME: Mutex<Option<String>> = Mutex::new(None);

/// Runs a dock on `device` for as long as process `parent` lives.
pub fn run_child(device: &str, parent: u32, initialize: fn() -> Result<()>) -> Result<()> {
    displays::set_home(device);
    let name = HSTRING::from(format!(
        "Local\\Muelle.Dock.{}",
        device.trim_start_matches(['\\', '.'])
    ));
    // SAFETY: The name lives for the call; the handle is released at exit.
    let _single = unsafe { Owned::new(CreateMutexW(None, false, &name)?) };
    // SAFETY: Reads the thread's last error right after the call.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Ok(());
    }
    muelle_shell::log::line(&format!("dock de la pantalla {device}"));
    // When the primary dock exits (or crashes), this one follows.
    std::thread::spawn(move || {
        // SAFETY: Waits on a process handle that is closed when the thread ends.
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, parent) {
                let handle = Owned::new(handle);
                WaitForSingleObject(*handle, INFINITE);
            }
        }
        crate::app::quit_this();
    });
    let result = initialize();
    muelle_shell::log::flush();
    result
}

fn enabled(widgets: &Widgets, device: &str) -> bool {
    widgets.screens.get(device).is_none_or(|s| s.enabled)
}

/// Starts a dock for each other display that should have one and has none.
/// Only the primary display's dock does this.
pub fn sync(widgets: &Widgets) {
    if displays::is_secondary_process() || std::env::var_os("BARRA_MONITOR").is_some() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Ok(mut children) = CHILDREN.lock() else {
        return;
    };
    children.retain_mut(|(_, child)| child.try_wait().ok().flatten().is_none());
    for display in displays::list().into_iter().filter(|d| !d.primary) {
        if !enabled(widgets, &display.device)
            || children.iter().any(|(device, _)| *device == display.device)
        {
            continue;
        }
        let spawned = Command::new(&exe)
            .args([FLAG, &display.device, &std::process::id().to_string()])
            .creation_flags(0x0800_0000)
            .spawn();
        match spawned {
            Ok(child) => children.push((display.device, child)),
            Err(error) => eprintln!("muelle: dock de {}: {error}", display.device),
        }
    }
}

/// Whether this display's dock was turned off in Settings.
pub fn turned_off(widgets: &Widgets) -> bool {
    displays::home_device().is_some_and(|device| !enabled(widgets, device))
}

/// Displays were added, removed or rearranged.
pub fn display_changed() {
    if let Ok(mut home) = HOME.lock() {
        *home = None;
    }
    if displays::is_secondary_process() {
        if displays::home_monitor().is_none() {
            crate::app::quit_this();
        }
    } else if let Some(config) = muelle_config::load(&muelle_config::default_path())
        .ok()
        .flatten()
    {
        sync(&config.widgets);
    }
}

/// Whether a window is on this dock's display.
pub fn on_this_display(hwnd: isize) -> bool {
    let home = {
        let Ok(mut home) = HOME.lock() else {
            return true;
        };
        if home.is_none() {
            *home = displays::home_device_name();
        }
        home.clone()
    };
    match (home, displays::window_device(hwnd)) {
        (Some(home), Some(device)) => home == device,
        _ => true,
    }
}
