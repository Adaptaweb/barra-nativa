mod io;
pub(super) mod protocol;

use super::{WorkerJob, worker::Event};
use anyhow::{Context, Result, ensure};
use muelle_shell::accessories::{Device, Kind};
use protocol::{Model, Reading};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    Devices::HumanInterfaceDevice::HidDevice,
    Win32::{
        Foundation::{HANDLE, WAIT_OBJECT_0},
        System::{JobObjects::AssignProcessToJobObject, Threading::WaitForSingleObject},
    },
};

pub const FLAG: &str = "--accessory-usb-query";
const INTERVAL: Duration = Duration::from_secs(60);
const LIMIT: usize = 8;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    path: String,
}

pub fn query_main() -> Result<()> {
    let mut input = String::new();
    std::io::stdin().take(4097).read_to_string(&mut input)?;
    ensure!(input.len() <= 4096, "USB request too large");
    let request: Request = serde_json::from_str(&input)?;
    let readings = io::query(&request.path)?;
    serde_json::to_writer(std::io::stdout().lock(), &readings)?;
    Ok(())
}

pub fn selector() -> Result<String> {
    protocol::PROFILES
        .iter()
        .map(|profile| {
            HidDevice::GetDeviceSelectorVidPid(
                profile.page,
                profile.usage,
                profile.vendor,
                profile.product,
            )
            .map(|query| format!("({query})"))
            .map_err(Into::into)
        })
        .collect::<Result<Vec<_>>>()
        .map(|queries| queries.join(" OR "))
}

fn probe(path: &str) -> Result<Vec<Reading>> {
    let job = WorkerJob::new()?;
    let mut child = Command::new(std::env::current_exe()?)
        .arg(FLAG)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    // The non-inherited job also cleans up if the watcher process crashes.
    unsafe {
        if let Err(error) = AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())) {
            let _ = child.kill();
            return Err(error.into());
        }
    }
    query_child(&mut child, path, 2000)
}

fn query_child(child: &mut Child, path: &str, timeout: u32) -> Result<Vec<Reading>> {
    if let Some(mut input) = child.stdin.take() {
        serde_json::to_writer(&mut input, &Request { path: path.into() })?;
        input.flush()?;
    }
    ensure!(
        unsafe { WaitForSingleObject(HANDLE(child.as_raw_handle()), timeout) } == WAIT_OBJECT_0,
        "USB provider exceeded process deadline"
    );
    ensure!(child.wait()?.success(), "USB provider failed");
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .context("USB output unavailable")?
        .take(4097)
        .read_to_end(&mut output)?;
    ensure!(output.len() <= 4096, "USB output too large");
    let readings: Vec<Reading> = serde_json::from_slice(&output)?;
    let profile = protocol::profile_path(path).context("unknown USB response target")?;
    let mut slots = std::collections::BTreeSet::new();
    ensure!(
        readings.len() <= 6
            && readings.iter().all(|r| slots.insert(r.slot)
                && r.percent.is_none_or(|n| n <= 100)
                && if profile.model == Model::Logitech {
                    (1..=6).contains(&r.slot)
                } else {
                    r.slot == 0 && r.kind == Kind::Headset
                }),
        "invalid USB response"
    );
    Ok(readings)
}

pub(super) struct Completed {
    pub path: String,
    pub generation: u64,
    pub result: Result<Vec<Reading>>,
}
struct Job {
    path: String,
    generation: u64,
}
struct Entry {
    id: String,
    generation: u64,
    next: Instant,
    failures: u8,
    devices: Vec<Device>,
}
#[derive(Default)]
pub struct Sources {
    entries: BTreeMap<String, Entry>,
    generation: u64,
    in_flight: Option<(String, u64)>,
    pool: Option<SyncSender<Job>>,
}
impl Sources {
    pub fn add(&mut self, path: String, id: String) {
        if protocol::profile_path(&path).is_none()
            || self.entries.contains_key(&path)
            || self.entries.len() >= LIMIT
        {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.entries.insert(
            path,
            Entry {
                id,
                generation: self.generation,
                next: Instant::now(),
                failures: 0,
                devices: Vec::new(),
            },
        );
    }
    pub fn remove(&mut self, path: &str) {
        self.entries.remove(path);
        if self.entries.is_empty() && self.in_flight.is_none() {
            self.pool = None;
        }
    }
    pub fn schedule(&mut self, tx: &SyncSender<Event>, full: &Arc<AtomicBool>) -> Result<()> {
        // Start no thread or USB query process unless an allowlisted interface exists.
        if self.entries.is_empty() {
            if self.in_flight.is_none() {
                self.pool = None;
            }
            return Ok(());
        }
        if self.pool.is_none() {
            let (jobs, receive) = mpsc::sync_channel::<Job>(1);
            let (tx, full) = (tx.clone(), full.clone());
            thread::Builder::new()
                .name("muelle-usb-query".into())
                .spawn(move || {
                    while let Ok(job) = receive.recv() {
                        let result = probe(&job.path);
                        if tx
                            .try_send(Event::Usb(Completed {
                                path: job.path,
                                generation: job.generation,
                                result,
                            }))
                            .is_err()
                        {
                            full.store(true, Ordering::Release);
                            break;
                        }
                    }
                })?;
            self.pool = Some(jobs);
        }
        // Exactly one in-flight operation. Neither hotplug nor metadata noise
        // can create an unbounded work queue or override its rate limit.
        if self.in_flight.is_some() {
            return Ok(());
        }
        if let Some((path, entry)) = self
            .entries
            .iter_mut()
            .filter(|(_, entry)| entry.next <= Instant::now())
            .min_by_key(|(_, entry)| entry.next)
            && let Some(pool) = &self.pool
        {
            pool.try_send(Job {
                path: path.clone(),
                generation: entry.generation,
            })?;
            self.in_flight = Some((path.clone(), entry.generation));
        }
        Ok(())
    }
    pub fn deadline(&self) -> Option<Instant> {
        if self.in_flight.is_some() {
            None
        } else {
            self.entries.values().map(|entry| entry.next).min()
        }
    }
    pub fn complete(&mut self, response: Completed) {
        if self.in_flight.as_ref().is_some_and(|(path, generation)| {
            *path == response.path && *generation == response.generation
        }) {
            self.in_flight = None;
        }
        if self.entries.is_empty() && self.in_flight.is_none() {
            self.pool = None;
        }
        let Some(entry) = self
            .entries
            .get_mut(&response.path)
            .filter(|entry| entry.generation == response.generation)
        else {
            return;
        };
        entry.devices.clear();
        match response.result {
            Ok(readings) => {
                entry.failures = 0;
                entry.devices = readings
                    .into_iter()
                    .map(|reading| Device {
                        id: if reading.slot == 0 {
                            entry.id.clone()
                        } else {
                            format!("{}/{}", entry.id, reading.slot)
                        },
                        name: if reading.kind == Kind::Mouse {
                            "Logitech HID++".into()
                        } else {
                            "Wireless headset".into()
                        },
                        kind: reading.kind,
                        percent: reading.percent,
                        conflicting: false,
                        active_audio: false,
                    })
                    .collect();
            }
            Err(_) => entry.failures = entry.failures.saturating_add(1),
        }
        entry.next = Instant::now() + retry_delay(entry.failures);
    }
    pub fn devices(&self) -> impl Iterator<Item = &Device> {
        self.entries.values().flat_map(|entry| &entry.devices)
    }
}
fn retry_delay(failures: u8) -> Duration {
    if failures == 0 {
        INTERVAL
    } else {
        Duration::from_secs((60u64 << failures.min(4)).min(900))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hotplug_noise_does_not_override_rate_limits_and_removal_clears_readings() {
        let path = r"\\?\hid#vid_1038&pid_22a1&mi_03#instance";
        let mut sources = Sources::default();
        assert!(sources.deadline().is_none() && sources.pool.is_none());
        sources.add(path.into(), "headset".into());
        let generation = sources.entries[path].generation;
        sources.complete(Completed {
            path: path.into(),
            generation,
            result: Ok(vec![Reading {
                slot: 0,
                kind: Kind::Headset,
                percent: Some(0),
            }]),
        });
        let next = sources.deadline();
        sources.add(path.into(), "headset".into());
        assert_eq!(sources.deadline(), next);
        assert_eq!(
            sources.devices().next().and_then(|device| device.percent),
            Some(0)
        );
        sources.complete(Completed {
            path: path.into(),
            generation,
            result: Err(anyhow::anyhow!("timeout")),
        });
        assert_eq!(sources.devices().count(), 0);
        sources.remove(path);
        assert!(sources.deadline().is_none());
    }

    #[test]
    #[ignore = "Native fixture, run only as the child of the deadline test"]
    fn stalled_usb_fixture() {
        thread::sleep(Duration::from_secs(30));
    }

    #[test]
    #[ignore = "Starts and terminates its own stalled test child; no hardware access"]
    fn usb_process_deadline_terminates_a_stalled_provider() -> Result<()> {
        let job = WorkerJob::new()?;
        let mut child = Command::new(std::env::current_exe()?)
            .args([
                "accessories::usb::tests::stalled_usb_fixture",
                "--exact",
                "--ignored",
            ])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let handle = HANDLE(child.as_raw_handle());
        unsafe {
            AssignProcessToJobObject(job.0, handle)?;
        }
        let start = Instant::now();
        assert!(query_child(&mut child, "fixture", 100).is_err());
        drop(job);
        assert_eq!(unsafe { WaitForSingleObject(handle, 1000) }, WAIT_OBJECT_0);
        assert!(start.elapsed() < Duration::from_secs(2));
        Ok(())
    }

    #[test]
    fn unplug_replug_rejects_late_results_and_backoff_is_bounded() {
        let path = r"\\?\hid#vid_046d&pid_c52b&mi_02#instance";
        let mut sources = Sources::default();
        sources.add(path.into(), "receiver-a".into());
        let generation = sources.entries[path].generation;
        sources.remove(path);
        sources.add(path.into(), "receiver-a".into());
        sources.complete(Completed {
            path: path.into(),
            generation,
            result: Ok(vec![Reading {
                slot: 1,
                kind: Kind::Mouse,
                percent: Some(50),
            }]),
        });
        assert_eq!(sources.devices().count(), 0);
        assert_eq!(retry_delay(0), Duration::from_secs(60));
        assert!(retry_delay(1) >= Duration::from_secs(60));
        assert_eq!(retry_delay(255), Duration::from_secs(900));
    }
}
