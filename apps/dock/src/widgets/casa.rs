//! Home Assistant: the state of the favourite buttons (lights, plugs, covers,
//! sensors). Address and buttons come from config.json (`widgets.homeUrl`,
//! `widgets.homeItems`); without them, from the Tauri Barra settings. The token
//! is the credential `Barra/casa.token`. Every 10 seconds.

use super::{CASA_ON, CasaData, Home, HomeButton, home_settings, publish, secrets};
use muelle_config::HomeItem;
use reqwest::blocking::Client;
use serde_json::Value;
use std::{collections::HashMap, sync::atomic::Ordering, thread, time::Duration};

const EVERY: Duration = Duration::from_secs(10);

/// The buttons saved by the Tauri Barra (`ajustes.casa`), when this app has none.
pub(super) fn from_tauri_barra() -> Option<(String, Vec<HomeItem>)> {
    let path = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
        .join(r"dev.barra.app\config.json");
    let config: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let home = &config["ajustes"]["casa"];
    let url = home["url"].as_str()?.trim_end_matches('/').to_owned();
    let items = home["items"]
        .as_array()?
        .iter()
        .map(|item| HomeItem {
            id: item["id"].as_str().unwrap_or_default().to_owned(),
            name: item["nombre"].as_str().unwrap_or_default().to_owned(),
            kind: item["tipo"].as_str().unwrap_or_default().to_owned(),
            entities: item["entidades"]
                .as_array()
                .map(|v| v.iter().filter_map(|e| e.as_str().map(String::from)).collect())
                .unwrap_or_default(),
        })
        .collect();
    Some((url, items))
}

fn states(client: &Client, url: &str, token: &str) -> Result<HashMap<String, Value>, String> {
    let response = client
        .get(format!("{url}/api/states"))
        .bearer_auth(token)
        .send()
        .map_err(|_| "No se pudo conectar con Home Assistant".to_owned())?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => return Err("Home Assistant rechazó el token".to_owned()),
        code => return Err(format!("Home Assistant respondió {code}")),
    }
    let list: Value = response
        .text()
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .ok_or_else(|| "Respuesta inesperada de Home Assistant".to_owned())?;
    Ok(list
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|e| Some((e["entity_id"].as_str()?.to_owned(), e.clone())))
                .collect()
        })
        .unwrap_or_default())
}

fn button(item: &HomeItem, states: &HashMap<String, Value>) -> HomeButton {
    let on = item.entities.iter().any(|id| {
        states
            .get(id)
            .and_then(|e| e["state"].as_str())
            .is_some_and(|s| matches!(s, "on" | "open" | "playing" | "unlocked" | "heat" | "cool"))
    });
    // A sensor-like first entity shows its value (temperature, humidity...).
    let value = item.entities.first().and_then(|id| states.get(id)).and_then(|e| {
        let state = e["state"].as_str()?;
        state.parse::<f64>().ok()?;
        let unit = e["attributes"]["unit_of_measurement"].as_str().unwrap_or("");
        Some(format!("{state} {unit}").trim().to_owned())
    });
    HomeButton {
        name: item.name.clone(),
        kind: item.kind.clone(),
        on,
        value,
    }
}

pub(super) fn start() {
    let spawned = thread::Builder::new()
        .name("barra-casa".into())
        .spawn(|| {
            let client = Client::builder()
                .timeout(Duration::from_secs(6))
                .no_proxy()
                .build()
                .ok();
            while CASA_ON.load(Ordering::Acquire) {
                let (url, items) = home_settings();
                let token = secrets::read("casa.token");
                let reading = match (url.is_empty(), token, client.as_ref()) {
                    (false, Some(token), Some(client)) => match states(client, &url, &token) {
                        Ok(states) => {
                            let buttons: Vec<HomeButton> =
                                items.iter().map(|item| button(item, &states)).collect();
                            CasaData {
                                state: Home::Connected,
                                on: buttons.iter().filter(|b| b.on).count() as u8,
                                buttons,
                            }
                        }
                        Err(error) => CasaData {
                            state: Home::Failed(error),
                            on: 0,
                            buttons: Vec::new(),
                        },
                    },
                    _ => CasaData {
                        state: Home::NotConnected,
                        on: 0,
                        buttons: Vec::new(),
                    },
                };
                publish(|r| {
                    let changed = r.casa.as_ref() != Some(&reading);
                    r.casa = Some(reading);
                    changed
                });
                thread::sleep(EVERY);
            }
        });
    if spawned.is_err() {
        CASA_ON.store(false, Ordering::Release);
    }
}
