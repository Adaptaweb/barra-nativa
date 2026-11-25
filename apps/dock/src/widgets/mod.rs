//! Barra widgets: small live readings drawn natively in the menu bar and the
//! dock: system load, weather, the clock with the calendar, Home Assistant
//! and AI plan limits.
//!
//! Each data source runs on its own thread only while a widget that needs it
//! is placed somewhere. A source stores its reading here and posts one message
//! to the menu bar when the reading changes, so the bar and the dock repaint
//! just that widget. Nothing is polled on the UI thread.

mod calendario;
mod casa;
mod cell;
mod clima;
mod control;
mod draw;
mod google;
mod ia;
mod more;
mod panel;
mod panel_content;
mod reloj;
mod secrets;
mod sistema;

pub use cell::Size;
pub use draw::render_bar;
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
    /// The programs in the notification area: opens the Windows flyout of
    /// hidden tray icons.
    Bandeja,
    /// Network, volume and battery; opens the Windows quick settings.
    Control,
    /// A bell button; opens the Windows notification center.
    Notificaciones,
}

impl WidgetId {
    pub const ALL: [Self; 10] = [
        Self::Bandeja,
        Self::Control,
        Self::Notificaciones,
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
            Self::Bandeja => "bandeja",
            Self::Control => "control",
            Self::Notificaciones => "notificaciones",
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
            Self::Bandeja => "Segundo plano",
            Self::Control => "Centro de control",
            Self::Notificaciones => "Notificaciones",
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
            Self::Bandeja | Self::Control | Self::Notificaciones => String::new(),
        }
    }

    pub fn entry_id(self) -> String {
        format!("{ENTRY_PREFIX}{}", self.key())
    }

    pub fn from_entry(id: &str) -> Option<Self> {
        Self::from_key(id.strip_prefix(ENTRY_PREFIX)?)
    }

    /// Like `fingerprint`, but ignoring what the menu bar does not show
    /// (the clock's seconds).
    pub fn bar_fingerprint(self, readings: &Readings) -> String {
        match self {
            Self::Reloj => format!(
                "{:?}",
                readings
                    .reloj
                    .as_ref()
                    .map(|d| RelojData { second: 0, ..d.clone() })
            ),
            _ => self.fingerprint(readings),
        }
    }

    /// Changes when what the widget shows changes.
    pub fn fingerprint(self, readings: &Readings) -> String {
        match self {
            Self::Sistema => format!("{:?}", readings.sistema),
            Self::Clima => format!("{:?}", readings.clima),
            Self::Reloj => format!("{:?}", readings.reloj),
            Self::Casa => format!("{:?}", readings.casa),
            // The dock shows only its chevron; the icons are in the panel.
            Self::Bandeja | Self::Notificaciones => String::new(),
            Self::Control => format!("{:?}", readings.control),
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
            Self::Bandeja | Self::Control | Self::Notificaciones => true,
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
            Self::Bandeja => (&BANDEJA_ON, start_tray),
            Self::Control => (&CONTROL_ON, control::start),
            // Nothing to read: a shape flag with no source thread (`configure` sets it).
            Self::Notificaciones => (&NOTIFICACIONES_ON, no_source),
        }
    }
}

/// Dock entry ids start with this, then the widget key.
const ENTRY_PREFIX: &str = "widget:";

/// Why the CPU temperature is, or is not, available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempSource {
    /// There is a temperature.
    Ok,
    /// No source answers and LibreHardwareMonitor is not installed.
    NoLhm,
    /// No source answers but LibreHardwareMonitor is installed.
    LhmClosed,
    /// A source answers but gives no plausible CPU temperature.
    NoSensor,
}

/// Latest system reading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SistemaData {
    /// CPU package temperature, °C, when a sensor source answers.
    pub temp: Option<u8>,
    pub temp_why: TempSource,
    /// LibreHardwareMonitor is being installed.
    pub installing: bool,
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
    /// Today's high and low.
    pub max: i16,
    pub min: i16,
    /// The next hours: hour ("15"), WMO code and temperature.
    pub hours: Vec<(String, u8, i16)>,
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

/// An event of the month agenda, with the details the month panel shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgendaEvent {
    pub title: String,
    /// Unix seconds. For all-day events `end` is exclusive.
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub location: String,
    /// Video call or event page; empty when there is none.
    pub link: String,
    /// Name of the calendar the event belongs to.
    pub calendar: String,
    /// Calendar colour as 0xRRGGBB; 0 when unknown.
    pub color: u32,
}

/// Events of the previous, current and next month.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Agenda {
    pub events: Vec<AgendaEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelojData {
    /// Date and time as the menu bar shows them ("mar 24 nov 19:39").
    pub clock: String,
    /// Seconds of the current minute, so the dock time advances every second.
    pub second: u32,
    /// Day of the month and weekday, for the dock tile.
    pub day: String,
    pub weekday: String,
    pub calendar: Calendar,
    pub next: Option<Event>,
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

/// A tray icon with its picture.
#[derive(Clone, Debug)]
pub struct TrayItem {
    pub icon: muelle_shell::tray::TrayIcon,
    pub image: Option<std::sync::Arc<muelle_shell::icons::IconImage>>,
}

impl PartialEq for TrayItem {
    fn eq(&self, other: &Self) -> bool {
        self.icon == other.icon
    }
}

#[derive(Clone, Default, PartialEq)]
pub struct Readings {
    /// Apps in the notification area.
    pub tray: Vec<TrayItem>,
    pub sistema: Option<SistemaData>,
    pub clima: Option<ClimaData>,
    pub reloj: Option<RelojData>,
    /// Calendar events of three months, for the month panel.
    pub agenda: Option<Agenda>,
    pub casa: Option<CasaData>,
    pub ia: Option<usage::Snapshot>,
    pub control: Option<ControlData>,
}

/// What the quick settings button shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlData {
    pub network: muelle_shell::status::Network,
    pub volume: Option<muelle_shell::status::Volume>,
    pub battery: Option<muelle_shell::system::Battery>,
    pub bluetooth: bool,
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
    tray: Vec::new(),
    sistema: None,
    clima: None,
    reloj: None,
    agenda: None,
    casa: None,
    ia: None,
    control: None,
});
static SETTINGS: Mutex<Option<Settings>> = Mutex::new(None);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static TARGET_MESSAGE: AtomicU32 = AtomicU32::new(0);
static SISTEMA_ON: AtomicBool = AtomicBool::new(false);
static CLIMA_ON: AtomicBool = AtomicBool::new(false);
static RELOJ_ON: AtomicBool = AtomicBool::new(false);
static CASA_ON: AtomicBool = AtomicBool::new(false);
static IA_ON: AtomicBool = AtomicBool::new(false);
static BANDEJA_ON: AtomicBool = AtomicBool::new(false);
static CONTROL_ON: AtomicBool = AtomicBool::new(false);
static NOTIFICACIONES_ON: AtomicBool = AtomicBool::new(false);

/// The bell has no data source to start.
fn no_source() {}

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
    let settings = &for_this_display(settings);
    let placed: Vec<WidgetId> = ids(&settings.bar_left)
        .into_iter()
        .chain(ids(&settings.bar_right))
        .chain(ids(&settings.dock))
        .chain(ids(&settings.dock_left))
        .chain(ids(&settings.island_left))
        .chain(ids(&settings.island_right))
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

/// The widgets of this dock's display: a secondary display has its own
/// placement in `screens`; everything else (city, Home Assistant) is shared.
fn for_this_display(settings: &Settings) -> Settings {
    let Some(device) = muelle_shell::displays::home_device() else {
        return settings.clone();
    };
    let own = settings.screens.get(device).cloned().unwrap_or_default();
    Settings {
        bar_left: own.bar_left,
        bar_right: own.bar_right,
        dock_left: own.dock_left,
        dock: own.dock,
        island_left: own.island_left,
        island_right: own.island_right,
        dock_sizes: own.dock_sizes,
        ..settings.clone()
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

/// The widgets on the right of the dock, in order, with their size.
pub fn dock() -> Vec<(WidgetId, Size)> {
    dock_zone(false)
}

/// The widgets on the left of the dock, in order, with their size.
pub fn dock_left() -> Vec<(WidgetId, Size)> {
    dock_zone(true)
}

/// The widgets of a separate island (bottom left or right), in order.
pub fn islands(left: bool) -> Vec<(WidgetId, Size)> {
    settings()
        .map(|s| {
            ids(if left { &s.island_left } else { &s.island_right })
                .into_iter()
                .map(|id| (id, Size::from_key(s.dock_sizes.get(id.key()).map(String::as_str))))
                .collect()
        })
        .unwrap_or_default()
}

fn dock_zone(left: bool) -> Vec<(WidgetId, Size)> {
    settings()
        .map(|s| {
{
            let mut list: Vec<(WidgetId, Size)> = ids(if left { &s.dock_left } else { &s.dock })
                .into_iter()
                .map(|id| (id, Size::from_key(s.dock_sizes.get(id.key()).map(String::as_str))))
                .collect();
            // The tray icons always close the dock, at its far right.
            if let Some(index) = list.iter().position(|(id, _)| *id == WidgetId::Bandeja) {
                let tray = list.remove(index);
                if !left {
                    list.push(tray);
                }
            }
            list
        }
        })
        .unwrap_or_default()
}

/// The size of a widget in the dock.
pub fn dock_size(id: WidgetId) -> Size {
    dock()
        .into_iter()
        .chain(dock_left())
        .chain(islands(true))
        .chain(islands(false))
        .find(|(w, _)| *w == id)
        .map_or(Size::Medium, |(_, s)| s)
}

/// Cell width in the dock, in hundredths of an icon.
pub fn dock_width(id: WidgetId, size: Size) -> u16 {
    cell::width(id, size)
}

/// The dock cell for a widget, `height` pixels tall, and whether its width
/// changed (the dock must then be laid out again).
pub fn render_cell(
    gfx: &crate::gfx::Gfx,
    id: WidgetId,
    readings: &Readings,
    height: u32,
    light: bool,
) -> anyhow::Result<(windows::UI::Composition::CompositionSurfaceBrush, bool)> {
    cell::render(gfx, id, dock_size(id), readings, height, light)
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
    // On the display BARRA_MONITOR points at, if set (see `dock_monitor`).
    let chosen = std::env::var("BARRA_MONITOR").ok().and_then(|v| {
        let (x, y) = v.split_once(',')?;
        muelle_shell::system::monitor_at(windows::Win32::Foundation::POINT {
            x: x.trim().parse().ok()?,
            y: y.trim().parse().ok()?,
        })
    });
    let Some(monitor) = chosen.or_else(muelle_shell::system::primary_monitor) else {
        return;
    };
    let anchor = windows::Win32::Foundation::POINT {
        x: monitor.screen.right - (260.0 * monitor.scale) as i32,
        y: monitor.screen.top + (28.0 * monitor.scale) as i32,
    };
    toggle_panel(gfx, id, anchor, Placement::Below);
}

/// Asks for another pass of the dock layout (a cell's width changed).
pub fn nudge() {
    publish(|_| true);
}

/// Starts this process's tray window (once) and keeps `readings.tray` current.
fn start_tray() {
    // Apps hand tray icons to one window: the primary display's dock has it.
    if muelle_shell::displays::is_secondary_process() {
        return;
    }
    fn changed() {
        let icons = muelle_shell::tray::icons();
        let previous = READINGS.lock().map(|r| r.tray.clone()).unwrap_or_default();
        let items: Vec<TrayItem> = icons
            .into_iter()
            .map(|icon| {
                // Reuse the picture when the icon handle did not change.
                let image = previous
                    .iter()
                    .find(|p| p.icon == icon)
                    .and_then(|p| p.image.clone())
                    .or_else(|| muelle_shell::tray::image(&icon).map(std::sync::Arc::new));
                TrayItem { icon, image }
            })
            .collect();
        publish(|r| {
            let changed = r.tray != items;
            r.tray = items;
            changed
        });
    }
    muelle_shell::tray::start(changed);
}
