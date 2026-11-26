//! What each widget's panel shows. When a widget has nothing set up, its
//! panel explains how to set it up, step by step.

use super::panel::{Block, Cell, PanelAction};
use super::{Home, Readings, TempSource, WidgetId};
use crate::usage::{Provider, State};
use chrono::{DateTime, Local, Utc};

const GREEN: [f32; 4] = [0.204, 0.78, 0.349, 1.0];
const ORANGE: [f32; 4] = [1.0, 0.624, 0.039, 1.0];
const RED: [f32; 4] = [1.0, 0.271, 0.227, 1.0];
const BLUE: [f32; 4] = [0.039, 0.518, 1.0, 1.0];

fn level(percent: u8) -> [f32; 4] {
    match percent {
        90.. => RED,
        70.. => ORANGE,
        _ => GREEN,
    }
}

fn open(label: &str, target: &str, primary: bool) -> (String, PanelAction, bool) {
    (label.to_owned(), PanelAction::Open(target.to_owned()), primary)
}

const WEEKDAYS: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];

/// "Hoy", "Mañana" or "jue 27" for the day of an event.
fn day_label(t: DateTime<Local>) -> String {
    use chrono::Datelike;
    let today = Local::now().date_naive();
    let days = (t.date_naive() - today).num_days();
    match days {
        0 => "Hoy".to_owned(),
        1 => "Mañana".to_owned(),
        _ => {
            let name: String = WEEKDAYS[t.weekday().num_days_from_monday() as usize]
                .chars()
                .take(3)
                .collect();
            format!("{name} {}", t.day())
        }
    }
}

/// "se reinicia hoy 21:40", "se reinicia jue 27 09:00".
fn reset_note(reset: Option<DateTime<Utc>>) -> String {
    let Some(t) = reset.map(|r| r.with_timezone(&Local)) else {
        return String::new();
    };
    let minutes = (t - Local::now()).num_minutes().max(0);
    let left = if minutes >= 24 * 60 {
        String::new()
    } else if minutes >= 60 {
        format!(" (en {}h {}m)", minutes / 60, minutes % 60)
    } else {
        format!(" (en {minutes}m)")
    };
    format!("Se reinicia {} {}{left}", day_label(t).to_lowercase(), t.format("%H:%M"))
}

#[expect(clippy::too_many_lines, reason = "One panel per widget")]
pub(super) fn blocks(id: WidgetId, r: &Readings) -> Vec<Block> {
    let mut b = Vec::new();
    match id {
        // Opens the Windows quick settings; never a panel of its own.
        WidgetId::Control => {}
        // Opens the Windows notification center; never a panel of its own.
        WidgetId::Notificaciones => {}
        // Drawn by musica_panel.
        WidgetId::Musica => {}
        WidgetId::Audio => {
            b.push(Block::Title("Audio".into(), String::new()));
            // The audio thread reads the lists; the panel never enumerates.
            let empty = Vec::new();
            for (heading, devices) in [
                ("Salida", r.audio.as_ref().map_or(&empty, |d| &d.outputs)),
                ("Entrada", r.audio.as_ref().map_or(&empty, |d| &d.inputs)),
            ] {
                b.push(Block::Section(heading.into()));
                if devices.is_empty() {
                    b.push(Block::Text("No hay dispositivos.".into(), true));
                    continue;
                }
                b.push(Block::Grid(
                    devices
                        .iter()
                        .map(|(id, name, default)| Cell {
                            title: name.clone(),
                            detail: if *default { "Predeterminado" } else { "" }.to_owned(),
                            on: *default,
                            action: Some(PanelAction::SetAudioDevice(id.clone())),
                        })
                        .collect(),
                ));
            }
            b.push(Block::Gap(4.0));
            let muted = r.audio.as_ref().and_then(|d| d.mic_muted);
            b.push(Block::Buttons(vec![(
                if muted == Some(true) { "Activar micrófono" } else { "Silenciar micrófono" }.into(),
                PanelAction::ToggleMic,
                muted != Some(true),
            )]));
        }
        WidgetId::Bandeja => {
            b.push(Block::Title("Segundo plano".into(), format!("{} apps", r.tray.len())));
            if r.tray.is_empty() {
                b.push(Block::Text("No hay programas con icono en la bandeja.".into(), true));
            } else {
                b.push(Block::Tray(r.tray.clone()));
                b.push(Block::Text("Clic para abrir · clic derecho para su menú.".into(), true));
            }
        }
        WidgetId::Sistema => {
            b.push(Block::Title("Sistema".into(), String::new()));
            let Some(d) = r.sistema else {
                b.push(Block::Text("Leyendo…".into(), true));
                return b;
            };
            match d.temp {
                Some(t) => b.push(Block::Meter {
                    label: "Temperatura de la CPU".into(),
                    percent: t.min(100),
                    color: level(if t >= 85 { 95 } else if t >= 70 { 75 } else { 10 }),
                    note: format!("{t} °C"),
                }),
                None => {
                    let (text, button) = match d.temp_why {
                        _ if d.installing => ("Instalando LibreHardwareMonitor…", None),
                        TempSource::NoLhm => (
                            "Para ver la temperatura de la CPU, Barra necesita LibreHardwareMonitor (gratuito, de código abierto). Windows pedirá permiso de administrador una vez: lee los sensores con un controlador.",
                            Some(("Instalar y activar", PanelAction::InstallLhm)),
                        ),
                        TempSource::LhmClosed => (
                            "LibreHardwareMonitor está instalado pero cerrado o sin su servidor web.",
                            Some(("Activar", PanelAction::StartLhm)),
                        ),
                        TempSource::NoSensor | TempSource::Ok => (
                            "El sensor responde pero no entrega la temperatura de la CPU de este equipo.",
                            None,
                        ),
                    };
                    b.push(Block::Text(text.into(), true));
                    if !d.installing && super::sistema::install_failed() {
                        b.push(Block::Text(
                            "No se pudo instalar con winget. Descárgalo de github.com/LibreHardwareMonitor/LibreHardwareMonitor.".into(),
                            true,
                        ));
                    }
                    if !d.installing && super::sistema::start_failed() {
                        b.push(Block::Text(
                            "No se encontró LibreHardwareMonitor. Pulsa Instalar y activar.".into(),
                            true,
                        ));
                    }
                    if let Some((label, action)) = button {
                        b.push(Block::Buttons(vec![(label.to_owned(), action, true)]));
                    }
                }
            }
            b.push(Block::Meter {
                label: "CPU".into(),
                percent: d.cpu,
                color: BLUE,
                note: String::new(),
            });
            b.push(Block::Meter {
                label: "Memoria".into(),
                percent: d.ram,
                color: GREEN,
                note: String::new(),
            });
            b.push(Block::Gap(4.0));
            b.push(Block::Buttons(vec![open("Administrador de tareas", "taskmgr.exe", true)]));
        }
        WidgetId::Memoria => {
            b.push(Block::Title("Memoria".into(), String::new()));
            let Some(d) = r.sistema else {
                b.push(Block::Text("Leyendo…".into(), true));
                return b;
            };
            let gb = super::cell::gb_text;
            b.push(Block::Meter {
                label: "En uso".into(),
                percent: d.ram,
                color: super::cell::ram_color(d.ram),
                note: format!("{} de {} GB", gb(d.ram_used_mb), gb(d.ram_total_mb)),
            });
            b.push(Block::Text(
                format!("Libre: {} GB", gb(d.ram_total_mb.saturating_sub(d.ram_used_mb))),
                true,
            ));
            b.push(Block::Gap(4.0));
            b.push(Block::Buttons(vec![open("Administrador de tareas", "taskmgr.exe", true)]));
        }
        WidgetId::Clima => {
            let Some(d) = r.clima.as_ref() else {
                b.push(Block::Title("Clima".into(), String::new()));
                b.push(Block::Text("Consultando el tiempo…".into(), true));
                return b;
            };
            b.push(Block::Title(d.city.clone(), "Open-Meteo".into()));
            b.push(Block::Big(
                Some(super::draw::symbol(d.code, d.day).to_owned()),
                format!("{}°", d.temp),
            ));
            b.push(Block::Text(super::clima::describe(d.code).into(), false));
            b.push(Block::Text(format!("Máx {}° · Mín {}°", d.max, d.min), false));
            let rain = if d.rain % 10 == 0 {
                format!("{}", d.rain / 10)
            } else {
                format!("{},{}", d.rain / 10, d.rain % 10)
            };
            b.push(Block::Text(format!("Viento {} km/h · Lluvia {rain} mm", d.wind), false));
            for (name, code, high, low) in &d.days {
                b.push(Block::Text(
                    format!("{name}: {high}° / {low}°  {}", super::draw::symbol(*code, true)),
                    false,
                ));
            }
            b.push(Block::Text(
                "La ciudad se cambia en Configuración › Widgets.".into(),
                true,
            ));
            b.push(Block::Buttons(vec![open("Abrir El Tiempo", "msnweather:", true)]));
        }
        // Drawn by calendario.rs.
        WidgetId::Reloj => {}
        WidgetId::Casa => {
            let Some(d) = r.casa.as_ref() else {
                b.push(Block::Title("Casa".into(), String::new()));
                b.push(Block::Text("Conectando con Home Assistant…".into(), true));
                return b;
            };
            match &d.state {
                Home::NotConnected => {
                    b.push(Block::Title("Casa".into(), "Home Assistant".into()));
                    b.push(Block::Section("Conecta Home Assistant".into()));
                    b.push(Block::Steps(vec![
                        "En Home Assistant abre tu perfil › Seguridad y crea un «Token de acceso de larga duración».".into(),
                        "En Configuración › Widgets › Casa escribe la dirección (por ejemplo http://192.168.1.10:8123) y pega el token.".into(),
                        "Elige los botones que quieres ver aquí.".into(),
                    ]));
                    b.push(Block::Buttons(vec![open(
                        "Cómo crear el token",
                        "https://www.home-assistant.io/docs/authentication/#your-account-profile",
                        true,
                    )]));
                }
                Home::Failed(error) => {
                    b.push(Block::Title("Casa".into(), "Home Assistant".into()));
                    b.push(Block::Text(error.clone(), false));
                }
                // Drawn by casa_panel.rs.
                Home::Connected => {}
            }
        }
        WidgetId::Claude | WidgetId::ChatGpt | WidgetId::Gemini => {
            let provider = id.provider().unwrap_or(Provider::Claude);
            let usage = r.ia.as_ref().map(|s| s.get(provider));
            let updated = usage
                .and_then(crate::usage::ProviderUsage::age_minutes)
                .map(|m| if m < 1 { "actualizado ahora".to_owned() } else { format!("hace {m} min") })
                .unwrap_or_default();
            b.push(Block::Title(id.name().into(), updated));
            match usage {
                None => b.push(Block::Text("Leyendo los límites…".into(), true)),
                Some(u) if u.state == State::SignIn => {
                    b.push(Block::Section("Conecta tus límites".into()));
                    let steps: Vec<String> = match provider {
                        Provider::Claude => vec![
                            "Barra lee el uso de tu plan con la sesión de Claude Code de este PC; no usa tu contraseña.".into(),
                            "Abre una terminal y ejecuta «claude». Si te lo pide, inicia sesión con tu cuenta Pro o Max.".into(),
                            "Cada vez que usas Claude Code la sesión se renueva y los límites aparecen aquí.".into(),
                        ],
                        Provider::Codex => vec![
                            "Instala Codex CLI: npm install -g @openai/codex".into(),
                            "Inicia sesión con tu cuenta de ChatGPT: codex login".into(),
                            "Úsalo una vez; los límites aparecen aquí solos.".into(),
                        ],
                        _ => vec![
                            "Instala Gemini CLI: npm install -g @google/gemini-cli".into(),
                            "Ejecuta «gemini» e inicia sesión con tu cuenta de Google.".into(),
                            "Los límites aparecen aquí solos. Si usas Antigravity, basta con tenerlo abierto.".into(),
                        ],
                    };
                    b.push(Block::Steps(steps));
                }
                Some(u) if u.windows.is_empty() => {
                    b.push(Block::Text("Sin datos por ahora. Prueba a actualizar en un rato.".into(), true));
                }
                Some(u) => {
                    for window in &u.windows {
                        b.push(Block::Meter {
                            label: match window.title().as_str() {
                                "5 h" => "Sesión (5 h)".to_owned(),
                                "7 d" => "Semanal".to_owned(),
                                other => other.to_owned(),
                            },
                            percent: window.used_percent,
                            color: level(window.used_percent),
                            note: reset_note(window.resets_at),
                        });
                    }
                }
            }
            b.push(Block::Gap(4.0));
            b.push(Block::Buttons(vec![
                ("Actualizar".into(), PanelAction::RefreshIa, false),
                open("Ver en la web", &id.target(), true),
            ]));
        }
    }
    b
}
