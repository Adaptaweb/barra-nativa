//! Widgets in the dock, drawn like the Tauri Barra's cells: no surface of
//! their own, straight on the dock glass, as tall as an icon inside the dock
//! (taller in the islands) and as wide as their size (compact, medium, wide)
//! asks. Icon, big figures, faded
//! details, thin separators.

use super::draw::{BLUE, GREEN, ORANGE, RED, ink, symbol};
use super::clima::describe;
use super::musica::NO_ARTWORK;
use super::{Calendar, ClimaData, Home, Readings, WidgetId};
use crate::gfx::{Gfx, Pen, Rgba, Weight};
use crate::media::{self, Control};
use crate::usage::{self, State};
use anyhow::Result;
use std::sync::Mutex;
use chrono::{DateTime, Local, TimeZone, Utc};
use windows::UI::Composition::CompositionSurfaceBrush;

const ACCENT_RED: Rgba = [1.0, 0.271, 0.227, 1.0];
const HL_ORANGE: Rgba = [1.0, 0.62, 0.2, 1.0];
const HOME_YELLOW: Rgba = [1.0, 0.82, 0.4, 1.0];
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
        (WidgetId::Casa, Size::Compact) => 140,
        (WidgetId::Casa, Size::Medium | Size::Wide | Size::Estado) => 250,
        (WidgetId::Musica, Size::Compact) => 130,
        (WidgetId::Musica, Size::Medium | Size::Estado) => 330,
        (WidgetId::Musica, Size::Wide) => 420,
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
pub fn render(
    gfx: &Gfx,
    id: WidgetId,
    size: Size,
    readings: &Readings,
    height: u32,
    light: bool,
) -> Result<(CompositionSurfaceBrush, bool, Zones)> {
    let size = size.for_widget(id);
    let height = height.max(16) as f32;
    // Text sizes are relative to a 48 pixel tall cell, as in the Barra dock.
    let unit = height / 48.0;
    let pad = (height * 0.14).round();
    let draw = |width: f32, end: &mut f32, zones: &mut Zones| {
        gfx.paint((width, height), unit, |pen| {
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
    // First on a roomy surface to learn the width, then at that width.
    let mut end = 0.0;
    draw(height * 9.0, &mut end, &mut Vec::new())?;
    // In steps of a tenth of an icon, so small changes do not move the dock.
    let hundredths = ((((end + pad) / height) * 10.0).ceil() * 10.0).clamp(60.0, 900.0) as u16;
    let width = (height * f32::from(hundredths) / 100.0).round();
    let mut zones = Vec::new();
    let cell = draw(width, &mut end, &mut zones)?;
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

fn bandeja(c: &mut Cell<'_, '_>, _r: &Readings) -> Result<()> {
    // Only the chevron: the icons live in its bubble, not loose in the dock.
    let size = 15.0;
    let (w, h) = c.gfx.measure_icon('\u{E70E}', size, c.unit)?;
    c.x += c.px(4.0);
    c.pen.icon('\u{E70E}', (c.x, (c.height - h) / 2.0), size, c.strong())?;
    c.x += w + c.px(4.0);
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

/// Network, volume and battery, as the Windows quick settings button.
fn control(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    let icon = 17.0;
    let gap = c.px(9.0);
    c.x += c.px(4.0);
    let Some(d) = r.control else {
        let (_, h) = c.gfx.measure_icon('\u{E713}', icon, c.unit)?;
        c.x += c.pen.icon('\u{E713}', (c.x, (c.height - h) / 2.0), icon, c.strong())?;
        c.x += c.px(4.0);
        return Ok(());
    };
    let mut glyphs = vec![super::control::network_glyph(d.network), super::control::volume_glyph(d.volume)];
    if d.bluetooth && size != Size::Compact {
        glyphs.push('\u{E702}');
    }
    for (index, glyph) in glyphs.into_iter().enumerate() {
        if index > 0 {
            c.x += gap;
        }
        let (_, h) = c.gfx.measure_icon(glyph, icon, c.unit)?;
        c.x += c.pen.icon(glyph, (c.x, (c.height - h) / 2.0), icon, c.strong())?;
    }
    if let Some(battery) = d.battery {
        c.x += gap;
        let glyph = super::control::battery_glyph(battery);
        let (_, h) = c.gfx.measure_icon(glyph, icon, c.unit)?;
        let color = if battery.charging {
            GREEN
        } else if battery.percent <= 20 {
            RED
        } else {
            c.strong()
        };
        c.x += c.pen.icon(glyph, (c.x, (c.height - h) / 2.0), icon, color)?;
        if size != Size::Compact {
            c.x += c.px(4.0);
            c.text(&format!("{}%", battery.percent), 12.0, Weight::Bold, c.strong())?;
        }
    }
    c.x += c.px(4.0);
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

    fn emoji(&mut self, text: &str, size: f32) -> Result<()> {
        let (w, h) = self.gfx.measure_emoji(text, size, self.unit)?;
        self.pen.emoji(text, (self.x, (self.height - h) / 2.0), size)?;
        self.x += w;
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
        (&date, 17.0, Weight::Bold, c.strong()),
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

fn clima(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    let Some(d) = r.clima.as_ref() else {
        return c.text("Clima…", 11.0, Weight::Normal, c.faded());
    };
    let temp = format!("{}°", d.temp);
    match size {
        Size::Compact => {
            c.emoji(symbol(d.code, d.day), 26.0)?;
            c.x += c.px(6.0);
            let range = format!("H {}° L {}°", d.max, d.min);
            c.column(
                (&temp, 22.0, Weight::Bold, c.strong()),
                (&range, 11.0, Weight::Normal, c.faded()),
                false,
                20,
            )
        }
        Size::Medium => {
            c.emoji(symbol(d.code, d.day), 22.0)?;
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
            c.emoji(symbol(d.code, d.day), 22.0)?;
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
                let (ew, eh) = c.gfx.measure_emoji(symbol(*code, true), 13.0, c.unit)?;
                let (hw, th) = c.gfx.measure(&format!("{high}°"), 11.0, c.unit, Weight::Bold)?;
                let low = format!(" {low}°");
                let (lw, _) = c.gfx.measure(&low, 11.0, c.unit, Weight::Normal)?;
                let w = nw.max(ew).max(hw + lw);
                let y = (c.height - nh - eh - th) / 2.0;
                c.pen.text(name, (c.x + (w - nw) / 2.0, y), 10.0, Weight::Normal, c.faded())?;
                c.pen.emoji(symbol(*code, true), (c.x + (w - ew) / 2.0, y + nh), 13.0)?;
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
    c.x += c.px(4.0);
    c.emoji(symbol(d.code, d.day), 24.0)
}

/// Ring colour of the CPU temperature: green, yellow from 65, red from 80.
fn temp_color(t: u8) -> Rgba {
    match t {
        80.. => [1.0, 0.27, 0.23, 1.0],
        65.. => [1.0, 0.80, 0.0, 1.0],
        _ => [0.20, 0.78, 0.35, 1.0],
    }
}

fn sistema(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    let Some(d) = r.sistema else {
        return c.text("Sistema…", 11.0, Weight::Normal, c.faded());
    };
    // The gauge: a 270° ring that always shows the CPU temperature.
    let ring = c.height * 0.8;
    let left = if size == Size::Compact { c.x } else { c.x - c.px(4.0) };
    let center = (left + ring / 2.0, c.height / 2.0);
    let stroke = c.px(4.5);
    let radius = ring / 2.0 - stroke;
    c.pen.arc(center, radius, stroke, (225.0, 270.0), ink(c.light, 0.22))?;
    let number = match d.temp {
        Some(t) => {
            let fraction = (f32::from(t) / 100.0).clamp(0.0, 1.0);
            c.pen.arc(center, radius, stroke, (225.0, 270.0 * fraction), temp_color(t))?;
            format!("{t}°")
        }
        None => "—".to_owned(),
    };
    let (nw, nh) = c.gfx.measure(&number, 13.0, c.unit, Weight::Bold)?;
    c.pen.text(&number, (center.0 - nw / 2.0, center.1 - nh / 2.0 - c.px(3.0)), 13.0, Weight::Bold, c.strong())?;
    let tag = if d.temp.is_some() { "CPU" } else { "sin sensor" };
    let (lw, _) = c.gfx.measure(tag, 7.5, c.unit, Weight::Bold)?;
    c.pen.text(tag, (center.0 - lw / 2.0, center.1 + radius - c.px(6.0)), 7.5, Weight::Bold, c.faded())?;
    c.x = left + ring;
    if size == Size::Compact {
        return Ok(());
    }
    c.x += c.px(10.0);
    // CPU and RAM rows: label, bar, percent.
    let rows = [("CPU", d.cpu), ("RAM", d.ram)];
    let row_h = c.line(15.0, Weight::Bold)?;
    let top = (c.height - row_h * 2.0 - c.px(4.0)) / 2.0;
    let (label_w, label_h) = c.gfx.measure("RAM", 11.0, c.unit, Weight::Bold)?;
    let (pct_w, _) = c.gfx.measure("100%", 15.0, c.unit, Weight::Bold)?;
    let bar_x = c.x + label_w + c.px(8.0);
    let bar_w = c.px(44.0);
    let pct_x = bar_x + bar_w + c.px(8.0);
    for (index, (label, pct)) in rows.into_iter().enumerate() {
        let y = top + (row_h + c.px(4.0)) * index as f32;
        c.pen.text(label, (c.x, y + (row_h - label_h) / 2.0), 11.0, Weight::Bold, c.faded())?;
        let bar_h = c.px(5.0);
        let bar_y = y + (row_h - bar_h) / 2.0;
        c.pen.round_rect((bar_x, bar_y), (bar_w, bar_h), bar_h / 2.0, ink(c.light, 0.15))?;
        let filled = bar_w * f32::from(pct.min(100)) / 100.0;
        if filled > 0.5 {
            c.pen.round_rect((bar_x, bar_y), (filled.max(bar_h), bar_h), bar_h / 2.0, BLUE)?;
        }
        let text = format!("{pct}%");
        let (tw, _) = c.gfx.measure(&text, 15.0, c.unit, Weight::Bold)?;
        c.pen.text(&text, (pct_x + pct_w - tw, y), 15.0, Weight::Bold, c.strong())?;
    }
    c.x = pct_x + pct_w;
    Ok(())
}

fn casa(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    c.emoji("🏠", 20.0)?;
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

/// "Sonando ahora": the artwork, what plays, the progress and the transport
/// buttons, on a card of the artwork's colour.
fn musica(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    let left = c.x;
    // The width is fixed per size (`width`), whatever the text says.
    let total = c.height * f32::from(width(WidgetId::Musica, size)) / 100.0;
    let right = total - left;
    let d = r.musica.as_ref();
    c.pen.round_rect(
        (0.0, 0.0),
        (total, c.height),
        c.height * 0.22,
        d.map_or(NO_ARTWORK, |d| d.color),
    )?;
    let compact = size == Size::Compact;
    let wide = size == Size::Wide;
    // The artwork, the text and the buttons use this much height; the
    // progress row takes the rest.
    let top_h = if wide { c.height * 0.70 } else { c.height };
    let side = (c.height * 0.62).round();
    if let Some(art) = d.and_then(|d| d.art.as_deref()).or(media::square_fallback()) {
        c.pen.image(art, (left, ((top_h - side) / 2.0).round()), (side, side))?;
    }

    // Buttons, from the right edge.
    let playing = d.is_some_and(|d| d.playing);
    let all = [
        ('\u{E892}', Control::Previous, d.is_some_and(|d| d.can_prev)),
        (if playing { '\u{E769}' } else { '\u{E768}' }, Control::PlayPause, d.is_some_and(|d| d.can_toggle)),
        ('\u{E893}', Control::Next, d.is_some_and(|d| d.can_next)),
    ];
    let shown = if compact { &all[1..2] } else { &all[..] };
    let slot = c.px(if compact { 22.0 } else { 19.0 });
    let buttons_x = right - slot * shown.len() as f32;
    for (index, (glyph, control, enabled)) in shown.iter().enumerate() {
        let from = buttons_x + slot * index as f32;
        let (gw, gh) = c.gfx.measure_icon(*glyph, 14.0, c.unit)?;
        let color = if *enabled { WHITE } else { WHITE_OFF };
        c.pen.icon(*glyph, (from + (slot - gw) / 2.0, (top_h - gh) / 2.0), 14.0, color)?;
        if *enabled {
            c.zones.push((from, from + slot, CellAction::Media(*control)));
        }
    }

    if !compact {
        // Title and artist between the artwork and the buttons.
        let text_x = left + side + c.px(8.0);
        let room = (buttons_x - c.px(6.0) - text_x).max(0.0);
        let (title, artist) = match d {
            Some(d) if d.title.is_empty() => ("Sin título", d.artist.as_str()),
            Some(d) => (d.title.as_str(), d.artist.as_str()),
            None => ("Nada sonando", ""),
        };
        let title = fit(c, title, 12.0, Weight::Bold, room)?;
        let artist = fit(c, artist, 10.5, Weight::Normal, room)?;
        let (_, th) = c.gfx.measure("Ág", 12.0, c.unit, Weight::Bold)?;
        let (_, ah) = c.gfx.measure("Ág", 10.5, c.unit, Weight::Normal)?;
        let y = (top_h - th - ah) / 2.0;
        c.pen.text(&title, (text_x, y), 12.0, Weight::Bold, WHITE)?;
        c.pen.text(&artist, (text_x, y + th), 10.5, Weight::Normal, WHITE_DIM)?;
    }

    if wide {
        // Elapsed time, the bar and the time left, under everything.
        let (duration, position) = d.map_or((0, 0), |d| (d.duration_s, d.position_s.min(d.duration_s)));
        let bar_h = c.px(3.0).max(2.0);
        let middle = top_h + (c.height - top_h) / 2.0;
        let (mut bar_x, mut bar_end) = (left, right);
        if duration > 0 {
            let widest = if duration >= 3600 { "-8:88:88" } else { "-88:88" };
            let (slot_w, th) = c.gfx.measure(widest, 9.0, c.unit, Weight::Normal)?;
            let elapsed = track_time(position);
            let remaining = format!("-{}", track_time(duration - position));
            let (rw, _) = c.gfx.measure(&remaining, 9.0, c.unit, Weight::Normal)?;
            let y = middle - th / 2.0;
            c.pen.text(&elapsed, (left, y), 9.0, Weight::Normal, WHITE_DIM)?;
            c.pen.text(&remaining, (right - rw, y), 9.0, Weight::Normal, WHITE_DIM)?;
            bar_x = left + slot_w + c.px(6.0);
            bar_end = right - slot_w - c.px(6.0);
        }
        let bar_w = (bar_end - bar_x).max(0.0);
        let bar_y = middle - bar_h / 2.0;
        c.pen.round_rect((bar_x, bar_y), (bar_w, bar_h), bar_h / 2.0, [1.0, 1.0, 1.0, 0.25])?;
        let filled = if duration > 0 { bar_w * position as f32 / duration as f32 } else { 0.0 };
        if filled > 0.5 {
            c.pen.round_rect((bar_x, bar_y), (filled.max(bar_h), bar_h), bar_h / 2.0, [1.0, 1.0, 1.0, 0.9])?;
        }
    }
    // The cell's width is the one asked for; just under it so it does not round up.
    c.x = total - left - 0.5;
    Ok(())
}
