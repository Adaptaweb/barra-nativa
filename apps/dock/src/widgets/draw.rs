//! How each widget looks in the menu bar.

use super::{ClimaData, Readings, SistemaData, WidgetId};
use crate::gfx::{Gfx, LabelBrush, Rgba, Weight};
use anyhow::Result;

pub(super) const GREEN: Rgba = [0.204, 0.78, 0.349, 1.0];
pub(super) const ORANGE: Rgba = [1.0, 0.624, 0.039, 1.0];
pub(super) const RED: Rgba = [1.0, 0.271, 0.227, 1.0];
pub(super) const BLUE: Rgba = [0.039, 0.518, 1.0, 1.0];

pub(super) fn ink(light: bool, alpha: f32) -> Rgba {
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
        other => super::more::bar(gfx, other, readings, scale, height, light),
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
pub(super) fn symbol(code: u8, day: bool) -> &'static str {
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
