//! Widgets in the dock, drawn like the Tauri Barra's cells: no surface of
//! their own, straight on the dock glass, as tall as an icon and as wide as
//! their size (compact, medium, wide) asks. Icon, big figures, faded
//! details, thin separators.

use super::draw::{BLUE, GREEN, ORANGE, RED, ink, symbol};
use super::{Calendar, Home, Readings, WidgetId};
use crate::gfx::{Gfx, Pen, Rgba, Weight};
use crate::usage::{self, State};
use anyhow::Result;
use std::sync::Mutex;
use chrono::{DateTime, Local, TimeZone, Utc};
use windows::UI::Composition::CompositionSurfaceBrush;

const ACCENT_RED: Rgba = [1.0, 0.271, 0.227, 1.0];
const HOME_YELLOW: Rgba = [1.0, 0.82, 0.4, 1.0];
const CALENDAR_COLORS: [Rgba; 4] = [
    [0.039, 0.518, 1.0, 1.0],
    [0.204, 0.78, 0.349, 1.0],
    [1.0, 0.624, 0.039, 1.0],
    [0.749, 0.353, 0.949, 1.0],
];

/// The size the user chose for a widget in the dock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    Compact,
    Medium,
    Wide,
}

impl Size {
    pub fn from_key(key: Option<&str>) -> Self {
        match key {
            Some("compacto") => Self::Compact,
            Some("ancho") => Self::Wide,
            _ => Self::Medium,
        }
    }
}

/// Width of the cell in hundredths of an icon. Fixed per widget and size so a
/// changing reading never moves the dock.
pub fn width(id: WidgetId, size: Size) -> u16 {
    if let Some(known) = WIDTHS
        .lock()
        .ok()
        .and_then(|w| w.iter().find(|(key, _)| *key == (id, size)).map(|(_, v)| *v))
    {
        return known;
    }
    match (id, size) {
        (WidgetId::Bandeja, _) => 70,
        (WidgetId::Control, Size::Compact) => 120,
        (WidgetId::Control, _) => 200,
        (WidgetId::Reloj, Size::Compact) => 135,
        (WidgetId::Reloj, Size::Medium) => 290,
        (WidgetId::Reloj, Size::Wide) => 600,
        (WidgetId::Clima, Size::Compact) => 160,
        (WidgetId::Clima, Size::Medium) => 300,
        (WidgetId::Clima, Size::Wide) => 480,
        (WidgetId::Sistema, Size::Compact) => 100,
        (WidgetId::Sistema, Size::Medium | Size::Wide) => 340,
        (WidgetId::Casa, Size::Compact) => 140,
        (WidgetId::Casa, Size::Medium | Size::Wide) => 250,
        (WidgetId::Claude | WidgetId::ChatGpt | WidgetId::Gemini, Size::Compact) => 170,
        (WidgetId::Claude | WidgetId::ChatGpt | WidgetId::Gemini, _) => 500,
    }
}

static WIDTHS: Mutex<Vec<((WidgetId, Size), u16)>> = Mutex::new(Vec::new());

/// Draws the cell `height` pixels tall, as wide as its content. The width it
/// needed is remembered for the dock layout (see `width`); returns whether it
/// changed.
pub fn render(
    gfx: &Gfx,
    id: WidgetId,
    size: Size,
    readings: &Readings,
    height: u32,
    light: bool,
) -> Result<(CompositionSurfaceBrush, bool)> {
    let height = height.max(16) as f32;
    // Text sizes are relative to a 48 pixel tall cell, as in the Barra dock.
    let unit = height / 48.0;
    let pad = (height * 0.14).round();
    let draw = |width: f32, end: &mut f32| {
        gfx.paint((width, height), unit, |pen| {
            let mut c = Cell {
                pen,
                gfx,
                unit,
                height,
                light,
                x: pad,
            };
            match id {
                WidgetId::Reloj => reloj(&mut c, size, readings),
                WidgetId::Clima => clima(&mut c, size, readings),
                WidgetId::Sistema => sistema(&mut c, size, readings),
                WidgetId::Casa => casa(&mut c, size, readings),
                WidgetId::Bandeja => bandeja(&mut c, readings),
                WidgetId::Control => control(&mut c, size, readings),
                WidgetId::Claude | WidgetId::ChatGpt | WidgetId::Gemini => {
                    ia(&mut c, id, size, readings)
                }
            }?;
            *end = c.x;
            Ok(())
        })
    };
    // First on a roomy surface to learn the width, then at that width.
    let mut end = 0.0;
    draw(height * 9.0, &mut end)?;
    // In steps of a tenth of an icon, so small changes do not move the dock.
    let hundredths = ((((end + pad) / height) * 10.0).ceil() * 10.0).clamp(60.0, 900.0) as u16;
    let width = (height * f32::from(hundredths) / 100.0).round();
    let cell = draw(width, &mut end)?;
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
    Ok((cell.brush, changed))
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
    height: f32,
    light: bool,
    x: f32,
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
    #[expect(clippy::too_many_arguments, reason = "Two styled lines")]
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
        let (mw, hb) = self.gfx.measure(&main, big, self.unit, Weight::Bold)?;
        let (sw, _) = self.gfx.measure(&secs, small, self.unit, Weight::Bold)?;
        Ok((mw + sw, hb))
    }

    /// Draws `HH:MM` big and `:SS` small and grey on the same baseline; returns the total width.
    fn clock_draw(&self, now: &DateTime<Local>, x: f32, top: f32, big: f32, small: f32) -> Result<f32> {
        use chrono::Timelike;
        /// Share of a Segoe UI line box above the baseline (ascent 2210 / (ascent + descent) 2724).
        const BASELINE: f32 = 0.81;
        let main = format!("{:02}:{:02}", now.hour(), now.minute());
        let secs = format!(":{:02}", now.second());
        let (_, hb) = self.gfx.measure(&main, big, self.unit, Weight::Bold)?;
        let (_, hs) = self.gfx.measure(&secs, small, self.unit, Weight::Bold)?;
        let mw = self.pen.text(&main, (x, top), big, Weight::Bold, self.strong())?;
        let y_small = top + (hb - hs) * BASELINE;
        let sw = self.pen.text(&secs, (x + mw, y_small), small, Weight::Bold, self.faded())?;
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
        let (tw, th) = c.clock_measure(&now, 17.0, 10.5)?;
        let (dw, dh) = c.gfx.measure(&date, 10.5, c.unit, Weight::Normal)?;
        let total = tw.max(dw);
        let x = c.x;
        let y = (c.height - th - dh) / 2.0;
        c.clock_draw(&now, x + (total - tw) / 2.0, y, 17.0, 10.5)?;
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
    let (_, hb) = c.clock_measure(&now, 17.0, 11.0)?;
    let width = c.clock_draw(&now, c.x, (c.height - hb) / 2.0, 17.0, 11.0)?;
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
    c.emoji(symbol(d.code, d.day), 17.0)?;
    c.x += c.px(8.0);
    let temp = format!("{}°", d.temp);
    if size == Size::Compact {
        return c.text(&temp, 17.0, Weight::Bold, c.strong());
    }
    let range = format!("Máx {}° · mín {}°", d.max, d.min);
    c.column(
        (&temp, 17.0, Weight::Bold, c.strong()),
        (&range, 10.5, Weight::Normal, c.faded()),
        false,
        20,
    )?;
    if size != Size::Wide {
        return Ok(());
    }
    c.separator()?;
    for (hour, code, t) in d.hours.iter().take(3) {
        // Hour, small icon, temperature, stacked.
        let (hw, hh) = c.gfx.measure(hour, 10.0, c.unit, Weight::Normal)?;
        let (ew, eh) = c.gfx.measure_emoji(symbol(*code, true), 10.5, c.unit)?;
        let text = format!("{t}°");
        let (tw, th) = c.gfx.measure(&text, 10.5, c.unit, Weight::Medium)?;
        let w = hw.max(ew).max(tw);
        let y = (c.height - hh - eh - th) / 2.0;
        c.pen.text(hour, (c.x + (w - hw) / 2.0, y), 10.0, Weight::Normal, c.faded())?;
        c.pen.emoji(symbol(*code, true), (c.x + (w - ew) / 2.0, y + hh), 10.5)?;
        c.pen.text(&text, (c.x + (w - tw) / 2.0, y + hh + eh), 10.5, Weight::Medium, c.strong())?;
        c.x += w + c.px(12.0);
    }
    Ok(())
}

fn temp_color(t: u8) -> Rgba {
    match t {
        85.. => RED,
        70.. => ORANGE,
        _ => GREEN,
    }
}

fn sistema(c: &mut Cell<'_, '_>, size: Size, r: &Readings) -> Result<()> {
    let Some(d) = r.sistema else {
        return c.text("Sistema…", 11.0, Weight::Normal, c.faded());
    };
    // The gauge: a 270° ring with the temperature, or the CPU load.
    let ring = c.height * 0.84;
    let (value, fraction, color, unit) = match d.temp {
        Some(t) => (t, f32::from(t) / 100.0, temp_color(t), "°"),
        None => (d.cpu, f32::from(d.cpu) / 100.0, BLUE, "%"),
    };
    let left = if size == Size::Compact { c.x } else { c.x - c.px(4.0) };
    let center = (left + ring / 2.0, c.height / 2.0);
    let stroke = c.px(4.5);
    let radius = ring / 2.0 - stroke;
    c.pen.arc(center, radius, stroke, (225.0, 270.0), ink(c.light, 0.22))?;
    c.pen.arc(center, radius, stroke, (225.0, 270.0 * fraction.clamp(0.0, 1.0)), color)?;
    let number = format!("{value}{unit}");
    let (nw, nh) = c.gfx.measure(&number, 13.0, c.unit, Weight::Bold)?;
    c.pen.text(&number, (center.0 - nw / 2.0, center.1 - nh / 2.0 - c.px(3.0)), 13.0, Weight::Bold, c.strong())?;
    let (lw, _) = c.gfx.measure("CPU", 7.5, c.unit, Weight::Bold)?;
    c.pen.text("CPU", (center.0 - lw / 2.0, center.1 + radius - c.px(6.0)), 7.5, Weight::Bold, c.faded())?;
    c.x = left + ring;
    if size == Size::Compact {
        return Ok(());
    }
    c.x += c.px(10.0);
    // CPU and RAM rows: label, bar, percent.
    let rows = [("CPU", d.cpu, BLUE), ("RAM", d.ram, GREEN)];
    let row_h = c.line(11.0, Weight::Bold)?;
    let top = (c.height - row_h * 2.0 - c.px(4.0)) / 2.0;
    let (label_w, _) = c.gfx.measure("RAM", 11.0, c.unit, Weight::Bold)?;
    let (pct_w, _) = c.gfx.measure("100%", 11.0, c.unit, Weight::Bold)?;
    let bar_x = c.x + label_w + c.px(8.0);
    let bar_w = c.px(52.0);
    let pct_x = bar_x + bar_w + c.px(8.0);
    for (index, (label, pct, color)) in rows.into_iter().enumerate() {
        let y = top + (row_h + c.px(4.0)) * index as f32;
        c.pen.text(label, (c.x, y), 11.0, Weight::Bold, c.faded())?;
        let bar_h = c.px(4.0);
        let bar_y = y + (row_h - bar_h) / 2.0;
        c.pen.round_rect((bar_x, bar_y), (bar_w, bar_h), bar_h / 2.0, ink(c.light, 0.18))?;
        let filled = bar_w * f32::from(pct.min(100)) / 100.0;
        if filled > 0.5 {
            c.pen.round_rect((bar_x, bar_y), (filled.max(bar_h), bar_h), bar_h / 2.0, color)?;
        }
        let text = format!("{pct}%");
        let (tw, _) = c.gfx.measure(&text, 11.0, c.unit, Weight::Bold)?;
        c.pen.text(&text, (pct_x + pct_w - tw, y), 11.0, Weight::Bold, c.strong())?;
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
