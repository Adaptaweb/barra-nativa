//! Weather from Open-Meteo (no account, no key). Only the configured city name
//! and its coordinates are sent. Every 15 minutes; a minute after a failure.

use super::{CLIMA_ON, ClimaData, city, publish};
use chrono::{Datelike, NaiveDate};
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

const WEEKDAYS: [&str; 7] = ["Lun", "Mar", "Mié", "Jue", "Vie", "Sáb", "Dom"];

/// The weather in words for a WMO code.
pub(super) fn describe(code: u8) -> &'static str {
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
            "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,weather_code,is_day,wind_speed_10m&daily=temperature_2m_max,temperature_2m_min,precipitation_sum,weather_code&forecast_days=4&timezone=auto"
        ))
        .send()
        .ok()?
        .text()
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())?;
    let now = forecast.get("current")?;
    let daily = &forecast["daily"];
    let temp_now = now["temperature_2m"].as_f64()?.round() as i16;
    // The three days after today.
    let days = (1..4)
        .filter_map(|i| {
            let date = NaiveDate::parse_from_str(daily["time"][i].as_str()?, "%Y-%m-%d").ok()?;
            Some((
                WEEKDAYS[date.weekday().num_days_from_monday() as usize].to_owned(),
                daily["weather_code"][i].as_u64().unwrap_or(0).min(255) as u8,
                daily["temperature_2m_max"][i].as_f64()?.round() as i16,
                daily["temperature_2m_min"][i].as_f64()?.round() as i16,
            ))
        })
        .collect();
    Some(ClimaData {
        max: daily["temperature_2m_max"][0].as_f64().map_or(temp_now, |v| v.round() as i16),
        min: daily["temperature_2m_min"][0].as_f64().map_or(temp_now, |v| v.round() as i16),
        wind: now["wind_speed_10m"].as_f64().unwrap_or(0.0).round().max(0.0) as u16,
        rain: (daily["precipitation_sum"][0].as_f64().unwrap_or(0.0) * 10.0).round().max(0.0) as u16,
        days,
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
