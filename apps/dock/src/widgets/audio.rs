//! The microphone mute button and the output/input chooser. A thread reads
//! the default devices once a second and the full device lists every five
//! seconds (and right after a change), and only publishes when something the
//! widget shows changed. The panel never enumerates devices itself.

use super::{AUDIO_ON, AudioData, publish};
use muelle_shell::audio_devices;
use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    thread,
    time::Duration,
};

const EVERY: Duration = Duration::from_secs(1);
const LISTS_EVERY: u32 = 5;
const STEP: Duration = Duration::from_millis(100);

/// Generation of the reader thread: each `start()` bumps it, older threads exit.
static GEN: AtomicU64 = AtomicU64::new(0);
/// A mic toggle or default change is running; clicks meanwhile are ignored.
static BUSY: AtomicBool = AtomicBool::new(false);

pub(super) fn start() {
    let generation = GEN.fetch_add(1, Ordering::AcqRel) + 1;
    let alive = move || AUDIO_ON.load(Ordering::Acquire) && GEN.load(Ordering::Acquire) == generation;
    let spawned = thread::Builder::new().name("barra-audio".into()).spawn(move || {
        let mut seconds = 0_u32;
        while alive() {
            read(seconds.is_multiple_of(LISTS_EVERY));
            seconds = seconds.wrapping_add(1);
            let mut waited = Duration::ZERO;
            while waited < EVERY && alive() {
                thread::sleep(STEP);
                waited += STEP;
            }
        }
        // A newer thread owns the data now: only clear it when we are the last.
        if GEN.load(Ordering::Acquire) == generation {
            publish(|r| r.audio.take().is_some());
        }
    });
    if spawned.is_err() {
        AUDIO_ON.store(false, Ordering::Release);
    }
}

fn list(output: bool) -> Vec<(String, String, bool)> {
    audio_devices::list(output)
        .into_iter()
        .map(|d| (d.id, d.name, d.default))
        .collect()
}

/// Reads the defaults now (and the device lists if asked) and publishes them
/// if they changed. Without a list read, the last published lists are kept.
fn read(lists: bool) {
    let mic_muted = audio_devices::mic_muted();
    let output = audio_devices::default_name(true).unwrap_or_default();
    let input = audio_devices::default_name(false).unwrap_or_default();
    let fresh = lists.then(|| (list(true), list(false)));
    publish(|r| {
        let (outputs, inputs) = match (fresh, r.audio.as_ref()) {
            (Some(both), _) => both,
            (None, Some(old)) => (old.outputs.clone(), old.inputs.clone()),
            (None, None) => (Vec::new(), Vec::new()),
        };
        let data = AudioData { mic_muted, output, input, outputs, inputs };
        let changed = r.audio.as_ref() != Some(&data);
        r.audio = Some(data);
        changed
    });
}

/// Runs one operation on its own thread, one at a time: a click while another
/// is running is ignored.
fn exclusive(name: &str, job: impl FnOnce() + Send + 'static) {
    if BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let spawned = thread::Builder::new().name(name.into()).spawn(move || {
        job();
        BUSY.store(false, Ordering::Release);
    });
    if spawned.is_err() {
        BUSY.store(false, Ordering::Release);
    }
}

/// Mutes or unmutes the default microphone and repaints right away.
pub(super) fn toggle_mic() {
    // COM calls can block for a moment: not on the UI thread.
    exclusive("barra-audio-mic", || {
        if let Err(error) = audio_devices::toggle_mic() {
            eprintln!("muelle: audio: no se pudo cambiar el micrófono: {error}");
        }
        read(true);
    });
}

/// Makes a device the default (it can take a moment) and repaints.
pub(super) fn set_default(id: String) {
    exclusive("barra-audio-set", move || {
        if let Err(error) = audio_devices::set_default(&id) {
            eprintln!("muelle: audio: no se pudo cambiar el dispositivo: {error}");
        }
        read(true);
    });
}
