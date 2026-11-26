//! The panel the weather widget opens, drawn by hand like the iOS weather:
//! a sky of the hour's colour, the city, the drawn icon big beside the
//! temperature, wind and rain in two tiles, the next days with a low-to-high
//! bar, and a button to the Windows weather app. Text is white whatever the
//! theme, since the sky decides the tone.

use super::{
    ClimaData, Readings,
    cell::weather_icon,
    clima::describe,
    panel::{Hit, PanelAction},
};
use crate::gfx::{Gfx, Pen, Rgba, Weight};
use anyhow::Result;

/// Logical width of the panel.
pub(super) const WIDTH: f32 = 340.0;

const PAD: f32 = 18.0;
const HEADER: f32 = 24.0;
const HERO: f32 = 84.0;
const TILE: f32 = 62.0;
const ROW: f32 = 38.0;
const BUTTON: f32 = 40.0;
const GAP: f32 = 12.0;

const WHITE: Rgba = [1.0; 4];
const WHITE_DIM: Rgba = [1.0, 1.0, 1.0, 0.75];
const SHADE: Rgba = [0.0, 0.0, 0.0, 0.18];
const TRACK: Rgba = [1.0, 1.0, 1.0, 0.2];
const LINE: Rgba = [1.0, 1.0, 1.0, 0.1];
const COOL: Rgba = [0.35, 0.78, 0.98, 1.0];
const WARM: Rgba = [1.0, 0.62, 0.04, 1.0];

/// Logical height of the panel for this reading.
pub(super) fn height(readings: &Readings) -> f32 {
    let days = readings.clima.as_ref().map_or(0, |d| d.days.len()) as f32;
    let forecast = if days > 0.0 { ROW * days + 12.0 + GAP } else { 0.0 };
    PAD + HEADER + HERO + GAP + TILE + GAP + forecast + BUTTON + PAD
}

/// The sky: blue on a fair day, grey-blue on a dull one, deep blue at night.
fn sky(d: &ClimaData) -> [Rgba; 2] {
    let fair = d.code <= 2;
    match (d.day, fair) {
        (false, _) => [[0.07, 0.10, 0.20, 0.95], [0.16, 0.21, 0.33, 0.95]],
        (true, true) => [[0.18, 0.49, 0.88, 0.95], [0.35, 0.65, 0.94, 0.95]],
        (true, false) => [[0.33, 0.42, 0.54, 0.95], [0.47, 0.56, 0.67, 0.95]],
    }
}

/// Today's rain as "0 mm" or "3,4 mm".
fn rain_text(rain: u16) -> String {
    if rain.is_multiple_of(10) {
        format!("{} mm", rain / 10)
    } else {
        format!("{},{} mm", rain / 10, rain % 10)
    }
}

pub(super) fn draw(pen: &Pen<'_>, gfx: &Gfx, readings: &Readings, scale: f32, hits: &mut Vec<Hit>) -> Result<()> {
    let px = |v: f32| (v * scale).round();
    let width = px(WIDTH);
    let height = px(height(readings));
    let Some(d) = readings.clima.as_ref() else {
        return Ok(());
    };
    let [mut top, mut bottom] = sky(d);
    // Liquid glass: the sky lets what is behind show through.
    if crate::appearance::current().material.is_liquid() {
        top[3] *= 0.55;
        bottom[3] *= 0.55;
    }
    pen.gradient_rect((0.0, 0.0), (width, height), px(8.0), (0.0, 0.0), (0.0, height), &[(0.0, top), (1.0, bottom)])?;
    let (left, right) = (px(PAD), width - px(PAD));
    let mut y = px(PAD);

    // The city, and where the reading comes from.
    let (_, ch) = gfx.measure(&d.city, 15.0, scale, Weight::Bold)?;
    pen.text(&d.city, (left, y + (px(HEADER) - ch) / 2.0), 15.0, Weight::Bold, WHITE)?;
    let (sw, sh) = gfx.measure("Open-Meteo", 11.0, scale, Weight::Normal)?;
    pen.text("Open-Meteo", (right - sw, y + (px(HEADER) - sh) / 2.0), 11.0, Weight::Normal, WHITE_DIM)?;
    y += px(HEADER);

    // The icon big beside the temperature, the sky in words and today's range.
    let icon = px(72.0);
    weather_icon(pen, (left, y + (px(HERO) - icon) / 2.0), icon, d.code, d.day)?;
    let text_x = left + icon + px(14.0);
    let temp = format!("{}°", d.temp);
    let (_, th) = gfx.measure(&temp, 50.0, scale, Weight::Heavy)?;
    let (_, dh) = gfx.measure("Ág", 14.0, scale, Weight::Medium)?;
    let (_, rh) = gfx.measure("Ág", 12.0, scale, Weight::Normal)?;
    let block = th + dh + rh;
    let mut ty = y + (px(HERO) - block) / 2.0;
    pen.text(&temp, (text_x, ty), 50.0, Weight::Heavy, WHITE)?;
    ty += th;
    pen.text(describe(d.code), (text_x, ty), 14.0, Weight::Medium, WHITE)?;
    ty += dh;
    pen.text(&format!("Máx. {}° · Mín. {}°", d.max, d.min), (text_x, ty), 12.0, Weight::Normal, WHITE_DIM)?;
    y += px(HERO) + px(GAP);

    // Wind and rain, a tile each.
    let tile_w = (right - left - px(10.0)) / 2.0;
    for (index, (label, value)) in [("VIENTO", format!("{} km/h", d.wind)), ("LLUVIA", rain_text(d.rain))]
        .into_iter()
        .enumerate()
    {
        let x = left + (tile_w + px(10.0)) * index as f32;
        pen.round_rect((x, y), (tile_w, px(TILE)), px(14.0), SHADE)?;
        let (_, lh) = gfx.measure(label, 10.0, scale, Weight::Bold)?;
        let (_, vh) = gfx.measure(&value, 20.0, scale, Weight::Bold)?;
        let inner = (px(TILE) - lh - vh - px(4.0)) / 2.0;
        pen.text(label, (x + px(12.0), y + inner), 10.0, Weight::Bold, WHITE_DIM)?;
        pen.text(&value, (x + px(12.0), y + inner + lh + px(4.0)), 20.0, Weight::Bold, WHITE)?;
    }
    y += px(TILE) + px(GAP);

    // The next days: name, icon, low, where the day falls on the week's range, high.
    if !d.days.is_empty() {
        let box_h = px(ROW) * d.days.len() as f32 + px(12.0);
        pen.round_rect((left, y), (right - left, box_h), px(14.0), SHADE)?;
        let lowest = d.days.iter().map(|(_, _, _, low)| *low).chain([d.min]).min().unwrap_or(d.min);
        let highest = d.days.iter().map(|(_, _, high, _)| *high).chain([d.max]).max().unwrap_or(d.max);
        let span = f32::from((highest - lowest).max(1));
        let (num_w, _) = gfx.measure("-88°", 13.0, scale, Weight::Bold)?;
        let name_x = left + px(12.0);
        let icon_side = px(22.0);
        let icon_x = name_x + px(44.0);
        let low_right = icon_x + icon_side + px(10.0) + num_w;
        let high_right = right - px(12.0);
        let bar_x = low_right + px(10.0);
        let bar_end = high_right - num_w - px(10.0);
        let bar_w = (bar_end - bar_x).max(0.0);
        let mut row_y = y + px(6.0);
        for (index, (name, code, high, low)) in d.days.iter().enumerate() {
            let row = px(ROW);
            if index > 0 {
                pen.round_rect((name_x, row_y), (right - left - px(24.0), px(1.0).max(1.0)), 0.0, LINE)?;
            }
            let (_, nh) = gfx.measure(name, 13.0, scale, Weight::Bold)?;
            pen.text(name, (name_x, row_y + (row - nh) / 2.0), 13.0, Weight::Bold, WHITE)?;
            weather_icon(pen, (icon_x, row_y + (row - icon_side) / 2.0), icon_side, *code, true)?;
            let low_text = format!("{low}°");
            let (lw, lh) = gfx.measure(&low_text, 13.0, scale, Weight::Normal)?;
            pen.text(&low_text, (low_right - lw, row_y + (row - lh) / 2.0), 13.0, Weight::Normal, WHITE_DIM)?;
            let high_text = format!("{high}°");
            let (hw, hh) = gfx.measure(&high_text, 13.0, scale, Weight::Bold)?;
            pen.text(&high_text, (high_right - hw, row_y + (row - hh) / 2.0), 13.0, Weight::Bold, WHITE)?;
            let bar_h = px(5.0);
            let bar_y = row_y + (row - bar_h) / 2.0;
            pen.round_rect((bar_x, bar_y), (bar_w, bar_h), bar_h / 2.0, TRACK)?;
            let from = bar_x + bar_w * f32::from(low - lowest) / span;
            let to = bar_x + bar_w * f32::from(high - lowest) / span;
            let fill_w = (to - from).max(bar_h);
            pen.gradient_rect((from, bar_y), (fill_w, bar_h), bar_h / 2.0, (bar_x, bar_y), (bar_x + bar_w, bar_y), &[(0.0, COOL), (1.0, WARM)])?;
            row_y += row;
        }
        y += box_h + px(GAP);
    }

    // The Windows weather app.
    let label = "Abrir El Tiempo";
    let (lw, lh) = gfx.measure(label, 13.0, scale, Weight::Bold)?;
    let button_w = lw + px(32.0);
    pen.round_rect((left, y), (button_w, px(BUTTON)), px(12.0), [1.0, 1.0, 1.0, 0.18])?;
    pen.text(label, (left + px(16.0), y + (px(BUTTON) - lh) / 2.0), 13.0, Weight::Bold, WHITE)?;
    hits.push(Hit {
        left,
        top: y,
        right: left + button_w,
        bottom: y + px(BUTTON),
        action: PanelAction::Open("msnweather:".to_owned()),
    });
    Ok(())
}
