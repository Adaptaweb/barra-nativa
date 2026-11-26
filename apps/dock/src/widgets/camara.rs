//! The camera picture in the Casa panel. While the panel is open, one thread
//! asks go2rtc for a snapshot every 2 seconds. The address, user and stream
//! come from config.json (`widgets.cameraUrl`...); without them, from the
//! Panel de proyectos preferences on this PC. The password is the credential
//! `Barra/camara.clave`.

use super::{CamaraData, CameraStatus, publish, secrets};
use muelle_shell::{icons::IconImage, imagery};
use reqwest::blocking::Client;
use serde_json::Value;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    thread,
    time::Duration,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

const EVERY: Duration = Duration::from_secs(2);
/// Twice the width of the picture in the panel, for sharp high-DPI screens.
const MAX_WIDTH: u32 = 660;
/// Corner radius as a share of the picture's width (12 of 272 logical pixels).
const CORNER: f32 = 12.0 / 272.0;

/// Bumped each time the panel opens or closes; a thread ends when it is stale.
static GENERATION: AtomicU32 = AtomicU32::new(0);

struct Camera {
    url: String,
    user: String,
    password: Option<String>,
    source: String,
    name: String,
}

/// The camera of this app's settings, else the Panel de proyectos one.
/// Reads a file: not for the UI thread.
fn resolve() -> Option<Camera> {
    let mut camera = match super::settings().filter(|s| !s.camera_url.trim().is_empty()) {
        Some(s) => {
            let mut camera = Camera {
                url: s.camera_url.trim().trim_end_matches('/').to_owned(),
                user: s.camera_user.trim().to_owned(),
                password: None,
                source: s.camera_source.trim().to_owned(),
                name: s.camera_name.trim().to_owned(),
            };
            // No stream chosen: the Panel de proyectos one, when there is one.
            if camera.source.is_empty() {
                let fallback = from_panel_proyectos()?;
                camera.source = fallback.source;
                if camera.name.is_empty() {
                    camera.name = fallback.name;
                }
            }
            camera
        }
        None => from_panel_proyectos()?,
    };
    if camera.name.is_empty() {
        camera.name = "Cámara".to_owned();
    }
    Some(camera)
}

fn from_panel_proyectos() -> Option<Camera> {
    let path = std::path::PathBuf::from(std::env::var_os("USERPROFILE")?)
        .join(r"Documents\Proyectos\panel-proyectos\preferencias.json");
    let json: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let cameras = &json["camaras"];
    let first = cameras["lista"].as_array()?.first()?;
    let text = |value: &Value| value.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    Some(Camera {
        url: text(&cameras["go2rtc"])?.trim_end_matches('/').to_owned(),
        user: text(&cameras["usuario"]).unwrap_or_default(),
        password: text(&cameras["clave"]),
        source: text(&first["sd"]).or_else(|| text(&first["hd"]))?,
        name: text(&first["nombre"]).unwrap_or_default(),
    })
}

/// Percent-encodes a query value.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Center crop to 16:9 with antialiased rounded corners.
fn crop_wide(image: &IconImage) -> IconImage {
    let (width, height) = if u64::from(image.width) * 9 > u64::from(image.height) * 16 {
        ((u64::from(image.height) * 16 / 9) as u32, image.height)
    } else {
        (image.width, (u64::from(image.width) * 9 / 16) as u32)
    };
    let (left, top) = ((image.width - width) / 2, (image.height - height) / 2);
    let radius = (width as f32 * CORNER).min(height as f32 / 2.0);
    let (half_w, half_h) = (width as f32 / 2.0, height as f32 / 2.0);
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let offset = (((y + top) * image.width + x + left) * 4) as usize;
            let dx = ((x as f32 + 0.5 - half_w).abs() - (half_w - radius)).max(0.0);
            let dy = ((y as f32 + 0.5 - half_h).abs() - (half_h - radius)).max(0.0);
            let coverage = (radius - dx.hypot(dy) + 0.5).clamp(0.0, 1.0);
            for channel in &image.bgra[offset..offset + 4] {
                bgra.push((f32::from(*channel) * coverage).round() as u8);
            }
        }
    }
    IconImage { width, height, bgra }
}

/// One snapshot, or what to tell the user.
fn fetch(client: &Client, camera: &Camera) -> Result<IconImage, String> {
    let mut request = client.get(format!(
        "{}/api/frame.jpeg?src={}",
        camera.url,
        encode(&camera.source)
    ));
    if !camera.user.is_empty() {
        request = request.basic_auth(&camera.user, camera.password.as_deref());
    }
    let response = request.send().map_err(|_| "Sin conexión".to_owned())?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => return Err("Usuario o clave".to_owned()),
        code => return Err(format!("Error {code}")),
    }
    let bytes = response.bytes().map_err(|_| "Sin conexión".to_owned())?;
    imagery::decode_full(&bytes, MAX_WIDTH, 40_000_000)
        .map(|image| crop_wide(&image))
        .ok_or_else(|| "Imagen no válida".to_owned())
}

/// Stores what the panel shows, unless a newer open or a close came first.
/// `camera` is `None` before it is known: no name and no link yet.
fn store(generation: u32, camera: Option<&Camera>, status: CameraStatus, image: Option<Arc<IconImage>>) {
    publish(|r| {
        if GENERATION.load(Ordering::Acquire) != generation {
            return false;
        }
        let reading = CamaraData {
            status,
            image,
            name: camera.map(|c| c.name.clone()).unwrap_or_default(),
            page: camera.map_or_else(String::new, |c| {
                format!("{}/stream.html?src={}", c.url, encode(&c.source))
            }),
        };
        let changed = r.camara.as_ref() != Some(&reading);
        r.camara = Some(reading);
        changed
    });
}

/// Starts the snapshots; called when the Casa panel opens. The thread finds
/// the camera; without one, `readings.camara` is emptied again and the panel
/// skips the picture.
pub(super) fn open() {
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    store(generation, None, CameraStatus::Connecting, None);
    let _ = thread::Builder::new().name("barra-camara".into()).spawn(move || {
        // SAFETY: COM for this thread (the image decoder), released when it ends.
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        run(generation);
        if com.is_ok() {
            // SAFETY: Pairs with the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
    });
}

fn run(generation: u32) {
    let Some(mut camera) = resolve() else {
        publish(|r| GENERATION.load(Ordering::Acquire) == generation && r.camara.take().is_some());
        return;
    };
    if camera.password.is_none() {
        camera.password = secrets::read("camara.clave");
    }
    let Ok(client) = Client::builder()
        .timeout(Duration::from_secs(5))
        .no_proxy()
        .build()
    else {
        return;
    };
    let mut last: Option<Arc<IconImage>> = None;
    while GENERATION.load(Ordering::Acquire) == generation {
        let status = match fetch(&client, &camera) {
            Ok(image) => {
                last = Some(Arc::new(image));
                CameraStatus::Ok
            }
            Err(error) => CameraStatus::Error(error),
        };
        store(generation, Some(&camera), status, last.clone());
        thread::sleep(EVERY);
    }
}

/// Stops the snapshots; called when any panel closes.
pub(super) fn close() {
    GENERATION.fetch_add(1, Ordering::AcqRel);
    publish(|r| {
        r.camara = None;
        false
    });
}
