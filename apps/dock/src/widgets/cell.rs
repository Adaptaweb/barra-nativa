//! Widgets in the dock, drawn like the Tauri Barra's cells: no surface of
//! their own, straight on the dock glass, as tall as an icon inside the dock
//! (taller in the islands) and as wide as their size (compact, medium, wide)
//! asks. Icon, big figures, faded
//! details, thin separators.

use super::draw::{GREEN, ORANGE, RED, ink};
use super::clima::describe;
use super::musica::{NO_ARTWORK, NO_ARTWORK_2};
use super::{Calendar, ClimaData, Home, Readings, WidgetId};
use crate::gfx::{Gfx, Pen, Rgba, Weight};
use crate::media::{self, Control};
use crate::usage::{self, State};
use anyhow::Result;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering::Relaxed},
};
use chrono::{DateTime, Local, TimeZone, Utc};
use windows::UI::Composition::CompositionSurfaceBrush;

const ACCENT_RED: Rgba = [1.0, 0.271, 0.227, 1.0];
const HL_ORANGE: Rgba = [1.0, 0.62, 0.2, 1.0];
const HOME_YELLOW: Rgba = [1.0, 0.82, 0.4, 1.0];
/// Orange tile behind the home glyph, like the iOS Home app (#FF9F0A).
const HOME_ORANGE: Rgba = [1.0, 0.624, 0.039, 1.0];
const CALENDAR_COLORS: [Rgba; 4] = [
    [0.039, 0.518, 1.0, 1.0],
    [0.204, 0.78, 0.349, 1.0],
    [1.0, 0.624, 0.039, 1.0],
    [0.749, 0.353, 0.949, 1.0],
];

/// What a click on a part of a cell does, instead of opening its panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CellAction {
    /// A transport button of the music widget.
    Media(Control),
    /// The microphone button of the audio widget.
    ToggleMic,
}

/// The size the user chose for a widget in the dock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    Compact,
    Medium,
    Wide,
    /// A weather card on its own colour; only the weather widget has it.
    Estado,
}

impl Size {
    pub fn from_key(key: Option<&str>) -> Self {
        match key {
            Some("compacto") => Self::Compact,
            Some("ancho") => Self::Wide,
            Some("estado") => Self::Estado,
            _ => Self::Medium,
        }
    }

    /// The size a widget really draws at: `Estado` is only for the weather.
    fn for_widget(self, id: WidgetId) -> Self {
        if self == Self::Estado && id != WidgetId::Clima {
            Self::Medium
        } else {
            self
        }
    }
}

/// Width of the cell in hundredths of an icon. Fixed per widget and size so a
/// changing reading never moves the dock.
pub fn width(id: WidgetId, size: Size) -> u16 {
    let size = size.for_widget(id);
    if let Some(known) = WIDTHS
        .lock()
        .ok()
        .and_then(|w| w.iter().find(|(key, _)| *key == (id, size)).map(|(_, v)| *v))
    {
        return known;
    }
    match (id, size) {
        (WidgetId::Bandeja, _) => 70,
        (WidgetId::Notificaciones, _) => 100,
        (WidgetId::Control, Size::Compact) => 120,
        (WidgetId::Control, _) => 200,
        (WidgetId::Reloj, Size::Compact) => 190,
        (WidgetId::Reloj, Size::Medium | Size::Estado) => 410,
        (WidgetId::Reloj, Size::Wide) => 800,
        (WidgetId::Clima, Size::Compact) => 160,
        (WidgetId::Clima, Size::Medium) => 300,
        (WidgetId::Clima, Size::Wide) => 480,
        (WidgetId::Clima, Size::Estado) => 420,
        (WidgetId::Sistema, Size::Compact) => 100,
        (WidgetId::Sistema, Size::Medium | Size::Wide | Size::Estado) => 370,
        (WidgetId::Memoria, Size::Compact) => 100,
        (WidgetId::Memoria, Size::Medium | Size::Estado) => 260,
        (WidgetId::Memoria, Size::Wide) => 380,
        (WidgetId::Bateria, Size::Compact) => 110,
        (WidgetId::Bateria, Size::Medium | Size::Estado) => 280,
        (WidgetId::Bateria, Size::Wide) => 460,
        (WidgetId::Casa, Size::Compact) => 140,
        (WidgetId::Casa, Size::Medium | Size::Wide | Size::Estado) => 250,
        (WidgetId::Musica, Size::Compact) => 170,
        (WidgetId::Musica, Size::Medium | Size::Estado) => 460,
        (WidgetId::Musica, Size::Wide) => 560,
        (WidgetId::Audio, Size::Compact) => 100,
        (WidgetId::Audio, Size::Medium | Size::Estado) => 190,
        (WidgetId::Audio, Size::Wide) => 380,
        (WidgetId::Claude | WidgetId::ChatGpt | WidgetId::Gemini, Size::Compact) => 170,
        (WidgetId::Claude | WidgetId::ChatGpt | WidgetId::Gemini, _) => 500,
    }
}

static WIDTHS: Mutex<Vec<((WidgetId, Size), u16)>> = Mutex::new(Vec::new());

/// Click zones of a cell: left and right edge in cell pixels, and what a click there does.
pub type Zones = Vec<(f32, f32, CellAction)>;

/// Draws the cell `height` pixels tall, as wide as its content. The width it
/// needed is remembered for the dock layout (see `width`); returns whether it
/// changed, and the parts of the cell a click acts on (x from, x to, action).
/// With `reuse`, a cell that keeps its width is drawn again on that brush's
/// surface instead of a new one.
pub fn render(
    gfx: &Gfx,
    id: WidgetId,
    size: Size,
    readings: &Readings,
    height: u32,
    light: bool,
    reuse: Option<&CompositionSurfaceBrush>,
) -> Result<(CompositionSurfaceBrush, bool, Zones)> {
    let size = size.for_widget(id);
    let height = height.max(16) as f32;
    // Text sizes are relative to a 48 pixel tall cell, as in the Barra dock.
    let unit = height / 48.0;
    let pad = (height * 0.14).round();
    let draw = |width: f32, end: &mut f32, zones: &mut Zones, reuse: Option<&CompositionSurfaceBrush>| {
        gfx.paint_into(reuse, (width, height), unit, |pen| {
            let mut c = Cell {
                pen,
                gfx,
                unit,
                width,
                height,
                light,
                x: pad,
                zones: Vec::new(),
            };
            match id {
                WidgetId::Reloj => reloj(&mut c, size, readings),
                WidgetId::Clima => clima(&mut c, size, readings),
                WidgetId::Sistema => sistema(&mut c, size, readings),
                WidgetId::Memoria => memoria(&mut c, size, readings),
                WidgetId::Bateria => bateria(&mut c, size, readings),
                WidgetId::Casa => casa(&mut c, size, readings),
                WidgetId::Bandeja => bandeja(&mut c, readings),
                WidgetId::Control => control(&mut c, size, readings),
                WidgetId::Notificaciones => notificaciones(&mut c),
                WidgetId::Musica => musica(&mut c, size, readings),
                WidgetId::Audio => audio(&mut c, size, readings),
                WidgetId::Claude | WidgetId::ChatGpt | WidgetId::Gemini => {
                    ia(&mut c, id, size, readings)
                }
            }?;
            *end = c.x;
            *zones = c.zones;
            Ok(())
        })
    };
    // In steps of a tenth of an icon, so small changes do not move the dock.
    let step = |end: f32| ((((end + pad) / height) * 10.0).ceil() * 10.0).clamp(60.0, 900.0) as u16;
    let to_px = |hundredths: u16| (height * f32::from(hundredths) / 100.0).round();
    let known = WIDTHS
        .lock()
        .ok()
        .and_then(|w| w.iter().find(|(key, _)| *key == (id, size)).map(|(_, v)| *v));
    let mut end = 0.0;
    let mut zones = Vec::new();
    // Drawn once at the width it had last time: the content's end tells
    // whether that still fits. Only a new width needs a second pass (the
    // first time, a roomy surface just to learn it).
    let (hundredths, cell) = match known {
        Some(hundredths) => {
            let cell = draw(to_px(hundredths), &mut end, &mut zones, reuse)?;
            let now = step(end);
            if now == hundredths {
                (hundredths, cell)
            } else {
                (now, draw(to_px(now), &mut end, &mut zones, None)?)
            }
        }
        None => {
            draw(height * 9.0, &mut end, &mut Vec::new(), None)?;
            let now = step(end);
            (now, draw(to_px(now), &mut end, &mut zones, None)?)
        }
    };
    let changed = WIDTHS.lock().is_ok_and(|mut widths| {
        match widths.iter_mut().find(|(key, _)| *key == (id, size)) {
            Some((_, old)) if *old == hundredths => false,
            Some((_, old)) => {
                *old = hundredths;
                true
            }
            None => {
                widths.push(((id, size), hundredths));
                true
            }
        }
    });
    Ok((cell.brush, changed, zones))
}

/// The tray: a chevron in the same rounded tile as the notifications and
/// control centre buttons; the icons live in its bubble, not loose in the dock.
fn bandeja(c: &mut Cell<'_, '_>, _r: &Readings) -> Result<()> {
    let side = (c.height * 0.84).round();
    let top = ((c.height - side) / 2.0).round();
    c.pen.round_rect((c.x, top), (side, side), side * 0.28, ink(c.light, 0.10))?;
    let size = 17.0;
    let (gw, gh) = c.gfx.measure_icon('\u{E70E}', size, c.unit)?;
    c.pen.icon('\u{E70E}', (c.x + (side - gw) / 2.0, top + (side - gh) / 2.0), size, ink(c.light, 0.75))?;
    c.x += side;
    Ok(())
}

/// A round bell button inside a rounded square; opens the notification center.
fn notificaciones(c: &mut Cell<'_, '_>) -> Result<()> {
    let side = (c.height * 0.84).round();
    let top = ((c.height - side) / 2.0).round();
    c.pen.round_rect((c.x, top), (side, side), side * 0.28, ink(c.light, 0.10))?;
    let disc = (side * 0.64).round();
    let offset = ((side - disc) / 2.0).round();
    c.pen.round_rect((c.x + offset, top + offset), (disc, disc), disc / 2.0, ink(c.light, 0.10))?;
    let glyph = 17.0;
    let (gw, gh) = c.gfx.measure_icon('\u{EA8F}', glyph, c.unit)?;
    c.pen.icon(
        '\u{EA8F}',
        (c.x + (side - gw) / 2.0, top + (side - gh) / 2.0),
        glyph,
        c.strong(),
    )?;
    c.x += side;
    Ok(())
}

/// iOS control-centre blue for a switch that is on.
const SWITCH_ON: Rgba = [0.04, 0.52, 1.0, 1.0];

/// A small control centre in the same rounded tile as the notifications and
/// tray buttons: four discs in a 2 × 2 grid for the network, Bluetooth,
/// volume and Do not disturb, blue when on, grey when off. The battery has
/// its own widget. A click opens the Windows quick settings.
fn control(c: &mut Cell<'_, '_>, _size: Size, r: &Readings) -> Result<()> {
    let side = (c.height * 0.84).round();
    let top = ((c.height - side) / 2.0).round();
    c.pen.round_rect((c.x, top), (side, side), side * 0.28, ink(c.light, 0.10))?;
    let pad = (side * 0.13).round();
    let gap = (side * 0.07).round();
    let disc = ((side - pad * 2.0 - gap) / 2.0).floor();
    let glyph_size = disc * 0.5 / c.unit;
    let items: [(char, bool); 4] = match r.control {
        Some(d) => [
            (super::control::network_glyph(d.network), !matches!(d.network, muelle_shell::status::Network::Offline)),
            ('\u{E702}', d.bluetooth),
            (super::control::volume_glyph(d.volume), !d.volume.is_none_or(|v| v.muted)),
            ('\u{E708}', d.quiet),
        ],
        None => [('\u{E701}', false), ('\u{E702}', false), ('\u{E995}', false), ('\u{E708}', false)],
    };
    for (index, (glyph, on)) in items.into_iter().enumerate() {
        let x = c.x + pad + (disc + gap) * (index % 2) as f32;
        let y = top + pad + (disc + gap) * (index / 2) as f32;
        let back = if on { SWITCH_ON } else { ink(c.light, 0.16) };
        c.pen.round_rect((x, y), (disc, disc), disc / 2.0, back)?;
        let (gw, gh) = c.gfx.measure_icon(glyph, glyph_size, c.unit)?;
        let color = if on { [1.0, 1.0, 1.0, 1.0] } else { c.strong() };
        c.pen.icon(glyph, (x + (disc - gw) / 2.0, y + (disc - gh) / 2.0), glyph_size, color)?;
    }
    c.x += side;
    Ok(())
}
/// Lays the cell out left to right.
struct Cell<'a, 'b> {
    pen: &'a Pen<'b>,
    gfx: &'a Gfx,
    unit: f32,
    width: f32,
    height: f32,
    light: bool,
    x: f32,
    /// Where a click does something of its own: x from, x to, what.
    zones: Zones,
}

impl Cell<'_, '_> {
    fn px(&self, v: f32) -> f32 {
        (v * self.unit).round()
    }

    fn gap(&mut self) {
        self.x += self.px(10.0);
    }

    fn line(&self, size: f32, weight: Weight) -> Result<f32> {
        Ok(self.gfx.measure("Ág", size, self.unit, weight)?.1)
    }

    /// Text centred vertically at the cell's middle; advances.
    fn text(&mut self, text: &str, size: f32, weight: Weight, color: Rgba) -> Result<()> {
        let (_, h) = self.gfx.measure(text, size, self.unit, weight)?;
        let w = self.pen.text(text, (self.x, (self.height - h) / 2.0), size, weight, color)?;
        self.x += w;
        Ok(())
    }

    /// Two lines stacked and centred, `top` over `bottom`; advances by the wider.
    fn column(
        &mut self,
        top: (&str, f32, Weight, Rgba),
        bottom: (&str, f32, Weight, Rgba),
        centered: bool,
        max_chars: usize,
    ) -> Result<()> {
        let top_text = shorten(top.0, max_chars);
        let bottom_text = shorten(bottom.0, max_chars);
        let (tw, th) = self.gfx.measure(&top_text, top.1, self.unit, top.2)?;
        let (bw, bh) = self.gfx.measure(&bottom_text, bottom.1, self.unit, bottom.2)?;
        let gap = self.px(1.0);
        let y = (self.height - th - bh - gap) / 2.0;
        let width = tw.max(bw);
        let (tx, bx) = if centered {
            (self.x + (width - tw) / 2.0, self.x + (width - bw) / 2.0)
        } else {
            (self.x, self.x)
        };
        self.pen.text(&top_text, (tx, y), top.1, top.2, top.3)?;
        self.pen.text(&bottom_text, (bx, y + th + gap), bottom.1, bottom.2, bottom.3)?;
        self.x += width;
        Ok(())
    }

    /// A thin vertical divider, 60% of the height, with room on both sides.
    fn separator(&mut self) -> Result<()> {
        self.gap();
        let h = self.height * 0.6;
        self.pen.round_rect(
            (self.x, (self.height - h) / 2.0),
            (self.px(1.0).max(1.0), h),
            0.0,
            ink(self.light, 0.16),
        )?;
        self.x += self.px(1.0).max(1.0);
        self.gap();
        Ok(())
    }

    /// Width and height of the clock time (`HH:MM` big plus `:SS` small); the height is the big box's.
    fn clock_measure(&self, now: &DateTime<Local>, big: f32, small: f32) -> Result<(f32, f32)> {
        use chrono::Timelike;
        let main = format!("{:02}:{:02}", now.hour(), now.minute());
        let secs = format!(":{:02}", now.second());
        let (mw, hb) = self.gfx.measure(&main, big, self.unit, Weight::Heavy)?;
        let (sw, _) = self.gfx.measure(&secs, small, self.unit, Weight::Heavy)?;
        Ok((mw + sw, hb))
    }

    /// Draws `HH:MM` big and `:SS` small and grey on the same baseline; returns the total width.
    fn clock_draw(&self, now: &DateTime<Local>, x: f32, top: f32, big: f32, small: f32) -> Result<f32> {
        use chrono::Timelike;
        /// Share of a Segoe UI line box above the baseline (ascent 2210 / (ascent + descent) 2724).
        const BASELINE: f32 = 0.81;
        let main = format!("{:02}:{:02}", now.hour(), now.minute());
        let secs = format!(":{:02}", now.second());
        let (_, hb) = self.gfx.measure(&main, big, self.unit, Weight::Heavy)?;
        let (_, hs) = self.gfx.measure(&secs, small, self.unit, Weight::Heavy)?;
        let mw = self.pen.text(&main, (x, top), big, Weight::Heavy, self.strong())?;
        let y_small = top + (hb - hs) * BASELINE;
        let sw = self.pen.text(&secs, (x + mw, y_small), small, Weight::Heavy, self.faded())?;
        Ok(mw + sw)
    }

    fn faded(&self) -> Rgba {
        ink(self.light, 0.55)
    }

    fn strong(&self) -> Rgba {
        ink(self.light, 1.0)
    }
}


fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

fn local(seconds: i64) -> Option<DateTime<Local>> {
    Local.timestamp_opt(seconds, 0).single()
}

/// "en 1 h 20 min", "en 5 min", "ahora".
fn in_time(seconds: i64) -> String {
    let minutes = (seconds + 59) / 60;
    if minutes <= 0 {
        return "ahora".into();
    }
    if minutes < 60 {
        return format!("en {minutes} min");
    }
    let (h, m) = (minutes / 60, minutes % 60);
    if m == 0 { format!("en {h} h") } else { format!("en {h} h {m} min") }
}

const WEEKDAYS: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];
const MONTHS: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"];

fn reloj(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    use chrono::Datelike;
    let now = Local::now();
    let weekday = WEEKDAYS[now.weekday().num_days_from_monday() as usize];
    if size == Size::Compact {
        let short: String = weekday.chars().take(3).collect();
        let date = format!("{short} {}", now.day());
        let (tw, th) = c.clock_measure(&now, 24.0, 14.0)?;
        let (dw, dh) = c.gfx.measure(&date, 10.5, c.unit, Weight::Normal)?;
        let total = tw.max(dw);
        let x = c.x;
        let y = (c.height - th - dh) / 2.0;
        c.clock_draw(&now, x + (total - tw) / 2.0, y, 24.0, 14.0)?;
        c.pen.text(&date, (x + (total - dw) / 2.0, y + th), 10.5, Weight::Normal, c.faded())?;
        c.x += total;
        return Ok(());
    }
    let date = format!("{} {}", now.day(), MONTHS[now.month0() as usize]);
    c.column(
        (&weekday.to_uppercase(), 9.5, Weight::Bold, ACCENT_RED),
        (&date, 24.0, Weight::Heavy, c.strong()),
        false,
        12,
    )?;
    c.separator()?;
    let (_, hb) = c.clock_measure(&now, 30.0, 17.0)?;
    let width = c.clock_draw(&now, c.x, (c.height - hb) / 2.0, 30.0, 17.0)?;
    c.x += width;
    if size != Size::Wide {
        return Ok(());
    }
    c.separator()?;
    let Some(d) = r.reloj.as_ref() else {
        return Ok(());
    };
    let max_chars = 26;
    match (&d.calendar, &d.next) {
        (Calendar::NotConnected, _) => c.text("Conecta tu calendario", 11.0, Weight::Normal, c.faded()),
        (Calendar::Failed, _) => c.column(
            ("Calendario", 12.0, Weight::Bold, c.strong()),
            ("Vuelve a conectarlo", 10.5, Weight::Normal, c.faded()),
            false,
            max_chars,
        ),
        (_, None) => c.text("Sin eventos próximos", 11.0, Weight::Normal, c.faded()),
        (_, Some(event)) => {
            let bar_h = c.height * 0.6;
            c.pen.round_rect(
                (c.x, (c.height - bar_h) / 2.0),
                (c.px(4.0), bar_h),
                c.px(2.0),
                CALENDAR_COLORS[0],
            )?;
            c.x += c.px(4.0) + c.px(8.0);
            let now_s = Utc::now().timestamp();
            let when = if event.all_day {
                "Todo el día".to_owned()
            } else if event.start <= now_s {
                format!("Ahora · hasta {}", local(event.end).map_or_else(String::new, |t| t.format("%H:%M").to_string()))
            } else {
                format!(
                    "{} · {}",
                    local(event.start).map_or_else(String::new, |t| t.format("%H:%M").to_string()),
                    in_time(event.start - now_s)
                )
            };
            c.column(
                (&event.title, 12.0, Weight::Bold, c.strong()),
                (&when, 10.5, Weight::Normal, c.faded()),
                false,
                max_chars,
            )
        }
    }
}

const SUN_RAY: Rgba = [1.0, 0.80, 0.0, 1.0];
const SUN_TOP: Rgba = [1.0, 0.88, 0.30, 1.0];
const SUN_BOTTOM: Rgba = [1.0, 0.62, 0.04, 1.0];
const MOON: Rgba = [1.0, 0.91, 0.66, 1.0];
const CLOUD: Rgba = [0.93, 0.95, 0.98, 1.0];
const CLOUD_DARK: Rgba = [0.70, 0.74, 0.81, 1.0];
const RAIN: Rgba = [0.35, 0.70, 1.0, 1.0];

/// A sun of disc `r` at `center`, with eight short rays.
fn draw_sun(pen: &Pen<'_>, center: (f32, f32), r: f32) -> Result<()> {
    for k in 0..8 {
        let a = (k as f32 * 45.0).to_radians();
        let (s, co) = a.sin_cos();
        pen.line(
            (center.0 + co * r * 1.4, center.1 + s * r * 1.4),
            (center.0 + co * r * 1.85, center.1 + s * r * 1.85),
            (r * 0.26).max(1.0),
            SUN_RAY,
        )?;
    }
    pen.gradient_rect(
        (center.0 - r, center.1 - r),
        (r * 2.0, r * 2.0),
        r,
        (center.0, center.1 - r),
        (center.0, center.1 + r),
        &[(0.0, SUN_TOP), (1.0, SUN_BOTTOM)],
    )
}

/// A cloud `w` wide (and 0.6 `w` tall) with its top left at (`x`, `y`).
fn draw_cloud(pen: &Pen<'_>, (x, y): (f32, f32), w: f32, color: Rgba) -> Result<()> {
    let h = w * 0.6;
    let disc = |cx: f32, cy: f32, r: f32| pen.round_rect((cx - r, cy - r), (r * 2.0, r * 2.0), r, color);
    disc(x + w * 0.58, y + h * 0.46, h * 0.42)?;
    disc(x + w * 0.32, y + h * 0.62, h * 0.3)?;
    pen.round_rect((x + w * 0.08, y + h * 0.55), (w * 0.84, h * 0.42), h * 0.21, color)
}

/// The weather as a drawn icon in a `side` square at (`x`, `y`), in the
/// iOS style: a yellow sun, a pale moon, white clouds, blue rain.
pub(super) fn weather_icon(pen: &Pen<'_>, (x, y): (f32, f32), side: f32, code: u8, day: bool) -> Result<()> {
    let sky = |pen: &Pen<'_>, center: (f32, f32), r: f32| {
        if day { draw_sun(pen, center, r) } else { pen.crescent(center, r * 1.15, (r * 0.6, -r * 0.4), MOON) }
    };
    match code {
        0 | 1 => sky(pen, (x + side / 2.0, y + side / 2.0), side * 0.23),
        2 => {
            sky(pen, (x + side * 0.38, y + side * 0.36), side * 0.17)?;
            draw_cloud(pen, (x + side * 0.2, y + side * 0.4), side * 0.78, CLOUD)
        }
        45 | 48 => {
            draw_cloud(pen, (x + side * 0.06, y + side * 0.1), side * 0.88, CLOUD)?;
            for k in 0..2 {
                let ly = y + side * (0.74 + 0.13 * k as f32);
                pen.line((x + side * 0.16, ly), (x + side * 0.84, ly), side * 0.07, CLOUD_DARK)?;
            }
            Ok(())
        }
        51..=67 | 80..=82 => {
            draw_cloud(pen, (x + side * 0.08, y + side * 0.08), side * 0.84, CLOUD_DARK)?;
            for k in 0..3 {
                let lx = x + side * (0.3 + 0.2 * k as f32);
                pen.line((lx, y + side * 0.7), (lx - side * 0.07, y + side * 0.9), side * 0.07, RAIN)?;
            }
            Ok(())
        }
        71..=77 | 85 | 86 => {
            draw_cloud(pen, (x + side * 0.08, y + side * 0.08), side * 0.84, CLOUD)?;
            for k in 0..3 {
                let dot = side * 0.1;
                let cx = x + side * (0.3 + 0.2 * k as f32);
                let cy = y + side * (0.76 + 0.06 * (k % 2) as f32);
                pen.round_rect((cx - dot / 2.0, cy - dot / 2.0), (dot, dot), dot / 2.0, CLOUD)?;
            }
            Ok(())
        }
        95.. => {
            draw_cloud(pen, (x + side * 0.08, y + side * 0.04), side * 0.84, CLOUD_DARK)?;
            pen.icon('\u{E945}', (x + side * 0.38, y + side * 0.52), side * 0.32 / pen.scale, SUN_RAY)?;
            Ok(())
        }
        _ => draw_cloud(pen, (x + side * 0.05, y + side * 0.2), side * 0.9, CLOUD),
    }
}

impl Cell<'_, '_> {
    /// The weather icon in a `side` (logical) square, centred on the cell's height; advances.
    fn weather(&mut self, code: u8, day: bool, side: f32) -> Result<()> {
        let side = self.px(side);
        weather_icon(self.pen, (self.x, ((self.height - side) / 2.0).round()), side, code, day)?;
        self.x += side;
        Ok(())
    }
}

fn clima(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    let Some(d) = r.clima.as_ref() else {
        return c.text("Clima…", 11.0, Weight::Normal, c.faded());
    };
    let temp = format!("{}°", d.temp);
    match size {
        // Only the icon and the temperature, as big as the clock's figures.
        Size::Compact => {
            c.weather(d.code, d.day, 34.0)?;
            c.x += c.px(8.0);
            c.text(&temp, 28.0, Weight::Heavy, c.strong())
        }
        Size::Medium => {
            c.weather(d.code, d.day, 30.0)?;
            c.x += c.px(6.0);
            c.column(
                (&temp, 20.0, Weight::Bold, c.strong()),
                (describe(d.code), 11.0, Weight::Normal, c.faded()),
                false,
                20,
            )?;
            c.separator()?;
            clima_details(c, d)
        }
        Size::Wide => {
            c.weather(d.code, d.day, 30.0)?;
            c.x += c.px(6.0);
            c.column(
                (&temp, 20.0, Weight::Bold, c.strong()),
                ("Hoy", 10.0, Weight::Normal, c.faded()),
                false,
                20,
            )?;
            c.separator()?;
            for (name, code, high, low) in &d.days {
                // Day name, icon, then the high and the low, stacked.
                let (nw, nh) = c.gfx.measure(name, 10.0, c.unit, Weight::Normal)?;
                let (ew, eh) = (c.px(17.0), c.px(17.0));
                let (hw, th) = c.gfx.measure(&format!("{high}°"), 11.0, c.unit, Weight::Bold)?;
                let low = format!(" {low}°");
                let (lw, _) = c.gfx.measure(&low, 11.0, c.unit, Weight::Normal)?;
                let w = nw.max(ew).max(hw + lw);
                let y = (c.height - nh - eh - th) / 2.0;
                c.pen.text(name, (c.x + (w - nw) / 2.0, y), 10.0, Weight::Normal, c.faded())?;
                weather_icon(c.pen, (c.x + (w - ew) / 2.0, y + nh), ew, *code, true)?;
                let x = c.x + (w - hw - lw) / 2.0;
                c.pen.text(&format!("{high}°"), (x, y + nh + eh), 11.0, Weight::Bold, c.strong())?;
                c.pen.text(&low, (x + hw, y + nh + eh), 11.0, Weight::Normal, c.faded())?;
                c.x += w + c.px(14.0);
            }
            // No gap after the last day.
            c.x -= c.px(14.0);
            Ok(())
        }
        Size::Estado => clima_status(c, d),
    }
}

/// Wind, rain and the high and low, one row each, for the medium weather.
fn clima_details(c: &mut Cell<'_, '_>, d: &ClimaData) -> Result<()> {
    let rain = if d.rain.is_multiple_of(10) {
        format!("{} mm", d.rain / 10)
    } else {
        format!("{},{} mm", d.rain / 10, d.rain % 10)
    };
    let rows = [
        ("💨", true, c.strong(), format!("{} km/h", d.wind)),
        ("💧", true, c.strong(), rain),
        ("H/L", false, HL_ORANGE, format!("{}° / {}°", d.max, d.min)),
    ];
    let size = 10.5;
    let (mut lead, mut tail, mut row) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (label, emoji, _, text) in &rows {
        let (lw, lh) = if *emoji {
            c.gfx.measure_emoji(label, size, c.unit)?
        } else {
            c.gfx.measure(label, size, c.unit, Weight::Bold)?
        };
        let (tw, th) = c.gfx.measure(text, size, c.unit, Weight::Medium)?;
        lead = lead.max(lw);
        tail = tail.max(tw);
        row = row.max(lh).max(th);
    }
    let mut y = (c.height - row * rows.len() as f32) / 2.0;
    for (label, emoji, color, text) in &rows {
        if *emoji {
            c.pen.emoji(label, (c.x, y), size)?;
        } else {
            c.pen.text(label, (c.x, y), size, Weight::Bold, *color)?;
        }
        c.pen.text(text, (c.x + lead + c.px(6.0), y), size, Weight::Medium, c.strong())?;
        y += row;
    }
    c.x += lead + c.px(6.0) + tail;
    Ok(())
}

/// The weather card: its own colour behind white text.
fn clima_status(c: &mut Cell<'_, '_>, d: &ClimaData) -> Result<()> {
    let fair = matches!(d.code, 0..=2);
    let background = match (d.day, fair) {
        (false, _) => [0.10, 0.14, 0.30, 1.0],
        (true, true) => [0.20, 0.46, 0.90, 1.0],
        (true, false) => [0.38, 0.45, 0.55, 1.0],
    };
    c.pen.round_rect((0.0, 0.0), (c.width, c.height), c.height * 0.3, background)?;
    let white = [1.0, 1.0, 1.0, 1.0];
    c.x += c.px(4.0);
    c.column(
        (describe(d.code), 14.0, Weight::Bold, white),
        (&d.city, 11.0, Weight::Normal, [1.0, 1.0, 1.0, 0.85]),
        false,
        18,
    )?;
    c.x += c.px(18.0);
    c.text(&format!("{}°", d.temp), 30.0, Weight::Normal, white)?;
    c.x += c.px(6.0);
    c.weather(d.code, d.day, 30.0)
}

/// Ring colour of the CPU temperature: green, yellow from 65, red from 80.
fn temp_color(t: u8) -> Rgba {
    match t {
        80.. => [1.0, 0.27, 0.23, 1.0],
        65.. => [1.0, 0.80, 0.0, 1.0],
        _ => [0.20, 0.78, 0.35, 1.0],
    }
}

/// A 270° gauge at `c.x`, as tall as most of the cell: its arc filled to
/// `fraction` in `color`, `value` in big heavy figures (like the clock's,
/// shrunk only if it would not fit inside) and `label` under it. Advances.
fn gauge(c: &mut Cell<'_, '_>, value: &str, label: &str, fraction: f32, color: Rgba) -> Result<()> {
    let ring = (c.height * 0.88).round();
    let center = (c.x + ring / 2.0, c.height / 2.0);
    let stroke = c.px(4.5);
    let radius = ring / 2.0 - stroke / 2.0 - c.px(1.0);
    c.pen.arc(center, radius, stroke, (225.0, 270.0), ink(c.light, 0.22))?;
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        c.pen.arc(center, radius, stroke, (225.0, 270.0 * fraction), color)?;
    }
    // The figures fill the inside of the ring.
    let room = 2.0 * (radius - stroke / 2.0) - c.px(3.0);
    let mut size = 17.0;
    let (mut nw, mut nh) = c.gfx.measure(value, size, c.unit, Weight::Heavy)?;
    while nw > room && size > 10.0 {
        size -= 1.0;
        (nw, nh) = c.gfx.measure(value, size, c.unit, Weight::Heavy)?;
    }
    c.pen.text(value, (center.0 - nw / 2.0, center.1 - nh / 2.0 - c.px(2.0)), size, Weight::Heavy, c.strong())?;
    // The label sits in the ring's opening, bright enough to tell the gauges apart.
    let (lw, lh) = c.gfx.measure(label, 9.5, c.unit, Weight::Heavy)?;
    c.pen.text(label, (center.0 - lw / 2.0, center.1 + radius - lh * 0.55), 9.5, Weight::Heavy, ink(c.light, 0.85))?;
    c.x += ring;
    Ok(())
}

/// CPU temperature, CPU load and memory, each in its own gauge; compact
/// keeps only the temperature (or the load without a sensor).
fn sistema(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    let Some(d) = r.sistema else {
        return c.text("Sistema…", 11.0, Weight::Normal, c.faded());
    };
    let temp = |c: &mut Cell<'_, '_>| match d.temp {
        Some(t) => gauge(c, &format!("{t}°"), "TEMP", f32::from(t) / 100.0, temp_color(t)),
        None => gauge(c, "—", "TEMP", 0.0, ink(c.light, 0.22)),
    };
    let cpu = |c: &mut Cell<'_, '_>| gauge(c, &format!("{}%", d.cpu), "CPU", f32::from(d.cpu) / 100.0, ram_color(d.cpu));
    if size == Size::Compact {
        return if d.temp.is_some() { temp(c) } else { cpu(c) };
    }
    let gap = c.px(8.0);
    temp(c)?;
    c.x += gap;
    cpu(c)?;
    c.x += gap;
    gauge(c, &format!("{}%", d.ram), "RAM", f32::from(d.ram) / 100.0, ram_color(d.ram))
}
/// Ring colour of the memory load: green, yellow from 70, red from 85.
pub(super) fn ram_color(percent: u8) -> Rgba {
    match percent {
        85.. => [1.0, 0.27, 0.23, 1.0],
        70.. => [1.0, 0.80, 0.0, 1.0],
        _ => [0.20, 0.78, 0.35, 1.0],
    }
}

/// "8,2": mebibytes as gibibytes with one decimal and a comma.
pub(super) fn gb_text(mb: u32) -> String {
    let tenths = super::gb_tenths(mb);
    format!("{},{}", tenths / 10, tenths % 10)
}

/// Memory: the load in a gauge like the CPU's. Medium adds the gigabytes in
/// use in big figures (like the date's) over the installed and free ones;
/// wide, a used / free bar.
fn memoria(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    let Some(d) = r.sistema else {
        return c.text("Memoria…", 11.0, Weight::Normal, c.faded());
    };
    let fraction = (f32::from(d.ram) / 100.0).clamp(0.0, 1.0);
    gauge(c, &format!("{}%", d.ram), "RAM", fraction, ram_color(d.ram))?;
    if size == Size::Compact || d.ram_total_mb == 0 {
        return Ok(());
    }
    c.x += c.px(10.0);
    let used = format!("{} GB", gb_text(d.ram_used_mb));
    let total = format!(
        "de {} GB · {} libres",
        gb_text(d.ram_total_mb),
        gb_text(d.ram_total_mb.saturating_sub(d.ram_used_mb))
    );
    c.column(
        (&used, 24.0, Weight::Heavy, c.strong()),
        (&total, 10.5, Weight::Normal, c.faded()),
        false,
        24,
    )?;
    if size != Size::Wide {
        return Ok(());
    }
    c.separator()?;
    // Used against free, with what is free under the bar.
    let free = format!("{} GB libres", gb_text(d.ram_total_mb.saturating_sub(d.ram_used_mb)));
    let (fw, fh) = c.gfx.measure(&free, 10.5, c.unit, Weight::Normal)?;
    let bar_w = c.px(84.0).max(fw);
    let bar_h = c.px(6.0);
    let gap = c.px(5.0);
    let top = (c.height - bar_h - gap - fh) / 2.0;
    c.pen.round_rect((c.x, top), (bar_w, bar_h), bar_h / 2.0, ink(c.light, 0.15))?;
    let filled = bar_w * fraction;
    if filled > 0.5 {
        c.pen.round_rect((c.x, top), (filled.max(bar_h), bar_h), bar_h / 2.0, ram_color(d.ram))?;
    }
    c.pen.text(&free, (c.x, top + bar_h + gap), 10.5, Weight::Normal, c.faded())?;
    c.x += bar_w;
    Ok(())
}

/// Seconds since the first battery paint: the water's clock.
fn water_clock() -> f32 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f32()
}

/// The water's top and bottom colours: green, yellow with battery saver,
/// red at 20 % or less while not charging.
pub(super) fn water_colors(b: muelle_shell::system::Battery) -> (Rgba, Rgba) {
    if b.percent <= 20 && !b.charging {
        ([1.0, 0.42, 0.37, 1.0], [0.85, 0.18, 0.13, 1.0])
    } else if b.saver {
        ([0.98, 0.80, 0.08, 1.0], [0.79, 0.54, 0.02, 1.0])
    } else {
        ([0.29, 0.87, 0.50, 1.0], [0.09, 0.64, 0.29, 1.0])
    }
}

/// "Quedan 4 h 10 min", "Cargando", "Con batería".
pub(super) fn battery_status(b: muelle_shell::system::Battery) -> String {
    match (b.charging, b.minutes_left) {
        (true, _) => "Cargando".to_owned(),
        (false, Some(m)) if m >= 60 => format!("Quedan {} h {} min", m / 60, m % 60),
        (false, Some(m)) => format!("Quedan {m} min"),
        (false, None) => "Con batería".to_owned(),
    }
}

/// The battery as a rounded box filling with water to its charge. Two waves
/// drift against each other and the surface bobs, so it never stands still;
/// bubbles rise while it charges. Medium adds the percent and what is left;
/// wide, battery saver and whether it is plugged in.
fn bateria(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    let Some(b) = r.control.and_then(|d| d.battery) else {
        return c.text(if r.control.is_some() { "Sin batería" } else { "Batería…" }, 11.0, Weight::Normal, c.faded());
    };
    let t = if muelle_shell::system::animations_enabled() { water_clock() } else { 0.0 };
    let side = (c.height * 0.86).round();
    let (x, y) = (c.x, ((c.height - side) / 2.0).round());
    let radius = side * 0.25;
    c.pen.round_rect((x, y), (side, side), radius, ink(c.light, 0.14))?;
    let level = (f32::from(b.percent.min(100)) / 100.0).max(0.08);
    let surface = y + side * (1.0 - level) + c.px(0.8) * (t * 2.4).sin();
    let (bright, deep) = water_colors(b);
    let back = [deep[0], deep[1], deep[2], 0.85];
    c.pen.wave((x, y), (side, side), radius, surface - c.px(1.0), (c.px(2.2), side * 0.9, 1.3 - t * 1.7), (back, back))?;
    c.pen.wave((x, y), (side, side), radius, surface + c.px(0.8), (c.px(1.8), side * 1.1, t * 2.4), (bright, deep))?;
    if b.charging {
        // Three bubbles rise from the bottom to the surface, one after another.
        for (k, (dx, d)) in [(0.28, 2.6), (0.55, 2.0), (0.74, 3.0)].into_iter().enumerate() {
            let along = (t * 0.45 + k as f32 / 3.0).fract();
            let bottom = y + side - c.px(4.0);
            let by = bottom - along * (bottom - surface - c.px(2.0)).max(0.0);
            let dot = c.px(d);
            c.pen.round_rect((x + side * dx, by), (dot, dot), dot / 2.0, [1.0, 1.0, 1.0, 0.65 * (1.0 - along)])?;
        }
    }
    let white = [1.0, 1.0, 1.0, 1.0];
    if size == Size::Compact {
        let number = b.percent.to_string();
        let (nw, nh) = c.gfx.measure(&number, 17.0, c.unit, Weight::Heavy)?;
        let (bw, bh) = if b.charging { c.gfx.measure_icon('\u{E945}', 11.0, c.unit)? } else { (0.0, 0.0) };
        let start = x + (side - nw - bw) / 2.0;
        if b.charging {
            c.pen.icon('\u{E945}', (start, y + (side - bh) / 2.0), 11.0, white)?;
        }
        c.pen.text(&number, (start + bw, y + (side - nh) / 2.0), 17.0, Weight::Heavy, white)?;
        c.x += side;
        return Ok(());
    }
    if b.charging {
        let (bw, bh) = c.gfx.measure_icon('\u{E945}', 15.0, c.unit)?;
        c.pen.icon('\u{E945}', (x + (side - bw) / 2.0, y + (side - bh) / 2.0), 15.0, white)?;
    }
    c.x += side + c.px(10.0);
    let percent = format!("{} %", b.percent);
    let status = battery_status(b);
    c.column(
        (&percent, 24.0, Weight::Heavy, c.strong()),
        (&status, 10.5, Weight::Normal, c.faded()),
        false,
        24,
    )?;
    if size != Size::Wide {
        return Ok(());
    }
    c.separator()?;
    let saver = if b.saver { "Ahorro de batería: sí" } else { "Ahorro de batería: no" };
    let plug = if b.charging { "Enchufado" } else { "Con batería" };
    c.column(
        (saver, 12.0, Weight::Bold, c.strong()),
        (plug, 10.5, Weight::Normal, c.faded()),
        false,
        24,
    )
}

fn casa(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    let side = (c.height * 0.62).round();
    let top = ((c.height - side) / 2.0).round();
    c.pen.round_rect((c.x, top), (side, side), side * 0.26, HOME_ORANGE)?;
    let glyph = 15.0;
    let (gw, gh) = c.gfx.measure_icon('\u{EA8A}', glyph, c.unit)?;
    c.pen.icon(
        '\u{EA8A}',
        (c.x + (side - gw) / 2.0, top + (side - gh) / 2.0),
        glyph,
        [1.0, 1.0, 1.0, 1.0],
    )?;
    c.x += side;
    c.x += c.px(8.0);
    let Some(d) = r.casa.as_ref() else {
        return c.text("Casa…", 11.0, Weight::Normal, c.faded());
    };
    match &d.state {
        Home::NotConnected => c.text("Sin conectar", 11.0, Weight::Normal, c.faded()),
        Home::Failed(_) => c.text("Sin conexión", 11.0, Weight::Normal, c.faded()),
        Home::Connected if size == Size::Compact => {
            c.text(&d.on.to_string(), 17.0, Weight::Bold, if d.on > 0 { HOME_YELLOW } else { c.strong() })
        }
        Home::Connected => {
            let sensor = d.buttons.iter().find(|b| !b.switchable).and_then(|b| b.value.clone());
            let mut detail = match d.on {
                1 => "1 encendido".to_owned(),
                n => format!("{n} encendidos"),
            };
            if let Some(value) = sensor {
                detail.push_str(&format!(" · {value}"));
            }
            c.column(
                ("Casa", 13.0, Weight::Bold, c.strong()),
                (&detail, 10.5, Weight::Normal, c.faded()),
                false,
                26,
            )
        }
    }
}

fn level(percent: u8) -> Rgba {
    match percent {
        90.. => RED,
        70.. => ORANGE,
        _ => GREEN,
    }
}

/// "1h 25m", "25m", "ahora"; the weekday when days away.
fn until(reset: Option<DateTime<Utc>>) -> String {
    use chrono::Datelike;
    let Some(reset) = reset else {
        return String::new();
    };
    let minutes = (reset - Utc::now()).num_minutes().max(0);
    if minutes == 0 {
        return "ahora".into();
    }
    let hours = minutes / 60;
    if hours >= 24 {
        let local = reset.with_timezone(&Local);
        let name: String = WEEKDAYS[local.weekday().num_days_from_monday() as usize].chars().take(3).collect();
        return format!("{} {name}", local.day());
    }
    if hours > 0 { format!("{hours}h {}m", minutes % 60) } else { format!("{minutes}m") }
}

fn ia(c: &mut Cell<'_, '_>, id: WidgetId, size: Size, r: &Readings) -> Result<()> {
    let Some(provider) = id.provider() else {
        return Ok(());
    };
    let logo = c.height * 0.62;
    if let Some(image) = usage::logo(provider) {
        c.pen.round_rect((c.x, (c.height - logo) / 2.0), (logo, logo), logo * 0.26, ink(c.light, 0.08))?;
        let inner = logo * 0.72;
        c.pen.image(image, (c.x + (logo - inner) / 2.0, (c.height - inner) / 2.0), (inner, inner))?;
    }
    c.x += logo + c.px(10.0);
    let Some(u) = r.ia.as_ref().map(|s| s.get(provider)) else {
        return c.text("Leyendo…", 11.0, Weight::Normal, c.faded());
    };
    if u.windows.is_empty() {
        if size == Size::Compact {
            // No room to explain: the panel tells how to connect it.
            return c.text("—", 15.0, Weight::Bold, c.faded());
        }
        let text = match (u.state, provider) {
            (State::SignIn, usage::Provider::Claude) => "Abre Claude Code",
            (State::SignIn, usage::Provider::Codex) => "Conectar ChatGPT",
            (State::SignIn, _) => "Conectar Gemini",
            _ => "Sin datos aún",
        };
        return c.text(text, 11.0, Weight::Normal, c.faded());
    }
    if size == Size::Compact {
        let w = &u.windows[0];
        return c.column(
            (&format!("{}%", w.used_percent), 15.0, Weight::Bold, level(w.used_percent)),
            (&until(w.resets_at), 10.0, Weight::Normal, c.faded()),
            false,
            10,
        );
    }
    // Rows like the reference: label, bar, time to reset, percent.
    let rows: Vec<_> = u.windows.iter().take(2).collect();
    let row_h = c.line(11.0, Weight::Bold)?;
    let top = (c.height - row_h * rows.len() as f32 - c.px(4.0)) / 2.0;
    let (label_w, _) = c.gfx.measure("Semanal", 11.0, c.unit, Weight::Bold)?;
    let (reset_w, _) = c.gfx.measure("10h 59m", 10.5, c.unit, Weight::Normal)?;
    let (pct_w, _) = c.gfx.measure("100%", 11.0, c.unit, Weight::Bold)?;
    let bar_x = c.x + label_w + c.px(8.0);
    let bar_w = c.px(56.0);
    let right = bar_x + bar_w + c.px(8.0) + reset_w + c.px(8.0) + pct_w;
    for (index, w) in rows.iter().enumerate() {
        let y = top + (row_h + c.px(4.0)) * index as f32;
        let label = match w.title().as_str() {
            "5 h" => "Sesión".to_owned(),
            "7 d" => "Semanal".to_owned(),
            other => other.to_owned(),
        };
        c.pen.text(&label, (c.x, y), 11.0, Weight::Bold, c.strong())?;
        let bar_h = c.px(4.0);
        let bar_y = y + (row_h - bar_h) / 2.0;
        c.pen.round_rect((bar_x, bar_y), (bar_w, bar_h), bar_h / 2.0, ink(c.light, 0.18))?;
        let filled = bar_w * f32::from(w.used_percent.min(100)) / 100.0;
        if filled > 0.5 {
            c.pen.round_rect((bar_x, bar_y), (filled.max(bar_h), bar_h), bar_h / 2.0, level(w.used_percent))?;
        }
        let reset = until(w.resets_at);
        let (rw, _) = c.gfx.measure(&reset, 10.5, c.unit, Weight::Normal)?;
        c.pen.text(&reset, (bar_x + bar_w + c.px(8.0) + reset_w - rw, y), 10.5, Weight::Normal, c.faded())?;
        let pct = format!("{}%", w.used_percent);
        let (tw, _) = c.gfx.measure(&pct, 11.0, c.unit, Weight::Bold)?;
        c.pen.text(&pct, (right - tw, y), 11.0, Weight::Bold, c.strong())?;
    }
    c.x = right;
    Ok(())
}

const WHITE: Rgba = [1.0, 1.0, 1.0, 1.0];
const WHITE_DIM: Rgba = [1.0, 1.0, 1.0, 0.7];
const WHITE_OFF: Rgba = [1.0, 1.0, 1.0, 0.4];

/// `text` cut with an ellipsis so it is at most `max` pixels wide.
fn fit(c: &Cell<'_, '_>, text: &str, size: f32, weight: Weight, max: f32) -> Result<String> {
    if text.is_empty() || c.gfx.measure(text, size, c.unit, weight)?.0 <= max {
        return Ok(text.to_owned());
    }
    let chars: Vec<char> = text.chars().collect();
    // The most characters that still fit with the ellipsis.
    let (mut low, mut high) = (0, chars.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        let cut: String = chars[..mid].iter().chain(&['…']).collect();
        if c.gfx.measure(&cut, size, c.unit, weight)?.0 <= max {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    Ok(chars[..low].iter().chain(&['…']).collect())
}

/// "3:07", or "1:02:03" from an hour on.
fn track_time(seconds: u32) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Two round buttons in a pill: the microphone (a click mutes it) and the
/// headphones (a click opens the panel, the cell's default). Wide also names
/// the current output.
fn audio(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    let left = c.x;
    // The width is fixed per size (`width`), whatever the text says.
    let total = c.height * f32::from(width(WidgetId::Audio, size)) / 100.0;
    let d = r.audio.as_ref();
    let muted = d.and_then(|d| d.mic_muted) == Some(true);
    let circle = (c.height * 0.7).round();
    let inset = ((c.height * 0.84).round() - circle) / 2.0;
    let gap = c.px(10.0);
    let both = size != Size::Compact;
    let pill_w = if both { circle * 2.0 + gap + inset * 2.0 } else { circle + inset * 2.0 };
    let pill_h = circle + inset * 2.0;
    let top = ((c.height - pill_h) / 2.0).round();
    c.pen.round_rect((left, top), (pill_w, pill_h), pill_h * 0.3, ink(c.light, 0.10))?;

    let glyph = 16.0;
    let circle_top = top + inset;
    let mic_x = left + inset;
    let back = if muted { [0.85, 0.2, 0.2, 1.0] } else { ink(c.light, 0.14) };
    c.pen.round_rect((mic_x, circle_top), (circle, circle), circle / 2.0, back)?;
    let (mic, color) = if muted { ('\u{F781}', WHITE) } else { ('\u{E720}', c.strong()) };
    let (gw, gh) = c.gfx.measure_icon(mic, glyph, c.unit)?;
    c.pen.icon(mic, (mic_x + (circle - gw) / 2.0, circle_top + (circle - gh) / 2.0), glyph, color)?;
    c.zones.push((mic_x, mic_x + circle, CellAction::ToggleMic));

    if both {
        let phones_x = mic_x + circle + gap;
        c.pen.round_rect((phones_x, circle_top), (circle, circle), circle / 2.0, ink(c.light, 0.14))?;
        let (gw, gh) = c.gfx.measure_icon('\u{E7F6}', glyph, c.unit)?;
        c.pen.icon(
            '\u{E7F6}',
            (phones_x + (circle - gw) / 2.0, circle_top + (circle - gh) / 2.0),
            glyph,
            c.strong(),
        )?;
    }

    if size == Size::Wide {
        let text_x = left + pill_w + c.px(10.0);
        let room = (total - left - text_x).max(0.0);
        let name = d.map_or("", |d| d.output.as_str());
        let name = fit(c, name, 11.0, Weight::Normal, room)?;
        let (_, th) = c.gfx.measure("Ág", 11.0, c.unit, Weight::Normal)?;
        c.pen.text(&name, (text_x, (c.height - th) / 2.0), 11.0, Weight::Normal, c.faded())?;
    }
    // The cell's width is the one asked for; just under it so it does not round up.
    c.x = total - left - 0.5;
    Ok(())
}

/// Whether the title or artist overflowed (and scrolls) in the last music render.
static MARQUEE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether the marquee may animate in the cells rendered now.
static MARQUEE_ON: AtomicBool = AtomicBool::new(false);

/// Only the islands animate the marquee; elsewhere long text is cut with an
/// ellipsis.
pub fn set_marquee(on: bool) {
    MARQUEE_ON.store(on, Relaxed);
}

/// The track the marquee is timing (title + artist) and when it started.
static MARQUEE_START: Mutex<Option<(String, std::time::Instant)>> = Mutex::new(None);

/// Whether the music cell needs to be repainted continuously to scroll.
pub fn marquee_active() -> bool {
    MARQUEE_ACTIVE.load(Relaxed)
}

/// Restarts the marquee clock when the track (title + artist) changes.
fn track_clock(title: &str, artist: &str) {
    let key = format!("{title}\u{0}{artist}");
    if let Ok(mut start) = MARQUEE_START.lock()
        && start.as_ref().is_none_or(|(known, _)| *known != key)
    {
        *start = Some((key, std::time::Instant::now()));
    }
}

/// Draws one line of text in `room` pixels. If it fits it is drawn as is;
/// if not, it scrolls (two copies, clipped to the room, with a pause at each
/// start) so it can be read. Returns the width used: the text's, or `room`
/// when it scrolls.
fn marquee(
    c: &Cell<'_, '_>,
    text: &str,
    (x, y): (f32, f32),
    size: f32,
    weight: Weight,
    color: Rgba,
    room: f32,
) -> Result<f32> {
    let (text_w, _) = c.gfx.measure(text, size, c.unit, weight)?;
    if text_w <= room {
        return c.pen.text(text, (x, y), size, weight, color);
    }
    if !MARQUEE_ON.load(Relaxed) {
        let cut = fit(c, text, size, weight, room)?;
        return c.pen.text(&cut, (x, y), size, weight, color);
    }
    MARQUEE_ACTIVE.store(true, Relaxed);
    const PAUSE: f32 = 1.8;
    let gap = c.px(32.0);
    let speed = c.px(28.0);
    let elapsed = MARQUEE_START
        .lock()
        .ok()
        .and_then(|s| s.as_ref().map(|(_, t)| t.elapsed().as_secs_f32()))
        .unwrap_or(0.0);
    let cycle = text_w + gap;
    let period = PAUSE + cycle / speed;
    let phase = elapsed % period;
    let offset = ((phase - PAUSE).max(0.0) * speed).min(cycle);
    let clip = (x, x + room);
    c.pen.text_clipped(text, (x - offset, y), size, weight, color, clip)?;
    c.pen.text_clipped(text, (x - offset + cycle, y), size, weight, color, clip)?;
    Ok(room)
}

/// Three static white bars (an "it is playing" equalizer), bottom aligned at
/// `bottom`, starting at `x`. Returns its total width.
fn equalizer(c: &Cell<'_, '_>, x: f32, bottom: f32) -> Result<f32> {
    let bar_w = c.px(2.5).max(2.0);
    let gap = c.px(1.5).max(1.0);
    let full = c.px(11.0);
    for (index, factor) in [0.5_f32, 1.0, 0.7].into_iter().enumerate() {
        let h = full * factor;
        let bx = x + (bar_w + gap) * index as f32;
        c.pen.round_rect((bx, bottom - h), (bar_w, h), bar_w / 2.0, [1.0, 1.0, 1.0, 0.95])?;
    }
    Ok(bar_w * 3.0 + gap * 2.0)
}

/// "Now playing", iOS style, on a card of the artwork's colour: the artwork
/// on the left, then title and artist with a thin progress bar, and on the
/// right the previous / play-pause (white circle) / next buttons. Compact
/// shows only the artwork (with a small equalizer while it plays) and the
/// play button. Medium adds the text and the progress bar. Wide also puts an
/// equalizer by the title and elapsed / remaining times around the bar.
fn musica(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    if MARQUEE_ON.load(Relaxed) {
        MARQUEE_ACTIVE.store(false, Relaxed);
    }
    let left = c.x;
    // The width is fixed per size (`width`), whatever the text says.
    let total = c.height * f32::from(width(WidgetId::Musica, size)) / 100.0;
    let right = total - left;
    let d = r.musica.as_ref();
    let colors = d.map_or([NO_ARTWORK, NO_ARTWORK_2], |d| d.colors);
    c.pen.gradient_rect(
        (0.0, 0.0),
        (total, c.height),
        c.height * 0.22,
        (0.0, 0.0),
        (total, c.height),
        &[(0.0, colors[0]), (1.0, colors[1])],
    )?;
    let compact = size == Size::Compact;
    let wide = size == Size::Wide;
    let playing = d.is_some_and(|d| d.playing);
    let side = (c.height * 0.66).round();
    let art_y = ((c.height - side) / 2.0).round();
    if let Some(art) = d.and_then(|d| d.art.as_deref()).or(media::square_fallback()) {
        c.pen.image(art, (left, art_y), (side, side))?;
    }
    if compact && playing {
        equalizer(c, left + c.px(4.0), art_y + side - c.px(4.0))?;
    }

    // Buttons, from the right edge: previous, play (white circle), next.
    let circle = c.px(26.0);
    let side_slot = c.px(18.0);
    let play_slot = circle + c.px(if compact { 4.0 } else { 6.0 });
    let all = [
        ('\u{E892}', Control::Previous, d.is_some_and(|d| d.can_prev), side_slot),
        (if playing { '\u{E769}' } else { '\u{E768}' }, Control::PlayPause, d.is_some_and(|d| d.can_toggle), play_slot),
        ('\u{E893}', Control::Next, d.is_some_and(|d| d.can_next), side_slot),
    ];
    let shown = if compact { &all[1..2] } else { &all[..] };
    let buttons_w: f32 = shown.iter().map(|b| b.3).sum();
    let buttons_x = right - buttons_w;
    let mut from = buttons_x;
    for (glyph, control, enabled, slot) in shown {
        if *control == Control::PlayPause {
            let cx = from + (slot - circle) / 2.0;
            let cy = (c.height - circle) / 2.0;
            let back = if *enabled { [1.0, 1.0, 1.0, 1.0] } else { [1.0, 1.0, 1.0, 0.35] };
            c.pen.round_rect((cx, cy), (circle, circle), circle / 2.0, back)?;
            let (gw, gh) = c.gfx.measure_icon(*glyph, 12.0, c.unit)?;
            c.pen.icon(*glyph, (cx + (circle - gw) / 2.0, cy + (circle - gh) / 2.0), 12.0, [0.11, 0.11, 0.12, 1.0])?;
        } else {
            let (gw, gh) = c.gfx.measure_icon(*glyph, 12.0, c.unit)?;
            let color = if *enabled { [1.0, 1.0, 1.0, 0.85] } else { WHITE_OFF };
            c.pen.icon(*glyph, (from + (slot - gw) / 2.0, (c.height - gh) / 2.0), 12.0, color)?;
        }
        if *enabled {
            c.zones.push((from, from + slot, CellAction::Media(*control)));
        }
        from += slot;
    }

    if !compact {
        // Title and artist between the artwork and the buttons, then the progress.
        let text_x = left + side + c.px(8.0);
        let room = (buttons_x - c.px(6.0) - text_x).max(0.0);
        let (title, artist) = match d {
            Some(d) if d.title.is_empty() => ("Sin título", d.artist.as_str()),
            Some(d) => (d.title.as_str(), d.artist.as_str()),
            None => ("Nada sonando", ""),
        };
        let (duration, position) = d.map_or((0, 0), |d| (d.duration_s, d.position_s.min(d.duration_s)));
        let eq_w = if wide && playing { c.px(2.5).max(2.0) * 3.0 + c.px(1.5).max(1.0) * 2.0 } else { 0.0 };
        // The equalizer only fits when the whole title does; otherwise it scrolls.
        let (title_w, _) = c.gfx.measure(title, 12.0, c.unit, Weight::Bold)?;
        let show_eq = eq_w > 0.0 && title_w + eq_w + c.px(6.0) <= room;
        let title_room = if show_eq { room - eq_w - c.px(6.0) } else { room };
        track_clock(title, artist);
        let (_, th) = c.gfx.measure("Ág", 12.0, c.unit, Weight::Bold)?;
        let (_, ah) = c.gfx.measure("Ág", 10.5, c.unit, Weight::Normal)?;
        // Height of the row under the artist: the thin bar, or the times row.
        let (row_gap, row_h) = if duration == 0 {
            (0.0, 0.0)
        } else if wide {
            let (_, h9) = c.gfx.measure("Ág", 9.0, c.unit, Weight::Normal)?;
            (c.px(3.0), h9.max(c.px(4.0)))
        } else {
            (c.px(4.0), c.px(3.0).max(2.0))
        };
        let y = (c.height - (th + ah + row_gap + row_h)) / 2.0;
        let drawn = marquee(c, title, (text_x, y), 12.0, Weight::Bold, WHITE, title_room)?;
        marquee(c, artist, (text_x, y + th), 10.5, Weight::Normal, WHITE_DIM, room)?;
        if show_eq {
            equalizer(c, text_x + drawn + c.px(6.0), y + th - c.px(2.0))?;
        }

        if duration > 0 {
            let row_y = y + th + ah + row_gap;
            let middle = row_y + row_h / 2.0;
            let bar_h = if wide { c.px(4.0).max(2.0) } else { row_h };
            let (mut bar_x, mut bar_end) = (text_x, text_x + room);
            if wide {
                // Elapsed on the left, remaining on the right; the widest
                // width keeps the bar from dancing as the digits change.
                let widest = if duration >= 3600 { "-8:88:88" } else { "-88:88" };
                let (slot_w, h9) = c.gfx.measure(widest, 9.0, c.unit, Weight::Normal)?;
                let elapsed = track_time(position);
                let remaining = format!("-{}", track_time(duration - position));
                let (rw, _) = c.gfx.measure(&remaining, 9.0, c.unit, Weight::Normal)?;
                let ty = middle - h9 / 2.0;
                c.pen.text(&elapsed, (bar_x, ty), 9.0, Weight::Normal, WHITE_DIM)?;
                c.pen.text(&remaining, (text_x + room - rw, ty), 9.0, Weight::Normal, WHITE_DIM)?;
                bar_x = text_x + slot_w + c.px(6.0);
                bar_end = text_x + room - slot_w - c.px(6.0);
            }
            let bar_w = (bar_end - bar_x).max(0.0);
            let bar_y = middle - bar_h / 2.0;
            c.pen.round_rect((bar_x, bar_y), (bar_w, bar_h), bar_h / 2.0, [1.0, 1.0, 1.0, 0.25])?;
            let filled = bar_w * position as f32 / duration as f32;
            if filled > 0.5 {
                c.pen.round_rect((bar_x, bar_y), (filled.max(bar_h), bar_h), bar_h / 2.0, [1.0, 1.0, 1.0, 0.9])?;
            }
        }
    }
    // The cell's width is the one asked for; just under it so it does not round up.
    c.x = total - left - 0.5;
    Ok(())
}
