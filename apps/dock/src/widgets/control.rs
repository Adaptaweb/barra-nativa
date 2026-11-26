//! Network, volume, Bluetooth and battery for the control center widget,
//! read every 2 seconds on its own thread. A click opens the Windows quick
//! settings (Win+A), which has the switches and sliders.

use super::{CONTROL_ON, ControlData, publish};
use crate::bluetooth;
use muelle_shell::{
    status::{self, AudioWatch, Network, Volume},
    system::{self, Battery},
};
use std::{sync::atomic::Ordering, thread, time::Duration};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

const EVERY: Duration = Duration::from_secs(2);

pub(super) fn network_glyph(network: Network) -> char {
    match network {
        Network::Offline => '\u{F384}',
        Network::Ethernet => '\u{E839}',
        Network::Wifi { bars, .. } => ['\u{E872}', '\u{E873}', '\u{E874}', '\u{E701}'][usize::from(bars.clamp(1, 4) - 1)],
    }
}

pub(super) fn volume_glyph(volume: Option<Volume>) -> char {
    match volume {
        Some(v) if !v.muted => ['\u{E992}', '\u{E993}', '\u{E994}', '\u{E995}'][usize::from(v.level.min(3))],
        _ => '\u{E74F}',
    }
}

/// Segoe Fluent battery glyphs: ten steps, and the same with a plug.
pub(super) fn battery_glyph(b: Battery) -> char {
    let step = u32::from(b.percent.min(100)) / 10;
    match (step, b.charging) {
        (10, false) => '\u{E83F}',
        (10, true) => '\u{E83E}',
        (step, charging) => {
            char::from_u32(if charging { 0xE85A } else { 0xE850 } + step).unwrap_or('\u{E83F}')
        }
    }
}

/// The battery, or in debug builds a made-up one when `BARRA_FAKE_BATTERY` is
/// set (`72`, or `72c` charging) so a desktop can show the battery widget.
fn battery() -> Option<Battery> {
    if cfg!(debug_assertions)
        && let Ok(fake) = std::env::var("BARRA_FAKE_BATTERY")
    {
        let charging = fake.ends_with('c');
        let percent = fake.trim_end_matches('c').parse::<u8>().unwrap_or(72).min(100);
        return Some(Battery { percent, charging, saver: false, minutes_left: (!charging).then_some(250) });
    }
    system::battery()
}

pub(super) fn start() {
    let spawned = thread::Builder::new()
        .name("barra-control".into())
        .spawn(|| {
            // SAFETY: COM for this thread, released when it ends.
            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            // Window 0: the change callbacks post nowhere; this thread polls.
            let mut audio = AudioWatch::new(0, 0);
            let radio = bluetooth::Monitor::new(0, 0).ok();
            while CONTROL_ON.load(Ordering::Acquire) {
                let reading = ControlData {
                    network: status::network(),
                    volume: audio.as_mut().and_then(AudioWatch::volume),
                    battery: battery(),
                    bluetooth: radio.as_ref().is_some_and(bluetooth::Monitor::is_on),
                };
                publish(|r| {
                    let changed = r.control != Some(reading);
                    r.control = Some(reading);
                    changed
                });
                thread::sleep(EVERY);
            }
            drop(audio);
            drop(radio);
            if com.is_ok() {
                // SAFETY: Pairs with the successful CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
        });
    if spawned.is_err() {
        CONTROL_ON.store(false, Ordering::Release);
    }
}
