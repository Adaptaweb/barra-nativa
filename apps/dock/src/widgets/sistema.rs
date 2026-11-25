//! CPU load and memory from Windows every 2 seconds; CPU temperature every 6.
//!
//! Windows gives no CPU temperature without a kernel driver, so it comes from
//! LibreHardwareMonitor (its web server, Options › Remote Web Server, port
//! 8085) or from the Panel de proyectos fan helper (port 8786), whichever
//! answers. Both are on this computer; nothing leaves it.

use super::{SISTEMA_ON, SistemaData, publish};
use reqwest::blocking::Client;
use serde_json::Value;
use std::{sync::atomic::Ordering, thread, time::Duration};
use windows::Win32::{
    Foundation::FILETIME,
    System::{
        SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
        Threading::GetSystemTimes,
    },
};

const EVERY: Duration = Duration::from_secs(2);

fn ticks(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

/// Idle and total CPU time since boot, in 100 ns units.
fn cpu_times() -> Option<(u64, u64)> {
    let (mut idle, mut kernel, mut user) = Default::default();
    // SAFETY: The three outputs are live locals.
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()? };
    // Kernel time includes idle time.
    Some((ticks(idle), ticks(kernel) + ticks(user)))
}

fn memory_percent() -> u8 {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` is initialized with its size.
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() {
        status.dwMemoryLoad.min(100) as u8
    } else {
        0
    }
}

fn plausible(v: f64) -> bool {
    v > 0.0 && v < 130.0
}

fn number(v: &Value) -> Option<f64> {
    let text = v.as_str()?;
    let clean: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-'))
        .collect();
    clean.replace(',', ".").parse().ok()
}

/// CPU package temperature from LibreHardwareMonitor's sensor tree.
fn from_lhm(client: &Client) -> Option<f64> {
    let tree: Value = client
        .get("http://127.0.0.1:8085/data.json")
        .send()
        .ok()?
        .text()
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())?;
    let mut stack = vec![(&tree, false)];
    while let Some((node, in_cpu)) = stack.pop() {
        let text = node["Text"].as_str().unwrap_or("").to_lowercase();
        let is_cpu = in_cpu
            || ["ryzen", "intel core", "core i", "core ultra", "xeon", "threadripper", "athlon"]
                .iter()
                .any(|m| text.contains(m));
        if is_cpu && text == "temperatures" {
            let sensors: Vec<(String, f64)> = node["Children"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(|s| Some((s["Text"].as_str()?.to_owned(), number(&s["Value"])?)))
                        .collect()
                })
                .unwrap_or_default();
            for key in ["Core (Tctl/Tdie)", "CPU Package", "Core (Tdie)", "Tctl/Tdie", "Package"] {
                if let Some((_, v)) = sensors.iter().find(|(n, v)| n == key && plausible(*v)) {
                    return Some(*v);
                }
            }
            return sensors
                .iter()
                .map(|(_, v)| *v)
                .filter(|v| plausible(*v))
                .reduce(f64::max);
        }
        if let Some(children) = node["Children"].as_array() {
            stack.extend(children.iter().map(|c| (c, is_cpu)));
        }
    }
    None
}

/// The Panel de proyectos fan helper (LibreHardwareMonitorLib inside).
fn from_helper(client: &Client) -> Option<f64> {
    let v: Value = client
        .get("http://127.0.0.1:8786/api/estado")
        .send()
        .ok()?
        .text()
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())?;
    v["snap"]["cpu"].as_f64().filter(|t| plausible(*t))
}

fn temperature(client: &Client) -> Option<u8> {
    from_lhm(client)
        .or_else(|| from_helper(client))
        .map(|t| t.round().clamp(0.0, 255.0) as u8)
}

pub(super) fn start() {
    let spawned = thread::Builder::new()
        .name("barra-sistema".into())
        .spawn(|| {
            let client = Client::builder()
                .timeout(Duration::from_millis(800))
                .no_proxy()
                .build()
                .ok();
            let mut previous = cpu_times();
            let mut temp = None;
            let mut round = 0u32;
            while SISTEMA_ON.load(Ordering::Acquire) {
                thread::sleep(EVERY);
                let now = cpu_times();
                let cpu = match (previous, now) {
                    (Some((idle0, total0)), Some((idle1, total1))) if total1 > total0 => {
                        let busy = (total1 - total0).saturating_sub(idle1 - idle0);
                        (busy * 100 / (total1 - total0)).min(100) as u8
                    }
                    _ => 0,
                };
                previous = now;
                if round.is_multiple_of(3) {
                    temp = client.as_ref().and_then(temperature);
                }
                round = round.wrapping_add(1);
                let reading = SistemaData {
                    temp,
                    cpu,
                    ram: memory_percent(),
                };
                publish(|r| {
                    let changed = r.sistema != Some(reading);
                    r.sistema = Some(reading);
                    changed
                });
            }
        });
    if spawned.is_err() {
        SISTEMA_ON.store(false, Ordering::Release);
    }
}
