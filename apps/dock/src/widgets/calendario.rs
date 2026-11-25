//! The month calendar the clock widget's panel shows: a header with the month
//! and its buttons, a legend, a grid with one cell per day, and a card on the
//! right listing the selected day. Everything is drawn from the readings and a
//! `View` (month shown, day selected) kept by the panel; nothing is stored here.

use super::{
    AgendaEvent, Calendar, Readings,
    panel::{ACCENT, Hit, PanelAction, ink},
};
use crate::gfx::{Gfx, Pen, Rgba, Weight};
use crate::usage::Provider;
use anyhow::Result;
use chrono::{DateTime, Datelike, Local, Months, NaiveDate, TimeZone};
use std::collections::BTreeMap;

/// Logical width of the calendar panel.
pub(super) const WIDTH: f32 = 840.0;

const PAD: f32 = 20.0;
const HEADER: f32 = 36.0;
const LEGEND: f32 = 18.0;
const WEEKDAYS: f32 = 28.0;
const CELL: f32 = 64.0;
const GAP: f32 = 8.0;
const GRID_W: f32 = CELL * 7.0 + GAP * 6.0;
const ROW: f32 = 48.0;

const PURPLE: Rgba = [0.62, 0.45, 0.98, 1.0];
const CYAN: Rgba = [0.35, 0.80, 0.98, 1.0];
const WHITE: Rgba = [1.0; 4];

const MONTHS: [&str; 12] = [
    "enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre",
    "octubre", "noviembre", "diciembre",
];
const MONTHS_SHORT: [&str; 12] = [
    "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic",
];
const DAYS_SHORT: [&str; 7] = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];
const DAYS_HEAD: [&str; 7] = ["LUN", "MAR", "MIÉ", "JUE", "VIE", "SÁB", "DOM"];

/// What the calendar shows: the month on screen and the day selected.
pub(super) struct View {
    /// First day of the month shown.
    pub month: NaiveDate,
    pub selected: NaiveDate,
}

impl View {
    pub fn today() -> Self {
        let today = Local::now().date_naive();
        Self { month: first_of(today), selected: today }
    }

    /// Moves `n` months; the selection goes to today in the current month,
    /// to the 1st in any other.
    pub fn shift(&mut self, n: i32) {
        let moved = if n >= 0 {
            self.month.checked_add_months(Months::new(n.unsigned_abs()))
        } else {
            self.month.checked_sub_months(Months::new(n.unsigned_abs()))
        };
        if let Some(month) = moved {
            self.month = month;
            let today = Local::now().date_naive();
            self.selected = if first_of(today) == month { today } else { month };
        }
    }

    /// Selects a day, switching to its month if needed.
    pub fn select(&mut self, day: NaiveDate) {
        self.selected = day;
        self.month = first_of(day);
    }
}

fn first_of(day: NaiveDate) -> NaiveDate {
    day.with_day(1).unwrap_or(day)
}

fn days_in_month(month: NaiveDate) -> u32 {
    month
        .checked_add_months(Months::new(1))
        .map_or(30, |next| next.signed_duration_since(month).num_days() as u32)
}

/// Rows (Monday to Sunday) the month needs, from 4 to 6.
fn weeks(month: NaiveDate) -> u32 {
    let offset = month.weekday().num_days_from_monday();
    (offset + days_in_month(month)).div_ceil(7).clamp(4, 6)
}

/// Logical height of the panel.
pub(super) fn height(view: &View) -> f32 {
    let weeks = weeks(view.month) as f32;
    PAD + HEADER + 12.0 + LEGEND + 14.0 + WEEKDAYS + 8.0 + weeks * (CELL + GAP) - GAP + PAD
}

/// A limit reset that falls on a day.
struct Reset {
    name: &'static str,
    at: DateTime<Local>,
}

#[derive(Default)]
struct Day<'a> {
    events: Vec<&'a AgendaEvent>,
    resets: Vec<Reset>,
}

fn local(ts: i64) -> Option<DateTime<Local>> {
    Local.timestamp_opt(ts, 0).single()
}

/// Puts every event and limit reset on the local days it touches.
fn collect(readings: &Readings) -> BTreeMap<NaiveDate, Day<'_>> {
    let mut days: BTreeMap<NaiveDate, Day<'_>> = BTreeMap::new();
    if let Some(agenda) = &readings.agenda {
        for event in &agenda.events {
            let Some(start) = local(event.start) else { continue };
            let first = start.date_naive();
            let last = if event.all_day && event.end > event.start {
                local(event.end - 1).map_or(first, |end| end.date_naive())
            } else {
                first
            };
            let mut day = first;
            // Bounded, so a broken feed cannot spin here.
            for _ in 0..62 {
                if day > last {
                    break;
                }
                days.entry(day).or_default().events.push(event);
                let Some(next) = day.succ_opt() else { break };
                day = next;
            }
        }
    }
    if let Some(ia) = &readings.ia {
        for (provider, name) in [
            (Provider::Claude, "Claude"),
            (Provider::Codex, "ChatGPT"),
            (Provider::Gemini, "Gemini"),
        ] {
            for window in &ia.get(provider).windows {
                let Some(at) = window.resets_at.map(|t| t.with_timezone(&Local)) else {
                    continue;
                };
                let day = days.entry(at.date_naive()).or_default();
                if !day.resets.iter().any(|r| r.name == name) {
                    day.resets.push(Reset { name, at });
                }
            }
        }
    }
    days
}

fn event_color(event: &AgendaEvent) -> Rgba {
    if event.color == 0 {
        return ACCENT;
    }
    let channel = |shift: u32| ((event.color >> shift) & 0xFF) as f32 / 255.0;
    [channel(16), channel(8), channel(0), 1.0]
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

/// Everything a drawing helper needs.
struct Ctx<'a, 'p> {
    pen: &'a Pen<'p>,
    gfx: &'a Gfx,
    scale: f32,
    dark: bool,
}

impl Ctx<'_, '_> {
    fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }

    /// Text centered in the box (`x`, `y`, `w`, `h`), in physical pixels.
    fn text_in(&self, text: &str, size: f32, weight: Weight, color: Rgba, (x, y, w, h): (f32, f32, f32, f32)) -> Result<()> {
        let (tw, th) = self.gfx.measure(text, size, self.scale, weight)?;
        self.pen.text(text, (x + (w - tw) / 2.0, y + (h - th) / 2.0), size, weight, color)?;
        Ok(())
    }

    /// Icon centered in the box.
    fn icon_in(&self, glyph: char, size: f32, color: Rgba, (x, y, w, h): (f32, f32, f32, f32)) -> Result<()> {
        let (iw, ih) = self.gfx.measure_icon(glyph, size, self.scale)?;
        self.pen.icon(glyph, (x + (w - iw) / 2.0, y + (h - ih) / 2.0), size, color)?;
        Ok(())
    }

    fn tile(&self, glyph: char, icon_size: f32, color: Rgba, (x, y): (f32, f32), side: f32, radius: f32) -> Result<()> {
        let (s, r) = (self.px(side), self.px(radius));
        self.pen.round_rect((x, y), (s, s), r, color)?;
        self.icon_in(glyph, icon_size, WHITE, (x, y, s, s))
    }

    fn dot(&self, color: Rgba, (x, y): (f32, f32), side: f32) -> Result<()> {
        let s = self.px(side);
        self.pen.round_rect((x, y), (s, s), s / 2.0, color)
    }
}

pub(super) fn draw(
    pen: &Pen<'_>,
    gfx: &Gfx,
    view: &View,
    readings: &Readings,
    dark: bool,
    scale: f32,
    hits: &mut Vec<Hit>,
) -> Result<()> {
    let cx = Ctx { pen, gfx, scale, dark };
    let days = collect(readings);
    header(&cx, view, hits)?;
    legend(&cx)?;
    let grid_top = cx.px(PAD + HEADER + 12.0 + LEGEND + 14.0);
    let weeks = weeks(view.month);
    let grid_h = cx.px(WEEKDAYS + 8.0) + weeks as f32 * cx.px(CELL + GAP) - cx.px(GAP);
    grid(&cx, view, &days, grid_top, hits)?;
    card(&cx, view, readings, &days, grid_top, grid_h, hits)
}

fn header(cx: &Ctx<'_, '_>, view: &View, hits: &mut Vec<Hit>) -> Result<()> {
    let top = cx.px(PAD);
    let title = format!(
        "{} {}",
        MONTHS[view.month.month0() as usize % 12],
        view.month.year()
    );
    let (_, th) = cx.gfx.measure(&title, 22.0, cx.scale, Weight::Bold)?;
    cx.pen.text(&title, (cx.px(PAD), top + (cx.px(HEADER) - th) / 2.0), 22.0, Weight::Bold, ink(cx.dark, 1.0))?;

    let side = cx.px(28.0);
    let box_top = top + (cx.px(HEADER) - side) / 2.0;
    let mut right = cx.px(WIDTH - PAD);
    let mut place = |width: f32, action: PanelAction| -> (f32, f32, f32, f32) {
        let left = right - width;
        hits.push(Hit { left, top: box_top, right, bottom: box_top + side, action });
        right = left - cx.px(4.0);
        (left, box_top, width, side)
    };
    for (glyph, size, action) in [
        ('\u{E711}', 12.0, PanelAction::CalClose),
        ('\u{E76C}', 12.0, PanelAction::CalShift(1)),
        ('\u{E76B}', 12.0, PanelAction::CalShift(-1)),
    ] {
        let area = place(side, action);
        cx.icon_in(glyph, size, ink(cx.dark, 0.9), area)?;
    }
    let (tw, _) = cx.gfx.measure("Hoy", 13.0, cx.scale, Weight::Medium)?;
    let area = place((tw + cx.px(16.0)).max(side), PanelAction::CalToday);
    cx.text_in("Hoy", 13.0, Weight::Medium, ACCENT, area)
}

fn legend(cx: &Ctx<'_, '_>) -> Result<()> {
    let top = cx.px(PAD + HEADER + 12.0);
    let mut x = cx.px(PAD);
    for (label, color) in [
        ("Eventos", ACCENT),
        ("Todo el día", PURPLE),
        ("Reinicio de límites", CYAN),
    ] {
        cx.dot(color, (x, top + (cx.px(LEGEND) - cx.px(8.0)) / 2.0), 8.0)?;
        x += cx.px(8.0 + 6.0);
        let (_, th) = cx.gfx.measure(label, 12.0, cx.scale, Weight::Normal)?;
        let width = cx.pen.text(label, (x, top + (cx.px(LEGEND) - th) / 2.0), 12.0, Weight::Normal, ink(cx.dark, 0.55))?;
        x += width + cx.px(18.0);
    }
    Ok(())
}

fn grid(cx: &Ctx<'_, '_>, view: &View, days: &BTreeMap<NaiveDate, Day<'_>>, top: f32, hits: &mut Vec<Hit>) -> Result<()> {
    let today = Local::now().date_naive();
    let left = cx.px(PAD);
    let step = cx.px(CELL + GAP);
    let cell = cx.px(CELL);
    let radius = cx.px(14.0);
    let back = ink(cx.dark, 0.06);

    // Weekday pills.
    let current = first_of(today) == view.month;
    for (index, name) in DAYS_HEAD.iter().enumerate() {
        let x = left + index as f32 * step;
        let h = cx.px(WEEKDAYS);
        cx.pen.round_rect((x, top), (cell, h), radius, back)?;
        let is_today = current && today.weekday().num_days_from_monday() as usize == index;
        let color = ink(cx.dark, if is_today { 1.0 } else { 0.55 });
        cx.text_in(name, 11.0, Weight::Bold, color, (x, top, cell, h))?;
    }

    // Days.
    let cells_top = top + cx.px(WEEKDAYS + 8.0);
    let offset = view.month.weekday().num_days_from_monday();
    for number in 1..=days_in_month(view.month) {
        let Some(date) = view.month.with_day(number) else { continue };
        let slot = offset + number - 1;
        let x = left + (slot % 7) as f32 * step;
        let y = cells_top + (slot / 7) as f32 * step;
        cx.pen.round_rect((x, y), (cell, cell), radius, back)?;
        if date == view.selected {
            // Four thin bars, so the cell keeps its own tint inside.
            let (t, inset) = (cx.px(2.0), cx.px(2.0));
            cx.pen.round_rect((x + inset, y), (cell - inset * 2.0, t), 0.0, ACCENT)?;
            cx.pen.round_rect((x + inset, y + cell - t), (cell - inset * 2.0, t), 0.0, ACCENT)?;
            cx.pen.round_rect((x, y + inset), (t, cell - inset * 2.0), 0.0, ACCENT)?;
            cx.pen.round_rect((x + cell - t, y + inset), (t, cell - inset * 2.0), 0.0, ACCENT)?;
        }

        let label = number.to_string();
        let (lw, lh) = cx.gfx.measure(&label, 16.0, cx.scale, Weight::Bold)?;
        let color = if date == today { ACCENT } else { ink(cx.dark, 0.9) };
        cx.pen.text(&label, (x + (cell - lw) / 2.0, y + cell - cx.px(12.0) - lh), 16.0, Weight::Bold, color)?;

        if let Some(day) = days.get(&date) {
            cell_marks(cx, day, (x, y))?;
        }
        hits.push(Hit { left: x, top: y, right: x + cell, bottom: y + cell, action: PanelAction::CalDay(date) });
    }
    Ok(())
}

/// The tile, the "+N" pill and the dots on top of a day cell.
fn cell_marks(cx: &Ctx<'_, '_>, day: &Day<'_>, (x, y): (f32, f32)) -> Result<()> {
    let cell = cx.px(CELL);
    let dot_y = y + cx.px(8.0);
    let mut dot_x = x + cell - cx.px(8.0) - cx.px(7.0);
    if let Some(first) = day.events.first() {
        // Tile (and the pill, when there is more than one event) centered on top.
        let side = cx.px(24.0);
        let more = (day.events.len() > 1).then(|| format!("+{}", day.events.len() - 1));
        let pill = match &more {
            Some(text) => {
                let (tw, th) = cx.gfx.measure(text, 10.0, cx.scale, Weight::Medium)?;
                Some((text, tw + cx.px(8.0), th))
            }
            None => None,
        };
        let total = side + pill.map_or(0.0, |(_, w, _)| cx.px(3.0) + w);
        let tx = x + (cell - total) / 2.0;
        let ty = y + cx.px(12.0);
        cx.tile('\u{E787}', 13.0, event_color(first), (tx, ty), 24.0, 7.0)?;
        if let Some((text, width, th)) = pill {
            let ph = th.max(cx.px(14.0));
            let px_left = tx + side + cx.px(3.0);
            let py = ty + (side - ph) / 2.0;
            cx.pen.round_rect((px_left, py), (width, ph), ph / 2.0, ink(cx.dark, 0.12))?;
            cx.text_in(text, 10.0, Weight::Medium, ink(cx.dark, 0.9), (px_left, py, width, ph))?;
        }
        let color = if day.events.iter().any(|e| e.all_day) { PURPLE } else { ACCENT };
        cx.dot(color, (dot_x, dot_y), 7.0)?;
        dot_x -= cx.px(7.0 + 3.0);
    }
    if !day.resets.is_empty() {
        cx.dot(CYAN, (dot_x, dot_y), 7.0)?;
    }
    Ok(())
}

/// One line of the day card.
struct Line {
    /// All-day entries first, then by time.
    order: (u8, i64),
    time: String,
    time_size: f32,
    color: Rgba,
    glyph: char,
    title: String,
    detail: String,
    link: String,
}

fn lines(day: Option<&Day<'_>>) -> Vec<Line> {
    let mut lines = Vec::new();
    let Some(day) = day else { return lines };
    for event in &day.events {
        let (time, time_size, order) = if event.all_day {
            ("Todo el día".to_owned(), 10.0, (0, 0))
        } else {
            let time = local(event.start).map_or_else(String::new, |t| t.format("%H:%M").to_string());
            (time, 12.0, (1, event.start))
        };
        let detail = if event.location.is_empty() {
            shorten(&event.calendar, 34)
        } else {
            shorten(&format!("{} · {}", event.calendar, event.location), 34)
        };
        lines.push(Line {
            order,
            time,
            time_size,
            color: event_color(event),
            glyph: '\u{E787}',
            title: shorten(&event.title, 26),
            detail,
            link: event.link.clone(),
        });
    }
    for reset in &day.resets {
        lines.push(Line {
            order: (1, reset.at.timestamp()),
            time: reset.at.format("%H:%M").to_string(),
            time_size: 12.0,
            color: CYAN,
            glyph: '\u{E72C}',
            title: format!("Reinicio de {}", reset.name),
            detail: String::new(),
            link: String::new(),
        });
    }
    lines.sort_by_key(|line| line.order);
    lines
}

fn card(
    cx: &Ctx<'_, '_>,
    view: &View,
    readings: &Readings,
    days: &BTreeMap<NaiveDate, Day<'_>>,
    top: f32,
    height: f32,
    hits: &mut Vec<Hit>,
) -> Result<()> {
    let left = cx.px(PAD) + cx.px(GRID_W) + cx.px(20.0);
    let width = cx.px(WIDTH) - left - cx.px(PAD);
    cx.pen.round_rect((left, top), (width, height), cx.px(16.0), ink(cx.dark, 0.05))?;
    let inner_left = left + cx.px(16.0);
    let inner_w = width - cx.px(32.0);
    let right = inner_left + inner_w;

    let lines = lines(days.get(&view.selected));
    let today = Local::now().date_naive();
    let title = if view.selected == today {
        "Hoy".to_owned()
    } else {
        format!(
            "{} {} {}",
            DAYS_SHORT[view.selected.weekday().num_days_from_monday() as usize],
            view.selected.day(),
            MONTHS_SHORT[view.selected.month0() as usize % 12]
        )
    };
    let y = top + cx.px(16.0);
    let (_, th) = cx.gfx.measure(&title, 16.0, cx.scale, Weight::Bold)?;
    cx.pen.text(&title, (inner_left, y), 16.0, Weight::Bold, ink(cx.dark, 1.0))?;
    if !lines.is_empty() {
        let count = if lines.len() == 1 { "1 evento".to_owned() } else { format!("{} eventos", lines.len()) };
        let (cw, ch) = cx.gfx.measure(&count, 12.0, cx.scale, Weight::Normal)?;
        cx.pen.text(&count, (right - cw, y + (th - ch) / 2.0), 12.0, Weight::Normal, ink(cx.dark, 0.55))?;
    }
    let list_top = y + th + cx.px(12.0);

    if lines.is_empty() {
        let (_, eh) = cx.gfx.measure("Sin eventos", 13.0, cx.scale, Weight::Normal)?;
        cx.pen.text("Sin eventos", (inner_left, list_top), 13.0, Weight::Normal, ink(cx.dark, 0.55))?;
        let hint = match readings.reloj.as_ref().map(|r| r.calendar) {
            Some(Calendar::NotConnected) => Some("Conecta Google Calendar en Ajustes › Widgets"),
            Some(Calendar::Failed) => Some("No se pudo leer el calendario. Vuelve a conectar Google en Ajustes."),
            _ => None,
        };
        if let Some(hint) = hint {
            cx.pen.paragraph(hint, (inner_left, list_top + eh + cx.px(8.0)), inner_w, 11.0, ink(cx.dark, 0.55))?;
        }
        return Ok(());
    }

    let row_h = cx.px(ROW);
    let room = (top + height - cx.px(16.0) - list_top) / row_h;
    let capacity = (room.floor() as usize).max(1);
    let (shown, hidden) = if lines.len() > capacity {
        let shown = capacity.saturating_sub(1);
        (shown, lines.len() - shown)
    } else {
        (lines.len(), 0)
    };
    for (index, line) in lines.iter().take(shown).enumerate() {
        let ry = list_top + index as f32 * row_h;
        draw_line(cx, line, (inner_left, ry), row_h)?;
        if !line.link.is_empty() {
            hits.push(Hit {
                left: inner_left,
                top: ry,
                right,
                bottom: ry + row_h,
                action: PanelAction::Open(line.link.clone()),
            });
        }
    }
    if hidden > 0 {
        let ry = list_top + shown as f32 * row_h;
        let text = format!("+{hidden} más");
        let (_, mh) = cx.gfx.measure(&text, 12.0, cx.scale, Weight::Normal)?;
        cx.pen.text(&text, (inner_left, ry + (row_h - mh) / 2.0), 12.0, Weight::Normal, ink(cx.dark, 0.55))?;
    }
    Ok(())
}

fn draw_line(cx: &Ctx<'_, '_>, line: &Line, (x, y): (f32, f32), row_h: f32) -> Result<()> {
    let faded = ink(cx.dark, 0.55);
    let (_, th) = cx.gfx.measure(&line.time, line.time_size, cx.scale, Weight::Normal)?;
    cx.pen.text(&line.time, (x, y + (row_h - th) / 2.0), line.time_size, Weight::Normal, faded)?;
    let tile_x = x + cx.px(56.0);
    let side = cx.px(28.0);
    cx.tile(line.glyph, 14.0, line.color, (tile_x, y + (row_h - side) / 2.0), 28.0, 8.0)?;
    let text_x = tile_x + side + cx.px(8.0);
    if line.detail.is_empty() {
        let (_, h) = cx.gfx.measure(&line.title, 13.0, cx.scale, Weight::Bold)?;
        cx.pen.text(&line.title, (text_x, y + (row_h - h) / 2.0), 13.0, Weight::Bold, ink(cx.dark, 1.0))?;
    } else {
        let (_, h) = cx.gfx.measure(&line.title, 13.0, cx.scale, Weight::Bold)?;
        cx.pen.text(&line.title, (text_x, y + cx.px(5.0)), 13.0, Weight::Bold, ink(cx.dark, 1.0))?;
        cx.pen.text(&line.detail, (text_x, y + cx.px(5.0) + h + cx.px(1.0)), 11.0, Weight::Normal, faded)?;
    }
    Ok(())
}
