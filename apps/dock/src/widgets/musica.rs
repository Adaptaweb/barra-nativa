//! What is playing, from the Windows media sessions, for the "Sonando ahora"
//! widget. It keeps its own watch while the widget is placed, so it works with
//! or without the menu bar. The reading is refreshed twice a second and only
//! published when something the widget shows changed (the position moves once
//! a second while playing).

use super::{MUSICA_ON, MusicaData, TARGET, TARGET_MESSAGE, publish};
use crate::media::{Control, Track, Watch};
use muelle_shell::icons::IconImage;
use std::{
    sync::{Arc, Mutex, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};
use windows::Win32::Foundation::HWND;

const EVERY: Duration = Duration::from_millis(500);

/// The colour of the card without artwork: a dark reddish tone.
pub(super) const NO_ARTWORK: [f32; 4] = [0.25, 0.12, 0.10, 1.0];

/// The second colour of the card without artwork: a slightly darker shade.
pub(super) const NO_ARTWORK_2: [f32; 4] = [0.14, 0.07, 0.12, 1.0];

/// Brightest the card may be, so white text stays readable.
const MAX_LUMINANCE: f32 = 0.35;

static WATCH: Mutex<Option<Arc<Watch>>> = Mutex::new(None);

pub(super) fn start() {
    let hwnd = HWND(TARGET.load(Ordering::Acquire) as *mut _);
    let Some(watch) = Watch::start_square(hwnd, TARGET_MESSAGE.load(Ordering::Acquire)) else {
        MUSICA_ON.store(false, Ordering::Release);
        return;
    };
    // The timeline is only read while someone asks for it.
    watch.show_progress(true);
    let watch = Arc::new(watch);
    if let Ok(mut slot) = WATCH.lock() {
        *slot = Some(watch.clone());
    }
    let spawned = thread::Builder::new().name("barra-musica".into()).spawn({
        let watch = watch.clone();
        move || {
            let mut cover: Option<(Arc<IconImage>, [[f32; 4]; 2])> = None;
            while MUSICA_ON.load(Ordering::Acquire) {
                let data = watch.snapshot().map(|track| reading(&track, &mut cover));
                publish(|r| {
                    let changed = r.musica != data;
                    r.musica = data;
                    changed
                });
                thread::sleep(EVERY);
            }
            if let Ok(mut slot) = WATCH.lock()
                && slot.as_ref().is_some_and(|current| Arc::ptr_eq(current, &watch))
            {
                *slot = None;
            }
            publish(|r| r.musica.take().is_some());
        }
    });
    if spawned.is_err() {
        MUSICA_ON.store(false, Ordering::Release);
        if let Ok(mut slot) = WATCH.lock() {
            *slot = None;
        }
    }
}

/// Sends a transport command to the current media session.
pub(super) fn control(control: Control) {
    let watch = WATCH.lock().ok().and_then(|slot| slot.clone());
    if let Some(watch) = watch
        && let Some(track) = watch.snapshot()
    {
        watch.send(&track, control);
    }
}

fn reading(track: &Track, cover: &mut Option<(Arc<IconImage>, [[f32; 4]; 2])>) -> MusicaData {
    let colors = match &track.artwork {
        Some(art) => {
            // Once per artwork, not on every tick.
            let known = cover
                .as_ref()
                .filter(|(known, _)| Arc::ptr_eq(known, art))
                .map(|(_, colors)| *colors);
            known.unwrap_or_else(|| {
                let colors = palette(art);
                *cover = Some((art.clone(), colors));
                colors
            })
        }
        None => [NO_ARTWORK, NO_ARTWORK_2],
    };
    let (duration, position) = track.timeline.map_or((0, 0), |timeline| {
        (
            timeline.duration.as_secs(),
            timeline.position_at(Instant::now()).as_secs(),
        )
    });
    MusicaData {
        title: track.title.clone(),
        artist: track.artist.clone(),
        playing: track.playing,
        can_prev: track.previous,
        can_toggle: track.toggle,
        can_next: track.next,
        art: track.artwork.clone(),
        colors,
        duration_s: u32::try_from(duration).unwrap_or(u32::MAX),
        position_s: u32::try_from(position).unwrap_or(u32::MAX),
    }
}

/// Running sums of one half of the artwork.
#[derive(Default)]
struct Half {
    colorful: [f32; 3],
    colorful_n: f32,
    all: [f32; 3],
    all_n: f32,
}

impl Half {
    /// The mean of the colourful pixels (of all of them if it is grey),
    /// darkened until white text reads on it. `None` without pixels.
    fn color(&self) -> Option<[f32; 4]> {
        let (sum, n) = if self.colorful_n > 0.0 {
            (self.colorful, self.colorful_n)
        } else {
            (self.all, self.all_n)
        };
        if n == 0.0 {
            return None;
        }
        let mut rgb = sum.map(|channel| (channel / n).clamp(0.0, 1.0));
        let luminance = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        if luminance > MAX_LUMINANCE {
            let factor = MAX_LUMINANCE / luminance;
            rgb = rgb.map(|channel| channel * factor);
        }
        Some([rgb[0], rgb[1], rgb[2], 1.0])
    }
}

/// The card's two colours from the artwork: the top left half and the bottom
/// right half (split by the anti-diagonal), for a diagonal gradient.
fn palette(image: &IconImage) -> [[f32; 4]; 2] {
    let mut halves = [Half::default(), Half::default()];
    let (width, height) = (image.width as usize, image.height as usize);
    let (chunks, _) = image.bgra.as_chunks::<4>();
    for (index, &[b, g, r, a]) in chunks.iter().enumerate() {
        // Premultiplied: skip the transparent corners, then undo the alpha.
        if a < 200 {
            continue;
        }
        let alpha = f32::from(a);
        let rgb = [
            f32::from(r) / alpha,
            f32::from(g) / alpha,
            f32::from(b) / alpha,
        ];
        let (x, y) = if width == 0 { (0, 0) } else { (index % width, index / width) };
        let half = &mut halves[usize::from(x * height + y * width >= width * height)];
        let high = rgb[0].max(rgb[1]).max(rgb[2]);
        let low = rgb[0].min(rgb[1]).min(rgb[2]);
        for (sum, value) in half.all.iter_mut().zip(rgb) {
            *sum += value;
        }
        half.all_n += 1.0;
        if high > 0.15 && (high - low) / high > 0.25 {
            for (sum, value) in half.colorful.iter_mut().zip(rgb) {
                *sum += value;
            }
            half.colorful_n += 1.0;
        }
    }
    let [first, second] = match (halves[0].color(), halves[1].color()) {
        (Some(first), Some(second)) => [first, second],
        (Some(only), None) | (None, Some(only)) => [only, only],
        (None, None) => return [NO_ARTWORK, NO_ARTWORK_2],
    };
    let apart: f32 = first.iter().zip(second).map(|(a, b)| (a - b).abs()).sum();
    if apart < 0.08 {
        // Nearly one colour: darken the second so the gradient shows.
        return [first, [first[0] * 0.6, first[1] * 0.6, first[2] * 0.6, 1.0]];
    }
    [first, second]
}
