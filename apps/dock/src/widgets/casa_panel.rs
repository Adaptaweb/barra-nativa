//! The panel the Casa widget opens: the header, the camera picture and the
//! Home Assistant buttons as cards grouped by kind. Only drawn while Home
//! Assistant is connected; the setup steps and errors use the block panel.

use super::{
    CamaraData, CameraStatus, Home, HomeButton, Readings,
    panel::{Hit, PanelAction, ink},
};
use crate::gfx::{Gfx, Pen, Rgba, Weight};
use anyhow::Result;

/// Logical width of the panel.
pub(super) const WIDTH: f32 = 300.0;

const PAD: f32 = 14.0;
/// Space between the header, the camera and each section.
const GAP: f32 = 14.0;
const HEADER: f32 = 44.0;
const ROUND_BUTTON: f32 = 30.0;
const SECTION: f32 = 20.0;
const CARD_H: f32 = 52.0;
const CARD_GAP: f32 = 8.0;
const CARD_RADIUS: f32 = 14.0;
const CAMERA_RADIUS: f32 = 12.0;

const BLUE: Rgba = [0.25, 0.6, 1.0, 1.0];
const CARD_ON: Rgba = [1.0, 1.0, 1.0, 0.95];
const TEXT_ON: Rgba = [0.1, 0.1, 0.12, 1.0];
const WHITE: Rgba = [1.0; 4];

/// Whether this panel draws the readings (Home Assistant answered).
pub(super) fn applies(readings: &Readings) -> bool {
    readings.casa.as_ref().is_some_and(|d| d.state == Home::Connected)
}

fn buttons(readings: &Readings) -> &[HomeButton] {
    readings.casa.as_ref().map_or(&[], |d| d.buttons.as_slice())
}

fn camera_height() -> f32 {
    (WIDTH - PAD * 2.0) * 9.0 / 16.0
}

fn grid_height(cards: usize) -> f32 {
    let rows = cards.div_ceil(2) as f32;
    rows * CARD_H + (rows - 1.0).max(0.0) * CARD_GAP
}

/// Logical height of the panel.
pub(super) fn height(readings: &Readings) -> f32 {
    let mut total = PAD * 2.0 + HEADER;
    if readings.camara.is_some() {
        total += GAP + camera_height();
    }
    for (_, cards) in sections(buttons(readings)) {
        total += GAP + SECTION + grid_height(cards.len());
    }
    total
}

/// The buttons by kind: lights, plugs, then the other kinds as they come.
fn sections(buttons: &[HomeButton]) -> Vec<(&str, Vec<&HomeButton>)> {
    let mut kinds = vec!["luz", "enchufe"];
    for button in buttons {
        if !kinds.contains(&button.kind.as_str()) {
            kinds.push(&button.kind);
        }
    }
    kinds
        .into_iter()
        .filter_map(|kind| {
            let group: Vec<&HomeButton> = buttons.iter().filter(|b| b.kind == kind).collect();
            (!group.is_empty()).then_some((kind, group))
        })
        .collect()
}

fn section_title(kind: &str) -> String {
    match kind {
        "luz" => "LUCES".to_owned(),
        "enchufe" => "ENCHUFES".to_owned(),
        "persiana" => "PERSIANAS".to_owned(),
        "sensor" => "SENSORES".to_owned(),
        "clima" => "CLIMA".to_owned(),
        "" => "OTROS".to_owned(),
        other => other.to_uppercase(),
    }
}

/// Segoe Fluent Icons glyph of a kind of button.
fn glyph(kind: &str) -> char {
    match kind {
        "luz" => '\u{EA80}',
        "sensor" => '\u{E946}',
        _ => '\u{E7E8}',
    }
}

fn faded(color: Rgba, factor: f32) -> Rgba {
    [color[0], color[1], color[2], color[3] * factor]
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

    /// `text` cut with an ellipsis so it is at most `max` pixels wide.
    fn fit(&self, text: &str, size: f32, weight: Weight, max: f32) -> Result<String> {
        if text.is_empty() || self.gfx.measure(text, size, self.scale, weight)?.0 <= max {
            return Ok(text.to_owned());
        }
        let chars: Vec<char> = text.chars().collect();
        // The most characters that still fit with the ellipsis.
        let (mut low, mut high) = (0, chars.len());
        while low < high {
            let mid = (low + high).div_ceil(2);
            let cut: String = chars[..mid].iter().chain(&['…']).collect();
            if self.gfx.measure(&cut, size, self.scale, weight)?.0 <= max {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        Ok(chars[..low].iter().chain(&['…']).collect())
    }

    /// A symbol centered in the square whose top left is (`x`, `y`).
    fn glyph_in(&self, glyph: char, size: f32, color: Rgba, (x, y): (f32, f32), side: f32) -> Result<()> {
        let (gw, gh) = self.gfx.measure_icon(glyph, size, self.scale)?;
        self.pen.icon(glyph, (x + (side - gw) / 2.0, y + (side - gh) / 2.0), size, color)?;
        Ok(())
    }

    fn header(&self, on: u8, home_url: &str, hits: &mut Vec<Hit>) -> Result<()> {
        let dark = self.dark;
        let (left, top) = (self.px(PAD), self.px(PAD));
        let header = self.px(HEADER);
        let (_, title_h) = self.gfx.measure("Casa", 20.0, self.scale, Weight::Heavy)?;
        let (_, note_h) = self.gfx.measure("Ág", 11.0, self.scale, Weight::Normal)?;
        let mut y = top + (header - title_h - note_h) / 2.0;
        self.pen.text("Casa", (left, y), 20.0, Weight::Heavy, ink(dark, 1.0))?;
        y += title_h;
        let note = match on {
            0 => "todo apagado".to_owned(),
            1 => "1 encendido".to_owned(),
            n => format!("{n} encendidos"),
        };
        self.pen.text(&note, (left, y), 11.0, Weight::Normal, ink(dark, 0.6))?;

        // The round buttons on the right: Home Assistant, then Barra Settings.
        let side = self.px(ROUND_BUTTON);
        let by = top + (header - side) / 2.0;
        let mut bx = self.px(WIDTH - PAD) - side;
        let mut round = vec![('\u{E713}', PanelAction::OpenSettings)];
        if !home_url.is_empty() {
            round.push(('\u{E8A7}', PanelAction::Open(home_url.to_owned())));
        }
        for (glyph, action) in round {
            self.pen.round_rect((bx, by), (side, side), side / 2.0, ink(dark, 0.1))?;
            self.glyph_in(glyph, 13.0, ink(dark, 0.9), (bx, by), side)?;
            hits.push(Hit { left: bx, top: by, right: bx + side, bottom: by + side, action });
            bx -= side + self.px(8.0);
        }
        Ok(())
    }

    fn camera(&self, camera: &CamaraData, top: f32, hits: &mut Vec<Hit>) -> Result<()> {
        let (x, y) = (self.px(PAD), self.px(top));
        let (width, height) = (self.px(WIDTH - PAD * 2.0), self.px(camera_height()));
        let radius = self.px(CAMERA_RADIUS);
        match &camera.image {
            Some(image) => self.pen.image(image, (x, y), (width, height))?,
            None => self.pen.round_rect((x, y), (width, height), radius, ink(self.dark, 0.1))?,
        }
        if !camera.name.is_empty() {
            let strip = self.px(44.0);
            let strip_top = y + height - strip;
            self.pen.gradient_rect(
                (x, strip_top),
                (width, strip),
                radius,
                (0.0, strip_top),
                (0.0, y + height),
                &[(0.0, [0.0, 0.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 0.0, 0.6])],
            )?;
            let name = self.fit(&camera.name, 13.0, Weight::Bold, width - self.px(20.0))?;
            let (_, nh) = self.gfx.measure(&name, 13.0, self.scale, Weight::Bold)?;
            self.pen.text(&name, (x + self.px(10.0), y + height - self.px(10.0) - nh), 13.0, Weight::Bold, WHITE)?;
        }
        let state = match &camera.status {
            CameraStatus::Ok => None,
            CameraStatus::Connecting => Some("Conectando…"),
            CameraStatus::Error(error) => Some(error.as_str()),
        };
        if let Some(state) = state {
            let state = self.fit(state, 10.5, Weight::Normal, width * 0.7)?;
            let (tw, th) = self.gfx.measure(&state, 10.5, self.scale, Weight::Normal)?;
            let (pw, ph) = (tw + self.px(16.0), th + self.px(8.0));
            let (px, py) = (x + width - pw - self.px(8.0), y + self.px(8.0));
            self.pen.round_rect((px, py), (pw, ph), ph / 2.0, [0.0, 0.0, 0.0, 0.55])?;
            self.pen.text(&state, (px + self.px(8.0), py + self.px(4.0)), 10.5, Weight::Normal, WHITE)?;
        }
        if !camera.page.is_empty() {
            hits.push(Hit {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
                action: PanelAction::Open(camera.page.clone()),
            });
        }
        Ok(())
    }

    /// The kind's title on the left and how many are on, on the right.
    fn section(&self, kind: &str, group: &[&HomeButton], top: f32) -> Result<()> {
        let (left, right, y) = (self.px(PAD), self.px(WIDTH - PAD), self.px(top));
        let (_, th) = self.gfx.measure("Ág", 9.5, self.scale, Weight::Bold)?;
        self.pen.text(&section_title(kind), (left, y), 9.5, Weight::Bold, ink(self.dark, 0.5))?;
        // Sensors and the like are not switched, so they have no count.
        if !group.iter().any(|b| b.switchable) {
            return Ok(());
        }
        let on = group.iter().filter(|b| b.on).count();
        let note = if on > 0 {
            format!("{on} de {}", group.len())
        } else if matches!(kind, "luz" | "persiana") {
            "apagadas".to_owned()
        } else {
            "apagados".to_owned()
        };
        let (nw, nh) = self.gfx.measure(&note, 10.0, self.scale, Weight::Normal)?;
        self.pen.text(&note, (right - nw, y + (th - nh) / 2.0), 10.0, Weight::Normal, ink(self.dark, 0.5))?;
        Ok(())
    }

    /// The cards of a section, two per row, starting at logical `top`.
    fn cards(&self, group: &[&HomeButton], top: f32, hits: &mut Vec<Hit>) -> Result<()> {
        let gap = self.px(CARD_GAP);
        let card_w = (self.px(WIDTH - PAD * 2.0) - gap) / 2.0;
        let card_h = self.px(CARD_H);
        for (index, button) in group.iter().enumerate() {
            let x = self.px(PAD) + (index % 2) as f32 * (card_w + gap);
            let y = self.px(top) + (index / 2) as f32 * (card_h + gap);
            self.card(button, (x, y), (card_w, card_h))?;
            if button.switchable && button.available {
                hits.push(Hit {
                    left: x,
                    top: y,
                    right: x + card_w,
                    bottom: y + card_h,
                    action: PanelAction::HomeToggle(button.id.clone()),
                });
            }
        }
        Ok(())
    }

    fn card(&self, button: &HomeButton, (x, y): (f32, f32), (width, height): (f32, f32)) -> Result<()> {
        let dark = self.dark;
        let fade = if button.available { 1.0 } else { 0.45 };
        let (back, circle, mark, name_color, state_color) = if button.on {
            (CARD_ON, BLUE, WHITE, TEXT_ON, faded(TEXT_ON, 0.6))
        } else {
            (ink(dark, 0.08), ink(dark, 0.1), ink(dark, 0.6), ink(dark, 1.0), ink(dark, 0.55))
        };
        self.pen.round_rect((x, y), (width, height), self.px(CARD_RADIUS), faded(back, fade))?;

        let side = self.px(30.0);
        let (cx, cy) = (x + self.px(10.0), y + (height - side) / 2.0);
        self.pen.round_rect((cx, cy), (side, side), side / 2.0, faded(circle, fade))?;
        self.glyph_in(glyph(&button.kind), 14.0, faded(mark, fade), (cx, cy), side)?;

        let text_x = cx + side + self.px(8.0);
        let room = (x + width - self.px(8.0) - text_x).max(0.0);
        let state = if !button.available {
            "No disponible".to_owned()
        } else if let Some(value) = &button.value {
            value.clone()
        } else if button.on {
            "Encendido".to_owned()
        } else {
            "Apagado".to_owned()
        };
        let name = self.fit(&button.name, 12.0, Weight::Bold, room)?;
        let state = self.fit(&state, 10.5, Weight::Normal, room)?;
        let (_, nh) = self.gfx.measure("Ág", 12.0, self.scale, Weight::Bold)?;
        let (_, sh) = self.gfx.measure("Ág", 10.5, self.scale, Weight::Normal)?;
        let ty = y + (height - nh - sh) / 2.0;
        self.pen.text(&name, (text_x, ty), 12.0, Weight::Bold, faded(name_color, fade))?;
        self.pen.text(&state, (text_x, ty + nh), 10.5, Weight::Normal, faded(state_color, fade))?;
        Ok(())
    }
}

pub(super) fn draw(
    pen: &Pen<'_>,
    gfx: &Gfx,
    readings: &Readings,
    dark: bool,
    scale: f32,
    hits: &mut Vec<Hit>,
) -> Result<()> {
    let cx = Ctx { pen, gfx, scale, dark };
    let Some(data) = readings.casa.as_ref() else {
        return Ok(());
    };
    let (home_url, _) = super::home_settings();
    cx.header(data.on, &home_url, hits)?;
    let mut y = PAD + HEADER;
    if let Some(camera) = &readings.camara {
        y += GAP;
        cx.camera(camera, y, hits)?;
        y += camera_height();
    }
    for (kind, group) in sections(&data.buttons) {
        y += GAP;
        cx.section(kind, &group, y)?;
        y += SECTION;
        cx.cards(&group, y, hits)?;
        y += grid_height(group.len());
    }
    Ok(())
}
