//! CPU load and memory from Windows every 2 seconds; CPU temperature every 6.
//!
//! Windows gives no CPU temperature without a kernel driver, so it comes from
//! LibreHardwareMonitor (its web server, Options › Remote Web Server, port
//! 8085) or from the Panel de proyectos fan helper (port 8786), whichever
//! answers. Both are on this computer; nothing leaves it.

use super::{SISTEMA_ON, SistemaData, TempSource, publish};
use reqwest::blocking::Client;
use serde_json::Value;
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
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

/// Memory load in percent, and the physical memory in use and installed, in MiB.
fn memory() -> (u8, u32, u32) {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` is initialized with its size.
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() {
        let mib = |bytes: u64| u32::try_from(bytes / (1024 * 1024)).unwrap_or(u32::MAX);
        let total = status.ullTotalPhys;
        let used = total.saturating_sub(status.ullAvailPhys);
        (status.dwMemoryLoad.min(100) as u8, mib(used), mib(total))
    } else {
        (0, 0, 0)
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
/// `None`: it did not answer. `Some(None)`: it answered without a temperature.
fn from_lhm(client: &Client) -> Option<Option<f64>> {
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
                    return Some(Some(*v));
                }
            }
            return Some(
                sensors
                    .iter()
                    .map(|(_, v)| *v)
                    .filter(|v| plausible(*v))
                    .reduce(f64::max),
            );
        }
        if let Some(children) = node["Children"].as_array() {
            stack.extend(children.iter().map(|c| (c, is_cpu)));
        }
    }
    Some(None)
}

/// The helper's page carries its per-run token as `const TOKEN = "xxxx";`.
fn fetch_token(client: &Client) -> Option<String> {
    let page = client.get("http://127.0.0.1:8786/").send().ok()?.text().ok()?;
    let marker = "const TOKEN = \"";
    let start = page.find(marker)? + marker.len();
    let len = page[start..].find('"')?;
    Some(page[start..start + len].to_owned())
}

/// The Panel de proyectos fan helper (LibreHardwareMonitorLib inside).
/// `None`: it did not answer. `Some(None)`: it answered without a temperature.
fn from_helper(client: &Client, token: &mut Option<String>) -> Option<Option<f64>> {
    for _ in 0..2 {
        if token.is_none() {
            *token = fetch_token(client);
        }
        let current = token.as_deref()?;
        let response = client
            .get("http://127.0.0.1:8786/api/estado")
            .header("X-Token", current)
            .send()
            .ok()?;
        if response.status().as_u16() == 403 {
            // The helper restarted and has a new token.
            *token = None;
            continue;
        }
        let v: Value = serde_json::from_str(&response.text().ok()?).ok()?;
        return Some(v["snap"]["cpu"].as_f64().filter(|t| plausible(*t)));
    }
    None
}

/// The reading and whether any source answered at all.
fn temperature(client: &Client, token: &mut Option<String>) -> (Option<u8>, bool) {
    let to_u8 = |t: f64| t.round().clamp(0.0, 255.0) as u8;
    let helper = from_helper(client, token);
    if let Some(Some(t)) = helper {
        return (Some(to_u8(t)), true);
    }
    let lhm = from_lhm(client);
    (lhm.flatten().map(to_u8), helper.is_some() || lhm.is_some())
}

static INSTALLING: AtomicBool = AtomicBool::new(false);
static INSTALL_FAILED: AtomicBool = AtomicBool::new(false);
static START_FAILED: AtomicBool = AtomicBool::new(false);
/// Asks the reader thread for a fresh temperature reading and exe search.
static FORCE: AtomicBool = AtomicBool::new(false);

pub(super) fn start_failed() -> bool {
    START_FAILED.load(Ordering::Acquire)
}

pub(super) fn install_failed() -> bool {
    INSTALL_FAILED.load(Ordering::Acquire)
}

const LHM_EXE: &str = "LibreHardwareMonitor.exe";

/// `LibreHardwareMonitor.exe` inside `dir`, looking `depth` levels down.
fn find_exe(dir: &Path, depth: u32) -> Option<PathBuf> {
    let direct = dir.join(LHM_EXE);
    if direct.is_file() {
        return Some(direct);
    }
    if depth == 0 {
        return None;
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .find_map(|entry| find_exe(&entry.path(), depth - 1))
}

fn lhm_exe() -> Option<PathBuf> {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let packages = PathBuf::from(local).join(r"Microsoft\WinGet\Packages");
        let found = std::fs::read_dir(packages).ok().and_then(|entries| {
            entries
                .flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("LibreHardwareMonitor.LibreHardwareMonitor")
                })
                .find_map(|e| find_exe(&e.path(), 2))
        });
        if found.is_some() {
            return found;
        }
    }
    let files = std::env::var_os("ProgramFiles")?;
    let exe = PathBuf::from(files).join("LibreHardwareMonitor").join(LHM_EXE);
    exe.is_file().then_some(exe)
}

const LHM_SETTINGS: [(&str, &str); 6] = [
    ("runWebServerMenuItem", "true"),
    ("listenerPort", "8085"),
    ("startMinMenuItem", "true"),
    ("minTrayMenuItem", "true"),
    ("minCloseMenuItem", "true"),
    ("runOnWindowsStartupMenuItem", "true"),
];

/// Turns on LibreHardwareMonitor's web server and its start-minimized,
/// start-with-Windows options in the config file next to its exe.
fn configure_lhm(exe: &Path) {
    let Some(config) = exe.parent().map(|dir| dir.join("LibreHardwareMonitor.config")) else {
        return;
    };
    let mut text = match std::fs::read_to_string(&config) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<configuration>\n  <appSettings>\n  </appSettings>\n</configuration>\n"
                .to_owned()
        }
        // Unreadable or not UTF-8: leave the user's file alone.
        Err(_) => return,
    };
    if !text.contains("</appSettings>") {
        let empty = ["<appSettings />", "<appSettings/>"]
            .iter()
            .find_map(|tag| text.find(tag).map(|at| (at, tag.len())));
        if let Some((at, len)) = empty {
            text.replace_range(at..at + len, "<appSettings>\n  </appSettings>");
        } else if let Some(at) = text.find("</configuration>") {
            text.insert_str(at, "  <appSettings>\n  </appSettings>\n");
        } else {
            return;
        }
    }
    for (key, value) in LHM_SETTINGS {
        let entry = format!("<add key=\"{key}\" value=\"{value}\" />");
        let needle = format!("<add key=\"{key}\"");
        let span = text.find(&needle).and_then(|start| {
            let end = start + text[start..].find("/>")? + 2;
            Some(start..end)
        });
        match span {
            Some(range) => text.replace_range(range, &entry),
            None => {
                if let Some(at) = text.find("</appSettings>") {
                    text.insert_str(at, &format!("  {entry}\n"));
                }
            }
        }
    }
    let tmp = config.with_file_name("LibreHardwareMonitor.config.tmp");
    let written = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &config));
    if let Err(error) = written {
        eprintln!("muelle: sistema: no se pudo escribir {}: {error}", config.display());
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Starts LibreHardwareMonitor on its own thread: the UAC prompt blocks.
pub(super) fn start_lhm() {
    let spawned = thread::Builder::new()
        .name("barra-lhm-start".into())
        .spawn(start_lhm_now);
    if spawned.is_err() {
        START_FAILED.store(true, Ordering::Release);
    }
}

/// Configures and starts LibreHardwareMonitor as administrator (blocking).
fn start_lhm_now() {
    START_FAILED.store(false, Ordering::Release);
    let Some(exe) = lhm_exe() else {
        START_FAILED.store(true, Ordering::Release);
        FORCE.store(true, Ordering::Release);
        return;
    };
    configure_lhm(&exe);
    if let Err(error) = muelle_shell::launcher::open_as_admin(&exe.to_string_lossy()) {
        eprintln!("muelle: sistema: {error}");
    }
    FORCE.store(true, Ordering::Release);
}

/// Installs LibreHardwareMonitor with winget, then starts it.
pub(super) fn install_lhm() {
    if INSTALLING.swap(true, Ordering::AcqRel) {
        return;
    }
    INSTALL_FAILED.store(false, Ordering::Release);
    let spawned = thread::Builder::new()
        .name("barra-lhm-install".into())
        .spawn(|| {
            let ran = Command::new("winget")
                .args([
                    "install",
                    "--id",
                    "LibreHardwareMonitor.LibreHardwareMonitor",
                    "--exact",
                    "--silent",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                ])
                .creation_flags(0x0800_0000)
                .status();
            // An "already installed" exit code still leaves a usable exe.
            if ran.is_ok_and(|s| s.success()) || lhm_exe().is_some() {
                start_lhm_now();
            } else {
                INSTALL_FAILED.store(true, Ordering::Release);
            }
            INSTALLING.store(false, Ordering::Release);
            FORCE.store(true, Ordering::Release);
        });
    if spawned.is_err() {
        INSTALL_FAILED.store(true, Ordering::Release);
        INSTALLING.store(false, Ordering::Release);
    }
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
            let mut why = TempSource::NoLhm;
            let mut token = None;
            let mut exe_found = false;
            let mut searched: Option<Instant> = None;
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
                let forced = FORCE.swap(false, Ordering::AcqRel);
                if round.is_multiple_of(3) || forced {
                    let (value, answered) = client
                        .as_ref()
                        .map_or((None, false), |c| temperature(c, &mut token));
                    temp = value;
                    why = match (temp, answered) {
                        (Some(_), _) => TempSource::Ok,
                        (None, true) => TempSource::NoSensor,
                        (None, false) => {
                            // Looking for the exe walks folders: once a minute at most.
                            if forced || searched.is_none_or(|at| at.elapsed() >= Duration::from_secs(60)) {
                                exe_found = lhm_exe().is_some();
                                searched = Some(Instant::now());
                            }
                            if exe_found { TempSource::LhmClosed } else { TempSource::NoLhm }
                        }
                    };
                }
                round = round.wrapping_add(1);
                let (ram, ram_used_mb, ram_total_mb) = memory();
                let reading = SistemaData {
                    temp,
                    temp_why: why,
                    installing: INSTALLING.load(Ordering::Acquire),
                    cpu,
                    ram,
                    ram_used_mb,
                    ram_total_mb,
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
