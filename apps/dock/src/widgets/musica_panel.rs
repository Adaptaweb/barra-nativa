//! The panel the Now Playing widget opens: a mini player with the cover, what
//! plays, the progress and the transport buttons, on a gradient of the cover's
//! colours. Text is white whatever the theme, since the gradient decides the tone.

use super::{
    Readings,
    musica::{NO_ARTWORK, NO_ARTWORK_2},
    panel::{Hit, PanelAction},
};
use crate::gfx::{Gfx, Pen, Rgba, Weight};
use crate::media::{self, Control};
use anyhow::Result;

/// Logical width of the panel.
pub(super) const WIDTH: f32 = 340.0;

const HEIGHT: f32 = 180.0;
const PAD: f32 = 16.0;
const ART: f32 = 64.0;
const BAR_TOP: f32 = 94.0;
const CONTROLS_Y: f32 = 128.0;
const VOLUME_Y: f32 = 160.0;
const HIT: f32 = 36.0;
/// Distance between the centers of the transport buttons.
const STEP: f32 = 48.0;

const WHITE: Rgba = [1.0; 4];
const WHITE_DIM: Rgba = [1.0, 1.0, 1.0, 0.7];
const WHITE_OFF: Rgba = [1.0, 1.0, 1.0, 0.35];
const TRACK: Rgba = [1.0, 1.0, 1.0, 0.25];
const FILL: Rgba = [1.0, 1.0, 1.0, 0.9];

/// Logical height of the panel.
pub(super) fn height() -> f32 {
    HEIGHT
}

/// Everything a drawing helper needs.
struct Ctx<'a, 'p> {
    pen: &'a Pen<'p>,
    gfx: &'a Gfx,
    scale: f32,
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

    /// Icon centered on (`x`, `y`), with a square hit area when it has an action.
    fn button(&self, glyph: char, size: f32, color: Rgba, (x, y): (f32, f32), action: Option<PanelAction>, hits: &mut Vec<Hit>) -> Result<()> {
        let (gw, gh) = self.gfx.measure_icon(glyph, size, self.scale)?;
        self.pen.icon(glyph, (x - gw / 2.0, y - gh / 2.0), size, color)?;
        if let Some(action) = action {
            let half = self.px(HIT) / 2.0;
            hits.push(Hit { left: x - half, top: y - half, right: x + half, bottom: y + half, action });
        }
        Ok(())
    }
}

/// "3:07", or "1:02:03" from an hour on.
fn track_time(seconds: u32) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// `slider` is set to the physical left and right x of the volume slider.
pub(super) fn draw(
    pen: &Pen<'_>,
    gfx: &Gfx,
    readings: &Readings,
    scale: f32,
    hits: &mut Vec<Hit>,
    slider: &mut Option<(f32, f32)>,
) -> Result<()> {
    let cx = Ctx { pen, gfx, scale };
    let d = readings.musica.as_ref();
    let width = cx.px(WIDTH);
    let height = cx.px(HEIGHT);

    // Over the frosted background of the popup, so a little see-through;
    // with liquid glass, clearly see-through.
    let alpha = if crate::appearance::current().material.is_liquid() { 0.55 } else { 0.92 };
    let colors = d.map_or([NO_ARTWORK, NO_ARTWORK_2], |d| d.colors).map(|c| [c[0], c[1], c[2], alpha]);
    pen.gradient_rect(
        (0.0, 0.0),
        (width, height),
        cx.px(8.0),
        (0.0, 0.0),
        (width, height),
        &[(0.0, colors[0]), (1.0, colors[1])],
    )?;

    // Cover, then title and artist centered in the room to its right.
    let pad = cx.px(PAD);
    let art = cx.px(ART);
    if let Some(image) = d.and_then(|d| d.art.as_deref()).or(media::square_fallback()) {
        pen.image(image, (pad, pad), (art, art))?;
    }
    let text_x = pad + art + cx.px(14.0);
    let room = (width - pad - text_x).max(0.0);
    let (title, artist) = match d {
        Some(d) if d.title.is_empty() => ("Sin título", d.artist.as_str()),
        Some(d) => (d.title.as_str(), d.artist.as_str()),
        None => ("Nada sonando", ""),
    };
    let title = cx.fit(title, 14.0, Weight::Bold, room)?;
    let artist = cx.fit(artist, 12.0, Weight::Normal, room)?;
    let (_, th) = gfx.measure("Ág", 14.0, scale, Weight::Bold)?;
    let (_, ah) = gfx.measure("Ág", 12.0, scale, Weight::Normal)?;
    let lines = if artist.is_empty() { th } else { th + ah };
    let mut y = pad + (art - lines) / 2.0;
    let (tw, _) = gfx.measure(&title, 14.0, scale, Weight::Bold)?;
    pen.text(&title, (text_x + (room - tw) / 2.0, y), 14.0, Weight::Bold, WHITE)?;
    y += th;
    if !artist.is_empty() {
        let (aw, _) = gfx.measure(&artist, 12.0, scale, Weight::Normal)?;
        pen.text(&artist, (text_x + (room - aw) / 2.0, y), 12.0, Weight::Normal, WHITE_DIM)?;
    }

    progress(&cx, d.map_or((0, 0), |d| (d.position_s, d.duration_s)), width)?;
    controls(&cx, d, width, hits)?;
    volume(&cx, d.and_then(|d| d.volume), width, hits, slider)
}

/// Elapsed time, a thin bar and the total, on one row.
fn progress(cx: &Ctx<'_, '_>, (position, duration): (u32, u32), width: f32) -> Result<()> {
    let pad = cx.px(PAD);
    let top = cx.px(BAR_TOP);
    let (elapsed, total) = if duration == 0 {
        ("0:00".to_owned(), "0:00".to_owned())
    } else {
        (track_time(position.min(duration)), track_time(duration))
    };
    let (ew, eh) = cx.gfx.measure(&elapsed, 10.5, cx.scale, Weight::Normal)?;
    let (tw, _) = cx.gfx.measure(&total, 10.5, cx.scale, Weight::Normal)?;
    cx.pen.text(&elapsed, (pad, top), 10.5, Weight::Normal, WHITE_DIM)?;
    cx.pen.text(&total, (width - pad - tw, top), 10.5, Weight::Normal, WHITE_DIM)?;

    let gap = cx.px(8.0);
    let bar_x = pad + ew + gap;
    let bar_w = (width - pad - tw - gap - bar_x).max(0.0);
    let bar_h = cx.px(3.0);
    let bar_y = top + (eh - bar_h) / 2.0;
    cx.pen.round_rect((bar_x, bar_y), (bar_w, bar_h), bar_h / 2.0, TRACK)?;
    if duration > 0 {
        let ratio = position.min(duration) as f32 / duration as f32;
        let filled = (bar_w * ratio).max(bar_h.min(bar_w));
        cx.pen.round_rect((bar_x, bar_y), (filled, bar_h), bar_h / 2.0, FILL)?;
    }
    Ok(())
}

/// The speaker (mute) on the left, a volume slider and the percent on one row.
/// `slider` gets the left and right physical x of the slider track.
fn volume(
    cx: &Ctx<'_, '_>,
    volume: Option<(u8, bool)>,
    width: f32,
    hits: &mut Vec<Hit>,
    slider: &mut Option<(f32, f32)>,
) -> Result<()> {
    let y = cx.px(VOLUME_Y);
    let pad = cx.px(PAD);
    let glyph = if volume.is_some_and(|(_, muted)| muted) { '\u{E74F}' } else { '\u{E767}' };
    // Centered on the glyph's hit area, so it lines up with the left padding.
    let (color, action) = if volume.is_some() {
        (WHITE, Some(PanelAction::ToggleMute))
    } else {
        (WHITE_OFF, None)
    };
    cx.button(glyph, 14.0, color, (cx.px(PAD + HIT / 2.0), y), action, hits)?;

    let text = volume.map_or_else(|| "--".to_owned(), |(percent, _)| percent.to_string());
    let (tw, _) = cx.gfx.measure("100", 10.5, cx.scale, Weight::Normal)?;
    let (nw, nh) = cx.gfx.measure(&text, 10.5, cx.scale, Weight::Normal)?;
    let text_color = if volume.is_some() { WHITE_DIM } else { WHITE_OFF };
    cx.pen.text(&text, (width - pad - nw, y - nh / 2.0), 10.5, Weight::Normal, text_color)?;

    let left = pad + cx.px(HIT);
    let right = (width - pad - tw - cx.px(8.0)).max(left);
    let track_h = cx.px(4.0);
    let knob = cx.px(12.0);
    cx.pen.round_rect((left, y - track_h / 2.0), (right - left, track_h), track_h / 2.0, TRACK)?;
    let ratio = volume.map_or(0.0, |(percent, _)| f32::from(percent) / 100.0);
    let end = left + (right - left) * ratio;
    if volume.is_some() {
        let filled = (end - left).max(track_h.min(right - left));
        cx.pen.round_rect((left, y - track_h / 2.0), (filled, track_h), track_h / 2.0, FILL)?;
    }
    let knob_color = if volume.is_some() { WHITE } else { WHITE_OFF };
    cx.pen.round_rect((end - knob / 2.0, y - knob / 2.0), (knob, knob), knob / 2.0, knob_color)?;
    if volume.is_some() {
        let half = cx.px(10.0);
        hits.push(Hit { left, top: y - half, right, bottom: y + half, action: PanelAction::Volume });
        *slider = Some((left, right));
    }
    Ok(())
}

/// The transport buttons, centered.
fn controls(cx: &Ctx<'_, '_>, d: Option<&super::MusicaData>, width: f32, hits: &mut Vec<Hit>) -> Result<()> {
    let y = cx.px(CONTROLS_Y);
    let middle = width / 2.0;
    let step = cx.px(STEP);
    let playing = d.is_some_and(|d| d.playing);
    let all = [
        ('\u{E892}', 18.0, Control::Previous, d.is_some_and(|d| d.can_prev), -step),
        (if playing { '\u{E769}' } else { '\u{E768}' }, 22.0, Control::PlayPause, d.is_some_and(|d| d.can_toggle), 0.0),
        ('\u{E893}', 18.0, Control::Next, d.is_some_and(|d| d.can_next), step),
    ];
    for (glyph, size, control, enabled, offset) in all {
        let color = if enabled { WHITE } else { WHITE_OFF };
        let action = enabled.then_some(PanelAction::Media(control));
        cx.button(glyph, size, color, (middle + offset, y), action, hits)?;
    }
    Ok(())
}
