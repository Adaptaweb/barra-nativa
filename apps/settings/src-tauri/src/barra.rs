//! Barra widget settings that are not plain preferences: secrets (calendar
//! links, the Home Assistant token) in the Windows Credential Manager, and
//! importing Home Assistant buttons saved by the Tauri Barra or the Panel de
//! proyectos.

use muelle_config::HomeItem;
use serde::Serialize;
use serde_json::Value;
use std::{env, fs, path::PathBuf};
use windows::{
    Win32::{
        Foundation::FILETIME,
        Security::Credentials::{
            CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW,
            CredFree, CredReadW, CredWriteW,
        },
    },
    core::{HSTRING, PCWSTR, PWSTR},
};

/// Only these secrets can be read or written from Settings.
const KEYS: [&str; 2] = ["calendario.ics", "casa.token"];

fn checked(key: &str) -> Result<HSTRING, String> {
    if KEYS.contains(&key) {
        Ok(HSTRING::from(format!("Barra/{key}")))
    } else {
        Err("clave no permitida".into())
    }
}

fn read(name: &HSTRING) -> Option<String> {
    // SAFETY: CredReadW fills `credential` on success; it is read and freed once.
    unsafe {
        let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
        CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None, &mut credential).ok()?;
        let found = &*credential;
        let bytes =
            std::slice::from_raw_parts(found.CredentialBlob, found.CredentialBlobSize as usize);
        let value = String::from_utf8(bytes.to_vec()).ok();
        CredFree(credential.cast());
        value
    }
}

fn write(name: &HSTRING, value: &str) -> Result<(), String> {
    let mut blob = value.as_bytes().to_vec();
    let user = HSTRING::from("barra");
    let credential = CREDENTIALW {
        Flags: CRED_FLAGS(0),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(name.as_ptr().cast_mut()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user.as_ptr().cast_mut()),
        LastWritten: FILETIME::default(),
        ..Default::default()
    };
    // SAFETY: Every pointer in `credential` lives until the call returns.
    unsafe { CredWriteW(&credential, 0) }.map_err(|error| error.to_string())
}

/// Whether a secret is saved; its value never goes back to the page.
#[tauri::command]
pub fn secret_saved(key: String) -> Result<bool, String> {
    Ok(read(&checked(&key)?).is_some_and(|v| !v.trim().is_empty()))
}

/// Saves a secret, or deletes it when `value` is empty.
#[tauri::command]
pub fn set_secret(key: String, value: String) -> Result<(), String> {
    let name = checked(&key)?;
    let value = value.trim();
    if value.is_empty() {
        // SAFETY: Deleting our own credential by name.
        unsafe {
            let _ = CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None);
        }
        return Ok(());
    }
    write(&name, value)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeImport {
    url: String,
    items: Vec<HomeItem>,
    /// Where it came from, to tell the user.
    source: String,
}

fn items_from(list: &[Value], favourites_only: bool) -> Vec<HomeItem> {
    let favourites: Vec<&Value> = list.iter().filter(|i| i["fav"].as_bool() == Some(true)).collect();
    let chosen: Vec<&Value> = if favourites_only && !favourites.is_empty() {
        favourites
    } else {
        list.iter().take(12).collect()
    };
    chosen
        .into_iter()
        .map(|item| HomeItem {
            id: item["id"].as_str().unwrap_or_default().to_owned(),
            name: item["nombre"].as_str().unwrap_or_default().to_owned(),
            kind: item["tipo"].as_str().unwrap_or_default().to_owned(),
            entities: item["entidades"]
                .as_array()
                .map(|v| v.iter().filter_map(|e| e.as_str().map(String::from)).collect())
                .unwrap_or_default(),
        })
        .collect()
}

fn json_at(path: PathBuf) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// Home Assistant address and buttons from the Tauri Barra, else from the
/// Panel de proyectos (whose token is then saved as the Barra secret).
#[tauri::command]
pub fn import_home() -> Result<HomeImport, String> {
    if let Some(config) = env::var_os("LOCALAPPDATA")
        .and_then(|d| json_at(PathBuf::from(d).join(r"dev.barra.app\config.json")))
    {
        let home = &config["ajustes"]["casa"];
        if let (Some(url), Some(list)) = (home["url"].as_str(), home["items"].as_array()) {
            return Ok(HomeImport {
                url: url.trim_end_matches('/').to_owned(),
                items: items_from(list, false),
                source: "Barra anterior".into(),
            });
        }
    }
    let profile = env::var_os("USERPROFILE").ok_or("Sin carpeta de usuario")?;
    let prefs = json_at(
        PathBuf::from(profile).join(r"Documents\Proyectos\panel-proyectos\preferencias.json"),
    )
    .ok_or("No encontré la Barra anterior ni el Panel de proyectos")?;
    let ha = &prefs["ha"];
    let url = ha["url"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("El Panel de proyectos no tiene la dirección de Home Assistant")?;
    if let Some(token) = ha["token"].as_str().filter(|s| !s.is_empty()) {
        write(&checked("casa.token")?, token)?;
    }
    Ok(HomeImport {
        url: url.trim_end_matches('/').to_owned(),
        items: items_from(ha["items"].as_array().map_or(&[][..], Vec::as_slice), true),
        source: "Panel de proyectos".into(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    device: String,
    label: String,
    primary: bool,
}

/// The connected displays, named by model: "Pantalla 2 · SAMSUNG".
#[tauri::command]
pub fn list_displays() -> Vec<DisplayInfo> {
    muelle_shell::displays::list()
        .into_iter()
        .map(|d| DisplayInfo {
            label: d.label(),
            device: d.device,
            primary: d.primary,
        })
        .collect()
}
