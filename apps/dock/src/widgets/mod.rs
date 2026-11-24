//! Barra widgets: small live readings drawn natively in the menu bar and the
//! dock: system load, weather, the clock with the calendar, Home Assistant
//! and AI plan limits.
//!
//! Each data source runs on its own thread only while a widget that needs it
//! is placed somewhere. A source stores its reading here and posts one message
//! to the menu bar when the reading changes, so the bar and the dock repaint
//! just that widget. Nothing is polled on the UI thread.

mod casa;
mod clima;
mod draw;
mod ia;
mod more;
mod panel;
mod panel_content;
mod reloj;
mod secrets;
mod sistema;

pub use draw::{render_bar, render_tile};
pub use panel::{Placement, refresh as refresh_panel, toggle as toggle_panel};

use crate::usage::{self, Provider};
use muelle_config::{HomeItem, Widgets as Settings};
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
    /// Date and time with the next calendar event. Replaces the bar's clock.
    Reloj,
    /// Home Assistant favourite buttons.
    Casa,
    /// Claude plan limits.
    Claude,
    /// ChatGPT plan limits, as Codex reports them.
    ChatGpt,
    /// Gemini quota.
    Gemini,
}

impl WidgetId {
    pub const ALL: [Self; 7] = [
        Self::Sistema,
        Self::Clima,
        Self::Reloj,
        Self::Casa,
        Self::Claude,
        Self::ChatGpt,
        Self::Gemini,
    ];

    pub fn from_key(key: &str) -> Option<Self> {
        let key = key.trim().to_lowercase();
        Self::ALL.into_iter().find(|id| id.key() == key)
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Sistema => "sistema",
            Self::Clima => "clima",
            Self::Reloj => "reloj",
            Self::Casa => "casa",
            Self::Claude => "claude",
            Self::ChatGpt => "chatgpt",
            Self::Gemini => "gemini",
        }
    }

    /// The name shown above its dock tile.
    pub fn name(self) -> &'static str {
        match self {
            Self::Sistema => "Sistema",
            Self::Clima => "Clima",
            Self::Reloj => "Calendario",
            Self::Casa => "Casa",
            Self::Claude => "Claude",
            Self::ChatGpt => "ChatGPT",
            Self::Gemini => "Gemini",
        }
    }

    /// The AI provider whose limits it shows.
    pub fn provider(self) -> Option<Provider> {
        match self {
            Self::Claude => Some(Provider::Claude),
            Self::ChatGpt => Some(Provider::Codex),
            Self::Gemini => Some(Provider::Gemini),
            _ => None,
        }
    }

    /// What a click on the widget opens. Until widgets have their own panels,
    /// a widget with nothing set up opens where to set it up.
    pub fn target(self) -> String {
        let readings = readings();
        match self {
            Self::Sistema => "taskmgr.exe".to_owned(),
            Self::Clima => "msnweather:".to_owned(),
            Self::Reloj => match readings.reloj.map(|r| r.calendar) {
                Some(Calendar::NotConnected) => {
                    "https://calendar.google.com/calendar/r/settings".to_owned()
                }
                _ => "ms-actioncenter:".to_owned(),
            },
            Self::Casa => {
                let (url, _) = home_settings();
                if url.is_empty() {
                    "https://www.home-assistant.io/docs/authentication/#your-account-profile"
                        .to_owned()
                } else {
                    url
                }
            }
            Self::Claude => "https://claude.ai/settings/usage".to_owned(),
            Self::ChatGpt => "https://chatgpt.com/codex/settings/usage".to_owned(),
            Self::Gemini => "https://aistudio.google.com/".to_owned(),
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
            Self::Reloj => format!("{:?}", readings.reloj),
            Self::Casa => format!("{:?}", readings.casa),
            Self::Claude | Self::ChatGpt | Self::Gemini => {
                let provider = self.provider().unwrap_or(Provider::Claude);
                readings.ia.as_ref().map_or_else(String::new, |snapshot| {
                    serde_json::to_string(snapshot.get(provider)).unwrap_or_default()
                })
            }
        }
    }

    /// Whether it has something to draw yet.
    pub fn has_reading(self, readings: &Readings) -> bool {
        match self {
            Self::Sistema => readings.sistema.is_some(),
            Self::Clima => readings.clima.is_some(),
            Self::Reloj => readings.reloj.is_some(),
            Self::Casa => readings.casa.is_some(),
            Self::Claude | Self::ChatGpt | Self::Gemini => readings.ia.is_some(),
        }
    }

    fn source(self) -> (&'static AtomicBool, fn()) {
        match self {
            Self::Sistema => (&SISTEMA_ON, sistema::start),
            Self::Clima => (&CLIMA_ON, clima::start),
            Self::Reloj => (&RELOJ_ON, reloj::start),
            Self::Casa => (&CASA_ON, casa::start),
            Self::Claude | Self::ChatGpt | Self::Gemini => (&IA_ON, ia::start),
        }
    }
}

/// Dock entry ids start with this, then the widget key.
const ENTRY_PREFIX: &str = "widget:";

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calendar {
    Loading,
    /// No calendar link saved.
    NotConnected,
    Connected,
    /// Links saved but none could be read.
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub title: String,
    /// Unix seconds.
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelojData {
    /// Date and time as the menu bar shows them ("mar 24 nov 19:39").
    pub clock: String,
    /// Day of the month and weekday, for the dock tile.
    pub day: String,
    pub weekday: String,
    pub calendar: Calendar,
    pub next: Option<Event>,
    /// The next events, soonest first, for the panel.
    pub upcoming: Vec<Event>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Home {
    NotConnected,
    Connected,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeButton {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub on: bool,
    pub value: Option<String>,
    /// Lights, plugs, covers... can be switched; sensors cannot.
    pub switchable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CasaData {
    pub state: Home,
    /// How many buttons are on.
    pub on: u8,
    pub buttons: Vec<HomeButton>,
}

#[derive(Clone, Default, PartialEq)]
pub struct Readings {
    pub sistema: Option<SistemaData>,
    pub clima: Option<ClimaData>,
    pub reloj: Option<RelojData>,
    pub casa: Option<CasaData>,
    pub ia: Option<usage::Snapshot>,
}

/// Which widgets the menu bar shows, left and right.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BarLayout {
    pub left: Vec<WidgetId>,
    pub right: Vec<WidgetId>,
}

impl BarLayout {
    pub fn has(&self, id: WidgetId) -> bool {
        self.left.contains(&id) || self.right.contains(&id)
    }
}

static READINGS: Mutex<Readings> = Mutex::new(Readings {
    sistema: None,
    clima: None,
    reloj: None,
    casa: None,
    ia: None,
});
static SETTINGS: Mutex<Option<Settings>> = Mutex::new(None);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static TARGET_MESSAGE: AtomicU32 = AtomicU32::new(0);
static SISTEMA_ON: AtomicBool = AtomicBool::new(false);
static CLIMA_ON: AtomicBool = AtomicBool::new(false);
static RELOJ_ON: AtomicBool = AtomicBool::new(false);
static CASA_ON: AtomicBool = AtomicBool::new(false);
static IA_ON: AtomicBool = AtomicBool::new(false);

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
    if previous.is_some_and(|old| old.weather_city != settings.weather_city) {
        if let Ok(mut readings) = READINGS.lock() {
            readings.clima = None;
        }
        clima::wake();
    }
    // Several widgets can share one source (the AI limits).
    let mut wanted: Vec<*const AtomicBool> = Vec::new();
    for id in WidgetId::ALL {
        let (flag, start) = id.source();
        if placed.contains(&id) {
            if !flag.swap(true, Ordering::AcqRel) {
                start();
            }
            wanted.push(flag);
        }
    }
    for id in WidgetId::ALL {
        let (flag, _) = id.source();
        if !wanted.contains(&std::ptr::from_ref(flag)) {
            flag.store(false, Ordering::Release);
        }
    }
}

fn settings() -> Option<Settings> {
    SETTINGS.lock().ok().and_then(|slot| slot.clone())
}

/// The menu bar placement from the current settings.
pub fn bar_layout() -> BarLayout {
    settings()
        .map(|settings| BarLayout {
            left: ids(&settings.bar_left),
            right: ids(&settings.bar_right),
        })
        .unwrap_or_default()
}

/// The widgets placed in the dock, in order.
pub fn dock() -> Vec<WidgetId> {
    settings().map(|s| ids(&s.dock)).unwrap_or_default()
}

fn city() -> String {
    settings().map(|s| s.weather_city).unwrap_or_default()
}

/// Home Assistant address and buttons: this app's, else the Tauri Barra's.
fn home_settings() -> (String, Vec<HomeItem>) {
    let own = settings()
        .map(|s| (s.home_url.trim_end_matches('/').to_owned(), s.home_items))
        .unwrap_or_default();
    if !own.0.is_empty() {
        return own;
    }
    casa::from_tauri_barra().unwrap_or_default()
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

/// Development aid: `BARRA_PANEL=casa` (debug builds) opens that widget's
/// panel once at start, to check panels without clicking.
pub fn open_debug_panel(gfx: &crate::gfx::Gfx) {
    static DONE: AtomicBool = AtomicBool::new(false);
    if !cfg!(debug_assertions) || DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(id) = std::env::var("BARRA_PANEL").ok().and_then(|k| WidgetId::from_key(&k)) else {
        return;
    };
    let Some(monitor) = muelle_shell::system::primary_monitor() else {
        return;
    };
    let anchor = windows::Win32::Foundation::POINT {
        x: monitor.screen.right - (260.0 * monitor.scale) as i32,
        y: monitor.screen.top + (28.0 * monitor.scale) as i32,
    };
    toggle_panel(gfx, id, anchor, Placement::Below);
}
