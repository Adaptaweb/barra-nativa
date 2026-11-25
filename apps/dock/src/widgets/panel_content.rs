//! What each widget's panel shows. When a widget has nothing set up, its
//! panel explains how to set it up, step by step.

use super::panel::{Block, Cell, PanelAction};
use super::{Calendar, Home, Readings, WidgetId};
use crate::usage::{Provider, State};
use chrono::{DateTime, Local, TimeZone, Utc};

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

fn local(seconds: i64) -> Option<DateTime<Local>> {
    Local.timestamp_opt(seconds, 0).single()
}

const WEEKDAYS: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];
const MONTHS: [&str; 12] = [
    "enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre",
    "octubre", "noviembre", "diciembre",
];

fn long_date(t: DateTime<Local>) -> String {
    use chrono::Datelike;
    format!(
        "{}, {} de {}",
        WEEKDAYS[t.weekday().num_days_from_monday() as usize],
        t.day(),
        MONTHS[t.month0() as usize]
    )
}

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

fn weather_text(code: u8) -> &'static str {
    match code {
        0 => "Despejado",
        1 => "Mayormente despejado",
        2 => "Parcialmente nublado",
        3 => "Nublado",
        45 | 48 => "Niebla",
        51..=57 => "Llovizna",
        61..=67 => "Lluvia",
        71..=77 => "Nieve",
        80..=82 => "Chubascos",
        85 | 86 => "Chubascos de nieve",
        95..=99 => "Tormenta",
        _ => "",
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
                None => b.push(Block::Text(
                    "Para ver la temperatura de la CPU abre LibreHardwareMonitor con su servidor web activo (Options › Remote Web Server › Run, puerto 8085) o el ayudante de ventiladores del Panel de proyectos. Windows no la entrega sin un controlador.".into(),
                    true,
                )),
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
            b.push(Block::Text(weather_text(d.code).into(), false));
            b.push(Block::Text(
                "La ciudad se cambia en Configuración › Widgets.".into(),
                true,
            ));
            b.push(Block::Buttons(vec![open("Abrir El Tiempo", "msnweather:", true)]));
        }
        WidgetId::Reloj => {
            let now = Local::now();
            b.push(Block::Title(long_date(now), String::new()));
            b.push(Block::Big(None, now.format("%H:%M").to_string()));
            let Some(d) = r.reloj.as_ref() else {
                return b;
            };
            match d.calendar {
                Calendar::NotConnected => {
                    b.push(Block::Section("Conecta tu calendario".into()));
                    b.push(Block::Steps(vec![
                        "En Google Calendar abre Configuración y elige tu calendario a la izquierda.".into(),
                        "En «Integrar el calendario» copia la «Dirección secreta en formato iCal».".into(),
                        "Pégala en Configuración › Widgets › Calendario. En Outlook usa «Publicar calendario» y copia el enlace ICS.".into(),
                    ]));
                    b.push(Block::Buttons(vec![open(
                        "Abrir Google Calendar",
                        "https://calendar.google.com/calendar/r/settings",
                        true,
                    )]));
                }
                Calendar::Failed => {
                    b.push(Block::Text(
                        "No se pudo leer tu calendario: el enlace guardado ya no es válido (Google lo cambia si se restablece la dirección secreta) o no hay conexión.".into(),
                        false,
                    ));
                    b.push(Block::Steps(vec![
                        "En Google Calendar abre Configuración y elige tu calendario.".into(),
                        "En «Integrar el calendario» copia otra vez la «Dirección secreta en formato iCal».".into(),
                        "Pégala en Configuración › Widgets › Calendario.".into(),
                    ]));
                    b.push(Block::Buttons(vec![open(
                        "Abrir Google Calendar",
                        "https://calendar.google.com/calendar/r/settings",
                        true,
                    )]));
                }
                Calendar::Loading => b.push(Block::Text("Leyendo el calendario…".into(), true)),
                Calendar::Connected if d.upcoming.is_empty() => {
                    b.push(Block::Section("Próximos eventos".into()));
                    b.push(Block::Text("Nada en los próximos 14 días.".into(), true));
                }
                Calendar::Connected => {
                    b.push(Block::Section("Próximos eventos".into()));
                    let now = Utc::now().timestamp();
                    for event in &d.upcoming {
                        let Some(start) = local(event.start) else {
                            continue;
                        };
                        let when = if event.start <= now {
                            "Ahora".to_owned()
                        } else if event.all_day {
                            day_label(start)
                        } else {
                            format!("{} {}", day_label(start), start.format("%H:%M"))
                        };
                        let title: String = event.title.chars().take(30).collect();
                        b.push(Block::Event(when, title));
                    }
                }
            }
            b.push(Block::Gap(4.0));
            b.push(Block::Buttons(vec![
                open("Google Calendar", "https://calendar.google.com/", false),
                open("Notificaciones", "ms-actioncenter:", false),
            ]));
        }
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
                Home::Connected => {
                    let note = match d.on {
                        0 => "todo apagado".to_owned(),
                        1 => "1 encendido".to_owned(),
                        n => format!("{n} encendidos"),
                    };
                    b.push(Block::Title("Casa".into(), note));
                    b.push(Block::Grid(
                        d.buttons
                            .iter()
                            .map(|button| Cell {
                                title: button.name.clone(),
                                detail: button.value.clone().unwrap_or_else(|| {
                                    if button.on { "Encendido" } else { "Apagado" }.to_owned()
                                }),
                                on: button.on,
                                action: button
                                    .switchable
                                    .then(|| PanelAction::HomeToggle(button.id.clone())),
                            })
                            .collect(),
                    ));
                    let (url, _) = super::home_settings();
                    if !url.is_empty() {
                        b.push(Block::Buttons(vec![open("Abrir Home Assistant", &url, false)]));
                    }
                }
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
