//! Barra widgets: small live readings (system load, weather) drawn natively in
//! the menu bar and, later, the dock.
//!
//! Each data source runs on its own thread only while a widget that needs it
//! is placed somewhere. A source stores its reading here and posts one message
//! to the menu bar when the reading changes, so the bar repaints just that
//! widget. Nothing is polled on the UI thread.

mod clima;
mod draw;
mod sistema;

pub use draw::{render_bar, render_tile};

use muelle_config::Widgets as Settings;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::PostMessageW,
};

/// A widget kind. The id in the settings is its lowercase Spanish name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WidgetId {
    /// CPU temperature gauge with CPU and memory bars.
    Sistema,
    /// Current weather for the configured city.
    Clima,
}

impl WidgetId {
    pub const ALL: [Self; 2] = [Self::Sistema, Self::Clima];

    pub fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_lowercase().as_str() {
            "sistema" => Some(Self::Sistema),
            "clima" => Some(Self::Clima),
            _ => None,
        }
    }
}

/// Latest system reading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SistemaData {
    /// CPU package temperature, °C, when a sensor source answers.
    pub temp: Option<u8>,
    pub cpu: u8,
    pub ram: u8,
}

/// Latest weather reading.
#[derive(Clone, Debug, PartialEq)]
pub struct ClimaData {
    pub city: String,
    pub temp: i16,
    /// WMO weather code.
    pub code: u8,
    pub day: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Readings {
    pub sistema: Option<SistemaData>,
    pub clima: Option<ClimaData>,
}

/// Which widgets the menu bar shows, left and right.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BarLayout {
    pub left: Vec<WidgetId>,
    pub right: Vec<WidgetId>,
}

static READINGS: Mutex<Readings> = Mutex::new(Readings {
    sistema: None,
    clima: None,
});
static SETTINGS: Mutex<Option<Settings>> = Mutex::new(None);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static TARGET_MESSAGE: AtomicU32 = AtomicU32::new(0);
static SISTEMA_ON: AtomicBool = AtomicBool::new(false);
static CLIMA_ON: AtomicBool = AtomicBool::new(false);

fn ids(keys: &[String]) -> Vec<WidgetId> {
    let mut out = Vec::new();
    for id in keys.iter().filter_map(|key| WidgetId::from_key(key)) {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// Applies the settings: which widgets are placed, and starts or stops the
/// sources they need.
pub fn configure(settings: &Settings) {
    let placed: Vec<WidgetId> = ids(&settings.bar_left)
        .into_iter()
        .chain(ids(&settings.bar_right))
        .chain(ids(&settings.dock))
        .collect();
    let previous = SETTINGS
        .lock()
        .map(|mut slot| slot.replace(settings.clone()))
        .ok()
        .flatten();
    let city_changed = previous.is_some_and(|old| old.weather_city != settings.weather_city);
    if city_changed {
        if let Ok(mut readings) = READINGS.lock() {
            readings.clima = None;
        }
        clima::wake();
    }
    for id in WidgetId::ALL {
        let on = placed.contains(&id);
        let (flag, start): (&AtomicBool, fn()) = match id {
            WidgetId::Sistema => (&SISTEMA_ON, sistema::start),
            WidgetId::Clima => (&CLIMA_ON, clima::start),
        };
        if on && !flag.swap(true, Ordering::AcqRel) {
            start();
        } else if !on {
            flag.store(false, Ordering::Release);
        }
    }
}

/// The menu bar placement from the current settings.
pub fn bar_layout() -> BarLayout {
    SETTINGS
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .map(|settings| BarLayout {
            left: ids(&settings.bar_left),
            right: ids(&settings.bar_right),
        })
        .unwrap_or_default()
}

fn city() -> String {
    SETTINGS
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|s| s.weather_city.clone()))
        .unwrap_or_default()
}

pub fn readings() -> Readings {
    READINGS.lock().map(|r| r.clone()).unwrap_or_default()
}

/// The window told when a reading changes, and the message it gets.
pub fn set_target(hwnd: HWND, message: u32) {
    TARGET_MESSAGE.store(message, Ordering::Release);
    TARGET.store(hwnd.0 as isize, Ordering::Release);
}

/// Stores a reading and tells the menu bar if it changed.
fn publish(update: impl FnOnce(&mut Readings) -> bool) {
    let changed = READINGS.lock().map(|mut r| update(&mut r)).unwrap_or(false);
    let hwnd = TARGET.load(Ordering::Acquire);
    if changed && hwnd != 0 {
        // SAFETY: Posting a scalar message; a destroyed window just fails.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd as *mut _)),
                TARGET_MESSAGE.load(Ordering::Acquire),
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

/// Dock entry ids start with this, then the widget key.
const ENTRY_PREFIX: &str = "widget:";

impl WidgetId {
    pub fn key(self) -> &'static str {
        match self {
            Self::Sistema => "sistema",
            Self::Clima => "clima",
        }
    }

    /// The name shown above its dock tile.
    pub fn name(self) -> &'static str {
        match self {
            Self::Sistema => "Sistema",
            Self::Clima => "Clima",
        }
    }

    /// What a click on the widget opens.
    pub fn target(self) -> &'static str {
        match self {
            Self::Sistema => "taskmgr.exe",
            Self::Clima => "msnweather:",
        }
    }

    pub fn entry_id(self) -> String {
        format!("{ENTRY_PREFIX}{}", self.key())
    }

    pub fn from_entry(id: &str) -> Option<Self> {
        Self::from_key(id.strip_prefix(ENTRY_PREFIX)?)
    }

    /// Changes when what the widget shows changes.
    pub fn fingerprint(self, readings: &Readings) -> String {
        match self {
            Self::Sistema => format!("{:?}", readings.sistema),
            Self::Clima => format!("{:?}", readings.clima),
        }
    }
}

/// The widgets placed in the dock, in order.
pub fn dock() -> Vec<WidgetId> {
    SETTINGS
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|s| ids(&s.dock)))
        .unwrap_or_default()
}

/// What a click on a widget in the menu bar opens.
pub fn open(id: WidgetId) {
    if let Err(error) = muelle_shell::launcher::open(id.target()) {
        eprintln!("muelle: widget {id:?}: {error}");
    }
}
