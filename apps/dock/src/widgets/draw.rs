//! How each widget looks in the menu bar.

use super::{ClimaData, Readings, SistemaData, WidgetId};
use crate::gfx::{Gfx, LabelBrush, Rgba, Weight};
use windows::UI::Composition::CompositionSurfaceBrush;
use anyhow::Result;

const GREEN: Rgba = [0.204, 0.78, 0.349, 1.0];
const ORANGE: Rgba = [1.0, 0.624, 0.039, 1.0];
const RED: Rgba = [1.0, 0.271, 0.227, 1.0];
const BLUE: Rgba = [0.039, 0.518, 1.0, 1.0];

fn ink(light: bool, alpha: f32) -> Rgba {
    if light {
        [0.0, 0.0, 0.0, alpha * 0.86]
    } else {
        [1.0, 1.0, 1.0, alpha * 0.94]
    }
}

fn temp_color(t: u8) -> Rgba {
    match t {
        85.. => RED,
        70.. => ORANGE,
        _ => GREEN,
    }
}

/// The widget for the menu bar, `height` physical pixels tall, or `None`
/// while it has no reading yet.
pub fn render_bar(
    gfx: &Gfx,
    id: WidgetId,
    readings: &Readings,
    scale: f32,
    height: f32,
    light: bool,
) -> Result<Option<LabelBrush>> {
    match id {
        WidgetId::Sistema => readings
            .sistema
            .map(|data| sistema(gfx, data, scale, height, light))
            .transpose(),
        WidgetId::Clima => readings
            .clima
            .as_ref()
            .map(|data| clima(gfx, data, scale, height, light))
            .transpose(),
    }
}

/// A 270° ring with the temperature (or the CPU load without a sensor), then
/// CPU and RAM as two thin bars, as in the Barra reference design.
fn sistema(gfx: &Gfx, d: SistemaData, scale: f32, height: f32, light: bool) -> Result<LabelBrush> {
    let px = |v: f32| (v * scale).round();
    let ring = height;
    let (value, fraction, color, unit) = match d.temp {
        Some(t) => (t, f32::from(t) / 100.0, temp_color(t), "°"),
        None => (d.cpu, f32::from(d.cpu) / 100.0, BLUE, "%"),
    };
    let small = 8.0;
    let (label_w, row_h) = gfx.measure("RAM", small, scale, Weight::Bold)?;
    let (pct_w, _) = gfx.measure("100%", small, scale, Weight::Medium)?;
    let bar_w = px(34.0);
    let gap = px(5.0);
    let width = ring + px(7.0) + label_w + gap + bar_w + gap + pct_w;
    gfx.paint((width, height), scale, |pen| {
        let center = (ring / 2.0, height / 2.0);
        let stroke = px(2.6);
        let radius = ring / 2.0 - stroke / 2.0 - px(0.5);
        pen.arc(center, radius, stroke, (225.0, 270.0), ink(light, 0.28))?;
        pen.arc(center, radius, stroke, (225.0, 270.0 * fraction.clamp(0.0, 1.0)), color)?;
        let number = format!("{value}{unit}");
        let (nw, nh) = gfx.measure(&number, 7.5, scale, Weight::Bold)?;
        pen.text(
            &number,
            (center.0 - nw / 2.0, center.1 - nh / 2.0),
            7.5,
            Weight::Bold,
            ink(light, 1.0),
        )?;

        let left = ring + px(7.0);
        let rows = [("CPU", d.cpu), ("RAM", d.ram)];
        let top = (height - row_h * 2.0) / 2.0;
        for (index, (label, pct)) in rows.into_iter().enumerate() {
            let y = top + row_h * index as f32;
            pen.text(label, (left, y), small, Weight::Bold, ink(light, 0.75))?;
            let bar_x = left + label_w + gap;
            let bar_h = px(3.0);
            let bar_y = y + (row_h - bar_h) / 2.0;
            pen.round_rect((bar_x, bar_y), (bar_w, bar_h), bar_h / 2.0, ink(light, 0.22))?;
            let filled = bar_w * f32::from(pct.min(100)) / 100.0;
            if filled > 0.5 {
                pen.round_rect((bar_x, bar_y), (filled.max(bar_h), bar_h), bar_h / 2.0, BLUE)?;
            }
            let text = format!("{pct}%");
            let (tw, _) = gfx.measure(&text, small, scale, Weight::Medium)?;
            pen.text(
                &text,
                (width - tw, y),
                small,
                Weight::Medium,
                ink(light, 1.0),
            )?;
        }
        Ok(())
    })
}

/// Emoji for a WMO weather code.
fn symbol(code: u8, day: bool) -> &'static str {
    match code {
        0 | 1 if day => "☀️",
        0 | 1 => "🌙",
        2 => "⛅",
        3 => "☁️",
        45 | 48 => "🌫️",
        51..=57 | 61..=67 | 80..=82 => "🌧️",
        71..=77 | 85 | 86 => "❄️",
        95..=99 => "⛈️",
        _ => "🌡️",
    }
}

fn clima(gfx: &Gfx, d: &ClimaData, scale: f32, height: f32, light: bool) -> Result<LabelBrush> {
    let icon = symbol(d.code, d.day);
    let text = format!("{}°", d.temp);
    let (iw, ih) = gfx.measure_emoji(icon, 12.0, scale)?;
    let (tw, th) = gfx.measure(&text, 13.0, scale, Weight::Normal)?;
    let gap = (4.0 * scale).round();
    gfx.paint((iw + gap + tw, height), scale, |pen| {
        pen.emoji(icon, (0.0, (height - ih) / 2.0), 12.0)?;
        pen.text(
            &text,
            (iw + gap, (height - th) / 2.0),
            13.0,
            Weight::Normal,
            ink(light, 1.0),
        )?;
        Ok(())
    })
}

/// The widget as a square dock tile, `side` pixels, drawn like an app icon:
/// a rounded square with the reading inside.
pub fn render_tile(
    gfx: &Gfx,
    id: WidgetId,
    readings: &Readings,
    side: u32,
    light: bool,
) -> Result<CompositionSurfaceBrush> {
    let side = side.max(16) as f32;
    // Text sizes are relative to a 48 pixel tile.
    let unit = side / 48.0;
    let tile = gfx.paint((side, side), unit, |pen| {
        let inset = side * 0.06;
        let body = side - inset * 2.0;
        let radius = body * 0.23;
        match id {
            WidgetId::Sistema => {
                let back: Rgba = if light {
                    [0.97, 0.97, 0.98, 1.0]
                } else {
                    [0.13, 0.13, 0.15, 1.0]
                };
                pen.round_rect((inset, inset), (body, body), radius, back)?;
                let Some(d) = readings.sistema else {
                    return Ok(());
                };
                let (value, fraction, color, caption) = match d.temp {
                    Some(t) => (format!("{t}°"), f32::from(t) / 100.0, temp_color(t), "CPU"),
                    None => (format!("{}%", d.cpu), f32::from(d.cpu) / 100.0, BLUE, "CPU"),
                };
                let center = (side / 2.0, side / 2.0);
                let stroke = body * 0.085;
                let ring = body * 0.36;
                pen.arc(center, ring, stroke, (225.0, 270.0), ink(light, 0.22))?;
                pen.arc(center, ring, stroke, (225.0, 270.0 * fraction.clamp(0.0, 1.0)), color)?;
                let (vw, vh) = gfx.measure(&value, 11.0, unit, Weight::Bold)?;
                pen.text(
                    &value,
                    (center.0 - vw / 2.0, center.1 - vh / 2.0 - unit),
                    11.0,
                    Weight::Bold,
                    ink(light, 1.0),
                )?;
                let (cw, _) = gfx.measure(caption, 6.5, unit, Weight::Bold)?;
                pen.text(
                    caption,
                    (center.0 - cw / 2.0, center.1 + ring * 0.55),
                    6.5,
                    Weight::Bold,
                    ink(light, 0.6),
                )?;
                // Memory as a thin bar along the bottom edge.
                let bar_w = body * 0.5;
                let bar_h = (body * 0.05).max(2.0);
                let bar_x = (side - bar_w) / 2.0;
                let bar_y = inset + body - bar_h - body * 0.07;
                pen.round_rect((bar_x, bar_y), (bar_w, bar_h), bar_h / 2.0, ink(light, 0.18))?;
                let filled = bar_w * f32::from(d.ram.min(100)) / 100.0;
                pen.round_rect((bar_x, bar_y), (filled.max(bar_h), bar_h), bar_h / 2.0, GREEN)?;
            }
            WidgetId::Clima => {
                let day = readings.clima.as_ref().is_none_or(|d| d.day);
                let back: Rgba = if day {
                    [0.27, 0.6, 0.95, 1.0]
                } else {
                    [0.1, 0.16, 0.35, 1.0]
                };
                pen.round_rect((inset, inset), (body, body), radius, back)?;
                let Some(d) = readings.clima.as_ref() else {
                    return Ok(());
                };
                let icon = symbol(d.code, d.day);
                pen.emoji(icon, (inset + body * 0.1, inset + body * 0.06), 13.0)?;
                let text = format!("{}°", d.temp);
                let (tw, th) = gfx.measure(&text, 16.0, unit, Weight::Medium)?;
                pen.text(
                    &text,
                    (inset + body - tw - body * 0.08, inset + body - th - body * 0.04),
                    16.0,
                    Weight::Medium,
                    [1.0, 1.0, 1.0, 1.0],
                )?;
            }
        }
        Ok(())
    })?;
    Ok(tile.brush)
}
