//! Follows Sunshine (Moonlight streaming) sessions from its log.
//!
//! Sunshine runs `muelle --stream on` when a session starts, but runs the
//! `off` command only when the streamed app ends, not when the client
//! disconnects: a session left open kept liquid glass off for good. So the
//! dock reads Sunshine's own log, which every user may read, and calls the
//! desktop streamed while the newest client event in it is a connection.
//!
//! The thread sleeps on a change notification for the log's folder: it wakes
//! only when Sunshine writes, and costs nothing otherwise.

use std::{
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    time::Duration,
};

use windows::{
    Win32::{
        Foundation::WAIT_OBJECT_0,
        Storage::FileSystem::{
            FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FindCloseChangeNotification,
            FindFirstChangeNotificationW, FindNextChangeNotification,
        },
        System::Threading::{INFINITE, WaitForSingleObject},
    },
    core::HSTRING,
};

/// How much of the log's end is read: a session's lines are few.
const TAIL: u64 = 64 * 1024;

/// Sunshine's configuration folder, which holds `sunshine.log`.
fn folder() -> Option<PathBuf> {
    let programs = std::env::var_os("ProgramFiles")?;
    let folder = PathBuf::from(programs).join("Sunshine").join("config");
    folder.join("sunshine.log").exists().then_some(folder)
}

/// Whether a client is connected now, by the newest client event in the log.
fn connected(log: &std::path::Path) -> Option<bool> {
    let mut file = std::fs::File::open(log).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(TAIL))).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let on = text.rfind("CLIENT CONNECTED");
    // The end of the streamed app ends the session too.
    let off = [text.rfind("CLIENT DISCONNECTED"), text.rfind("Executing Undo Cmd")]
        .into_iter()
        .flatten()
        .max();
    Some(match (on, off) {
        (Some(on), Some(off)) => on > off,
        (Some(_), None) => true,
        _ => false,
    })
}

/// Brings the streaming state in line with the log, if it differs.
fn sync(log: &std::path::Path) {
    let Some(now) = connected(log) else {
        return;
    };
    if now != crate::appearance::streaming_marked() {
        muelle_shell::log::line(if now {
            "Sunshine: cliente conectado, modo streaming"
        } else {
            "Sunshine: cliente desconectado, fin del modo streaming"
        });
        if let Err(error) = crate::appearance::set_streaming(now) {
            muelle_shell::log::line(&format!("modo streaming: {error:#}"));
        }
    }
}

/// Starts following Sunshine's log, if Sunshine is installed. Once per
/// machine session is enough: only the primary display's dock calls it.
pub fn spawn() {
    let Some(folder) = folder() else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("muelle-sunshine".into())
        .spawn(move || {
            let log = folder.join("sunshine.log");
            // A session that ended while the dock was closed.
            sync(&log);
            // SAFETY: The handle is waited on and closed by this thread only.
            unsafe {
                let Ok(change) = FindFirstChangeNotificationW(
                    &HSTRING::from(folder.as_os_str()),
                    false,
                    FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE,
                ) else {
                    return;
                };
                while WaitForSingleObject(change, INFINITE) == WAIT_OBJECT_0 {
                    // Let a burst of writes settle before reading.
                    std::thread::sleep(Duration::from_millis(500));
                    sync(&log);
                    if FindNextChangeNotification(change).is_err() {
                        break;
                    }
                }
                let _ = FindCloseChangeNotification(change);
            }
        });
}
