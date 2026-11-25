//! Weather from Open-Meteo (no account, no key). Only the configured city name
//! and its coordinates are sent. Every 15 minutes; a minute after a failure.

use super::{CLIMA_ON, ClimaData, city, publish};
use reqwest::blocking::Client;
use serde_json::Value;
use std::{
    sync::{
        Condvar, Mutex,
        atomic::Ordering,
    },
    thread,
    time::Duration,
};

const EVERY: Duration = Duration::from_secs(15 * 60);
const RETRY: Duration = Duration::from_secs(60);

static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

/// Fetch now (the city changed).
pub(super) fn wake() {
    if let Ok(mut flag) = WAKE.0.lock() {
        *flag = true;
        WAKE.1.notify_all();
    }
}

fn sleep(duration: Duration) {
    let Ok(guard) = WAKE.0.lock() else {
        thread::sleep(duration);
        return;
    };
    if let Ok((mut guard, _)) = WAKE.1.wait_timeout_while(guard, duration, |woken| !*woken) {
        *guard = false;
    }
}

fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.trim().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn fetch(client: &Client, city: &str) -> Option<ClimaData> {
    let places: Value = client
        .get(format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=es",
            encode(city)
        ))
        .send()
        .ok()?
        .text()
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())?;
    let place = places.get("results")?.get(0)?;
    let (lat, lon) = (place["latitude"].as_f64()?, place["longitude"].as_f64()?);
    let name = place["name"].as_str().unwrap_or(city).to_owned();
    let forecast: Value = client
        .get(format!(
            "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,weather_code,is_day&hourly=temperature_2m,weather_code&daily=temperature_2m_max,temperature_2m_min&forecast_days=2&timezone=auto"
        ))
        .send()
        .ok()?
        .text()
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())?;
    let now = forecast.get("current")?;
    // Hourly lists start at local midnight; take the hours after this one.
    let this_hour = now["time"].as_str().and_then(|t| t.get(..13)).unwrap_or_default();
    let hourly = &forecast["hourly"];
    let times = hourly["time"].as_array().cloned().unwrap_or_default();
    let start = times
        .iter()
        .position(|t| t.as_str().is_some_and(|t| t.starts_with(this_hour)))
        .unwrap_or(0);
    let hours = (start + 1..(start + 7).min(times.len()))
        .filter_map(|i| {
            Some((
                times[i].as_str()?.get(11..13)?.to_owned(),
                hourly["weather_code"][i].as_u64().unwrap_or(0).min(255) as u8,
                hourly["temperature_2m"][i].as_f64()?.round() as i16,
            ))
        })
        .collect();
    let daily = &forecast["daily"];
    let temp_now = now["temperature_2m"].as_f64()?.round() as i16;
    Some(ClimaData {
        max: daily["temperature_2m_max"][0].as_f64().map_or(temp_now, |v| v.round() as i16),
        min: daily["temperature_2m_min"][0].as_f64().map_or(temp_now, |v| v.round() as i16),
        hours,
        city: name,
        temp: now["temperature_2m"].as_f64()?.round() as i16,
        code: now["weather_code"].as_u64().unwrap_or(0).min(255) as u8,
        day: now["is_day"].as_i64().unwrap_or(1) == 1,
    })
}

pub(super) fn start() {
    let spawned = thread::Builder::new()
        .name("barra-clima".into())
        .spawn(|| {
            let Ok(client) = Client::builder().timeout(Duration::from_secs(8)).build() else {
                CLIMA_ON.store(false, Ordering::Release);
                return;
            };
            while CLIMA_ON.load(Ordering::Acquire) {
                let city = city();
                let wait = match (!city.trim().is_empty())
                    .then(|| fetch(&client, &city))
                    .flatten()
                {
                    Some(reading) => {
                        publish(|r| {
                            let changed = r.clima.as_ref() != Some(&reading);
                            r.clima = Some(reading);
                            changed
                        });
                        EVERY
                    }
                    None => RETRY,
                };
                sleep(wait);
            }
        });
    if spawned.is_err() {
        CLIMA_ON.store(false, Ordering::Release);
    }
}
