//! How the clock with the calendar, Home Assistant and the AI limits look in
//! the menu bar and the dock.

use super::draw::{BLUE, GREEN, ORANGE, RED, ink};
use super::{Calendar, CasaData, Home, Readings, RelojData, WidgetId};
use crate::gfx::{Gfx, LabelBrush, Rgba, Weight};
use crate::usage::{self, ProviderUsage, State, Window};
use anyhow::Result;
use chrono::{DateTime, Local, TimeZone, Utc};

const CALENDAR_RED: Rgba = [1.0, 0.231, 0.188, 1.0];

/// Menu bar look of the widgets that are not in `draw.rs`.
pub(super) fn bar(
    gfx: &Gfx,
    id: WidgetId,
    readings: &Readings,
    scale: f32,
    height: f32,
    light: bool,
) -> Result<Option<LabelBrush>> {
    match id {
        WidgetId::Reloj => readings
            .reloj
            .as_ref()
            .map(|d| reloj_bar(gfx, d, scale, height, light))
            .transpose(),
        WidgetId::Casa => readings
            .casa
            .as_ref()
            .map(|d| casa_bar(gfx, d, scale, height, light))
            .transpose(),
        WidgetId::Claude | WidgetId::ChatGpt | WidgetId::Gemini => {
            let (Some(snapshot), Some(provider)) = (readings.ia.as_ref(), id.provider()) else {
                return Ok(None);
            };
            ia_bar(gfx, provider, snapshot.get(provider), scale, height, light).map(Some)
        }
        // The bar has its own arrow for hidden tray icons.
        WidgetId::Control => readings
            .control
            .map(|d| control_bar(gfx, d, scale, height, light))
            .transpose(),
        WidgetId::Notificaciones => notificaciones_bar(gfx, scale, height, light).map(Some),
        WidgetId::Bateria => readings
            .control
            .and_then(|d| d.battery)
            .map(|b| bateria_bar(gfx, b, scale, height, light))
            .transpose(),
        WidgetId::Musica => readings
            .musica
            .as_ref()
            .map(|d| musica_bar(gfx, d, scale, height, light))
            .transpose(),
        WidgetId::Audio => readings
            .audio
            .as_ref()
            .map(|d| audio_bar(gfx, d, scale, height, light))
            .transpose(),
        WidgetId::Sistema | WidgetId::Memoria | WidgetId::Clima | WidgetId::Bandeja => Ok(None),
    }
}

/// The battery glyph (with a plug while charging) and the percent.
fn bateria_bar(gfx: &Gfx, b: muelle_shell::system::Battery, scale: f32, height: f32, light: bool) -> Result<LabelBrush> {
    let glyph = super::control::battery_glyph(b);
    let text = format!("{} %", b.percent);
    let (gw, gh) = gfx.measure_icon(glyph, 14.0, scale)?;
    let (tw, th) = gfx.measure(&text, 13.0, scale, Weight::Normal)?;
    let gap = (5.0 * scale).round();
    let color = if b.percent <= 20 && !b.charging { RED } else { ink(light, 1.0) };
    gfx.paint((gw + gap + tw, height), scale, |pen| {
        pen.icon(glyph, (0.0, (height - gh) / 2.0), 14.0, color)?;
        pen.text(&text, (gw + gap, (height - th) / 2.0), 13.0, Weight::Normal, ink(light, 1.0))?;
        Ok(())
    })
}

/// A note and the title of what is playing.
fn musica_bar(gfx: &Gfx, d: &super::MusicaData, scale: f32, height: f32, light: bool) -> Result<LabelBrush> {
    let size = 12.0;
    let gap = (5.0 * scale).round();
    let title = if d.title.is_empty() { &d.artist } else { &d.title };
    let text = shorten(title, 28);
    let (iw, ih) = gfx.measure_icon('\u{EC4F}', size, scale)?;
    let (tw, th) = gfx.measure(&text, 12.0, scale, Weight::Normal)?;
    gfx.paint((iw + gap + tw, height), scale, |pen| {
        pen.icon('\u{EC4F}', (0.0, (height - ih) / 2.0), size, ink(light, 0.8))?;
        pen.text(&text, (iw + gap, (height - th) / 2.0), 12.0, Weight::Normal, ink(light, 1.0))?;
        Ok(())
    })
}

/// Only the microphone, crossed out and red when muted.
fn audio_bar(gfx: &Gfx, d: &super::AudioData, scale: f32, height: f32, light: bool) -> Result<LabelBrush> {
    let size = 13.0;
    let (glyph, color) = if d.mic_muted == Some(true) {
        ('\u{F781}', RED)
    } else {
        ('\u{E720}', ink(light, 1.0))
    };
    let (w, h) = gfx.measure_icon(glyph, size, scale)?;
    gfx.paint((w, height), scale, |pen| {
        pen.icon(glyph, (0.0, (height - h) / 2.0), size, color)?;
        Ok(())
    })
}

/// Only the bell.
fn notificaciones_bar(gfx: &Gfx, scale: f32, height: f32, light: bool) -> Result<LabelBrush> {
    let size = 13.0;
    let (w, h) = gfx.measure_icon('\u{EA8F}', size, scale)?;
    gfx.paint((w, height), scale, |pen| {
        pen.icon('\u{EA8F}', (0.0, (height - h) / 2.0), size, ink(light, 1.0))?;
        Ok(())
    })
}

fn control_bar(
    gfx: &Gfx,
    d: super::ControlData,
    scale: f32,
    height: f32,
    light: bool,
) -> Result<LabelBrush> {
    let size = 13.0;
    let gap = (7.0 * scale).round();
    let mut glyphs = vec![super::control::network_glyph(d.network), super::control::volume_glyph(d.volume)];
    if d.bluetooth {
        glyphs.push('\u{E702}');
    }
    if let Some(b) = d.battery {
        glyphs.push(super::control::battery_glyph(b));
    }
    let mut width = 0.0;
    for (i, g) in glyphs.iter().enumerate() {
        width += gfx.measure_icon(*g, size, scale)?.0 + if i > 0 { gap } else { 0.0 };
    }
    let percent = d.battery.map(|b| format!("{}%", b.percent));
    if let Some(p) = &percent {
        width += (4.0 * scale).round() + gfx.measure(p, 12.0, scale, Weight::Normal)?.0;
    }
    gfx.paint((width.ceil(), height), scale, |pen| {
        let mut x = 0.0;
        for (i, g) in glyphs.iter().enumerate() {
            if i > 0 {
                x += gap;
            }
            let (_, h) = gfx.measure_icon(*g, size, scale)?;
            x += pen.icon(*g, (x, (height - h) / 2.0), size, ink(light, 1.0))?;
        }
        if let Some(p) = &percent {
            x += (4.0 * scale).round();
            let (_, h) = gfx.measure(p, 12.0, scale, Weight::Normal)?;
            pen.text(p, (x, (height - h) / 2.0), 12.0, Weight::Normal, ink(light, 1.0))?;
        }
        Ok(())
    })
}

fn level(percent: u8) -> Rgba {
    match percent {
        90.. => RED,
        70.. => ORANGE,
        _ => GREEN,
    }
}

fn local(seconds: i64) -> Option<DateTime<Local>> {
    Local.timestamp_opt(seconds, 0).single()
}

fn is_today(seconds: i64) -> bool {
    local(seconds).is_some_and(|t| t.date_naive() == Local::now().date_naive())
}

fn time_of(seconds: i64) -> String {
    local(seconds).map_or_else(String::new, |t| t.format("%H:%M").to_string())
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

/// "Ahora · Reunión", "15:00 Reunión", "Mañana 09:00 Reunión" for an event
/// in the next 24 hours; nothing for later ones.
fn event_text(d: &RelojData) -> Option<String> {
    let next = d.next.as_ref()?;
    let now = Utc::now().timestamp();
    if next.start - now > 24 * 3600 {
        return None;
    }
    let title = shorten(&next.title, 24);
    Some(if next.start <= now {
        format!("Ahora · {title}")
    } else if next.all_day {
        title
    } else if is_today(next.start) {
        format!("{} {title}", time_of(next.start))
    } else {
        format!("Mañana {} {title}", time_of(next.start))
    })
}

fn reloj_bar(gfx: &Gfx, d: &RelojData, scale: f32, height: f32, light: bool) -> Result<LabelBrush> {
    let px = |v: f32| (v * scale).round();
    let event = event_text(d);
    let (cw, ch) = gfx.measure(&d.clock, 13.0, scale, Weight::Normal)?;
    let event_size = event
        .as_ref()
        .map(|t| gfx.measure(t, 12.0, scale, Weight::Normal))
        .transpose()?;
    let dot = px(6.0);
    let lead = event_size.map_or(0.0, |(w, _)| dot + px(5.0) + w + px(12.0));
    let warning = (d.calendar == Calendar::Failed).then_some("⚠");
    let warn_w = match warning {
        Some(w) => gfx.measure(w, 11.0, scale, Weight::Normal)?.0 + px(6.0),
        None => 0.0,
    };
    gfx.paint((lead + warn_w + cw, height), scale, |pen| {
        let mut x = 0.0;
        if let (Some(text), Some((w, h))) = (&event, event_size) {
            pen.round_rect((x, (height - dot) / 2.0), (dot, dot), dot / 2.0, CALENDAR_RED)?;
            x += dot + px(5.0);
            pen.text(text, (x, (height - h) / 2.0), 12.0, Weight::Normal, ink(light, 0.72))?;
            x += w + px(12.0);
        }
        if let Some(w) = warning {
            let (_, h) = gfx.measure(w, 11.0, scale, Weight::Normal)?;
            pen.text(w, (x, (height - h) / 2.0), 11.0, Weight::Normal, ORANGE)?;
            x += warn_w;
        }
        pen.text(&d.clock, (x, (height - ch) / 2.0), 13.0, Weight::Normal, ink(light, 1.0))?;
        Ok(())
    })
}

fn casa_bar(gfx: &Gfx, d: &CasaData, scale: f32, height: f32, light: bool) -> Result<LabelBrush> {
    let text = match &d.state {
        Home::Connected => d.on.to_string(),
        Home::NotConnected => "Conectar".to_owned(),
        Home::Failed(_) => "!".to_owned(),
    };
    let icon = '\u{EA8A}';
    let (iw, ih) = gfx.measure_icon(icon, 12.0, scale)?;
    let (tw, th) = gfx.measure(&text, 13.0, scale, Weight::Normal)?;
    let gap = (4.0 * scale).round();
    gfx.paint((iw + gap + tw, height), scale, |pen| {
        pen.icon(icon, (0.0, (height - ih) / 2.0), 12.0, ink(light, 1.0))?;
        let color = match d.state {
            Home::Failed(_) => ORANGE,
            _ => ink(light, 1.0),
        };
        pen.text(&text, (iw + gap, (height - th) / 2.0), 13.0, Weight::Normal, color)?;
        Ok(())
    })
}

/// "1h 25m", "25m", "ahora" until a reset; the weekday when it is days away.
fn until(reset: Option<DateTime<Utc>>) -> String {
    let Some(reset) = reset else {
        return String::new();
    };
    let minutes = (reset - Utc::now()).num_minutes().max(0);
    if minutes == 0 {
        return "ahora".to_owned();
    }
    let hours = minutes / 60;
    if hours >= 24 {
        let names = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];
        let day = chrono::Datelike::weekday(&reset.with_timezone(&Local)).num_days_from_monday();
        return names[day as usize].to_owned();
    }
    if hours > 0 {
        format!("{hours}h {}m", minutes % 60)
    } else {
        format!("{minutes}m")
    }
}

/// Logo, then one row per window (5 h and weekly): label, bar, percent and
/// time to reset, as in the Barra reference design.
fn ia_bar(
    gfx: &Gfx,
    provider: usage::Provider,
    u: &ProviderUsage,
    scale: f32,
    height: f32,
    light: bool,
) -> Result<LabelBrush> {
    let px = |v: f32| (v * scale).round();
    let logo = px(16.0);
    let small = 8.0;
    let rows: Vec<&Window> = u.windows.iter().take(2).collect();
    if rows.is_empty() {
        let text = match (u.state, provider) {
            // Claude Code renews its sign-in each time it runs.
            (State::SignIn, usage::Provider::Claude) => "Abre Claude Code",
            (State::SignIn, usage::Provider::Codex) => "Inicia Codex CLI",
            (State::SignIn, usage::Provider::Gemini) => "Inicia Gemini CLI",
            (State::SignIn, _) => "Iniciar sesión",
            (State::Loading, _) => "…",
            _ => "Sin datos",
        };
        let (tw, th) = gfx.measure(text, 11.0, scale, Weight::Normal)?;
        let gap = px(5.0);
        return gfx.paint((logo + gap + tw, height), scale, |pen| {
            if let Some(image) = usage::logo(provider) {
                pen.image(image, (0.0, (height - logo) / 2.0), (logo, logo))?;
            }
            pen.text(text, (logo + gap, (height - th) / 2.0), 11.0, Weight::Normal, ink(light, 0.7))?;
            Ok(())
        });
    }
    let (label_w, row_h) = gfx.measure("7 d", small, scale, Weight::Bold)?;
    let (pct_w, _) = gfx.measure("100%", small, scale, Weight::Medium)?;
    let (reset_w, _) = gfx.measure("10h 59m", small, scale, Weight::Normal)?;
    let bar_w = px(30.0);
    let gap = px(5.0);
    let width = logo + px(7.0) + label_w + gap + bar_w + gap + reset_w + gap + pct_w;
    gfx.paint((width, height), scale, |pen| {
        if let Some(image) = usage::logo(provider) {
            pen.image(image, (0.0, (height - logo) / 2.0), (logo, logo))?;
        }
        let left = logo + px(7.0);
        let top = (height - row_h * rows.len() as f32) / 2.0;
        for (index, w) in rows.iter().enumerate() {
            let y = top + row_h * index as f32;
            pen.text(&w.title(), (left, y), small, Weight::Bold, ink(light, 0.75))?;
            let bar_x = left + label_w + gap;
            let bar_h = px(3.0);
            let bar_y = y + (row_h - bar_h) / 2.0;
            pen.round_rect((bar_x, bar_y), (bar_w, bar_h), bar_h / 2.0, ink(light, 0.22))?;
            let filled = bar_w * f32::from(w.used_percent.min(100)) / 100.0;
            if filled > 0.5 {
                pen.round_rect((bar_x, bar_y), (filled.max(bar_h), bar_h), bar_h / 2.0, level(w.used_percent))?;
            }
            let reset = until(w.resets_at);
            let (rw, _) = gfx.measure(&reset, small, scale, Weight::Normal)?;
            pen.text(
                &reset,
                (bar_x + bar_w + gap + reset_w - rw, y),
                small,
                Weight::Normal,
                ink(light, 0.55),
            )?;
            let pct = format!("{}%", w.used_percent);
            let (tw, _) = gfx.measure(&pct, small, scale, Weight::Medium)?;
            pen.text(&pct, (width - tw, y), small, Weight::Medium, ink(light, 1.0))?;
        }
        let _ = BLUE;
        Ok(())
    })
}
