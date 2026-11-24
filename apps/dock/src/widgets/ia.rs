//! AI plan limits (Claude, ChatGPT through Codex, Gemini) for their widgets.
//! Muelle's own reader does the work in a short-lived child process; this
//! asks it for every provider every 5 minutes while one of these widgets is
//! placed, whether or not the provider's app is running.

use super::{IA_ON, publish};
use crate::usage;
use std::{
    sync::{Condvar, Mutex, atomic::Ordering},
    thread,
    time::Duration,
};

static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

/// Read again now (the panel's refresh button).
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

const EVERY: Duration = Duration::from_secs(5 * 60);

pub(super) fn start() {
    let spawned = thread::Builder::new()
        .name("barra-ia".into())
        .spawn(|| {
            while IA_ON.load(Ordering::Acquire) {
                let snapshot = usage::fetch_all_in_child();
                publish(|r| {
                    let changed = r.ia.as_ref() != Some(&snapshot);
                    r.ia = Some(snapshot);
                    changed
                });
                sleep(EVERY);
            }
        });
    if spawned.is_err() {
        IA_ON.store(false, Ordering::Release);
    }
}
