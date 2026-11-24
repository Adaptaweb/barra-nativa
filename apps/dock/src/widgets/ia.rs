//! AI plan limits (Claude, ChatGPT through Codex, Gemini) for their widgets.
//! Muelle's own reader does the work in a short-lived child process; this
//! asks it for every provider every 5 minutes while one of these widgets is
//! placed, whether or not the provider's app is running.

use super::{IA_ON, publish};
use crate::usage;
use std::{sync::atomic::Ordering, thread, time::Duration};

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
                thread::sleep(EVERY);
            }
        });
    if spawned.is_err() {
        IA_ON.store(false, Ordering::Release);
    }
}
