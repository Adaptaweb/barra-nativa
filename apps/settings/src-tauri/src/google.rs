//! Connects a Google account (OAuth 2.0 desktop client, PKCE, loopback
//! redirect) so the dock can read Google Calendar. Only the refresh token is
//! kept, in the Windows Credential Manager next to the other Barra secrets.

use crate::barra::{checked, delete, read, write};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/calendar.readonly";
/// How long the browser has to come back with the authorisation.
const WAIT: Duration = Duration::from_secs(180);

/// Percent-encodes everything but the RFC 3986 unreserved characters.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Reverses `encode`; `+` is a space as in query strings.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = text.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok());
                if let Some(value) = hex {
                    out.push(value);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn random_text(bytes: usize) -> Result<String, String> {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).map_err(|_| "No pude generar números aleatorios".to_owned())?;
    Ok(URL_SAFE_NO_PAD.encode(buffer))
}

/// The client id and secret from the Google Cloud credentials JSON.
fn client() -> Result<(String, String), String> {
    let missing = || "Pega primero el JSON del cliente OAuth".to_owned();
    let json = read(&checked("google.cliente")?).ok_or_else(missing)?;
    let value: Value = serde_json::from_str(&json)
        .map_err(|_| "El JSON del cliente OAuth no es válido".to_owned())?;
    let section = value.get("installed").or_else(|| value.get("web")).ok_or_else(missing)?;
    let field = |name: &str| section[name].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    match (field("client_id"), field("client_secret")) {
        (Some(id), Some(secret)) => Ok((id, secret)),
        _ => Err("El JSON del cliente OAuth no trae client_id y client_secret".into()),
    }
}

fn respond(stream: &mut TcpStream, ok: bool, message: &str) {
    let title = if ok { "Barra" } else { "Barra: error" };
    let body = format!(
        "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\"><title>{title}</title></head>\
         <body style=\"font-family:Segoe UI,sans-serif;text-align:center;margin-top:20vh\">\
         <h2>{message}</h2></body></html>"
    );
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

/// Waits for the browser to be redirected to the loopback listener and returns
/// the authorisation code.
fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buffer = [0u8; 4096];
        let Ok(read) = stream.read(&mut buffer) else { continue };
        let request = String::from_utf8_lossy(&buffer[..read]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("GET "))
            .and_then(|rest| rest.split(' ').next())
            .unwrap_or_default();
        let query = target.split_once('?').map_or("", |(_, q)| q);
        let param = |name: &str| {
            query
                .split('&')
                .filter_map(|pair| pair.split_once('='))
                .find(|(key, _)| *key == name)
                .map(|(_, value)| decode(value))
        };
        let (code, error) = (param("code"), param("error"));
        if code.is_none() && error.is_none() {
            // Not the redirect (for example /favicon.ico).
            respond(&mut stream, false, "Esperando a Google…");
            continue;
        }
        if param("state").as_deref() != Some(state) {
            respond(&mut stream, false, "La respuesta no coincide con la solicitud.");
            return Err("La respuesta de Google no coincide con la solicitud".into());
        }
        if let Some(error) = error {
            respond(&mut stream, false, "Google no autorizó la conexión.");
            return Err(format!("Google no autorizó la conexión ({error})"));
        }
        if let Some(code) = code {
            respond(&mut stream, true, "Barra ya está conectada a Google. Puedes cerrar esta pestaña.");
            return Ok(code);
        }
    }
    Err("Tiempo agotado esperando a Google".into())
}

fn connect() -> Result<(), String> {
    let (client_id, client_secret) = client()?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");

    let verifier = random_text(48)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_text(16)?;

    let url = format!(
        "{AUTH_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}\
         &access_type=offline&prompt=consent&code_challenge={}&code_challenge_method=S256&state={}",
        encode(&client_id),
        encode(&redirect_uri),
        encode(SCOPE),
        encode(&challenge),
        encode(&state),
    );
    muelle_shell::launcher::open(&url).map_err(|_| "No pude abrir el navegador".to_owned())?;

    let code = wait_for_code(&listener, &state)?;

    let body = format!(
        "code={}&client_id={}&client_secret={}&code_verifier={}&redirect_uri={}&grant_type=authorization_code",
        encode(&code),
        encode(&client_id),
        encode(&client_secret),
        encode(&verifier),
        encode(&redirect_uri),
    );
    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "No pude preparar la conexión con Google".to_owned())?;
    let response = http
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .map_err(|_| "No pude contactar con Google".to_owned())?;
    let status = response.status();
    let text = response.text().map_err(|_| "Respuesta de Google ilegible".to_owned())?;
    let json: Value = serde_json::from_str(&text)
        .map_err(|_| format!("Respuesta de Google no válida (HTTP {status})"))?;
    let Some(refresh) = json["refresh_token"].as_str().filter(|t| !t.is_empty()) else {
        // Only Google's error code is reported, never the response body.
        let reason = json["error"].as_str().unwrap_or("sin refresh token");
        return Err(format!("Google no devolvió el permiso ({reason})"));
    };
    write(&checked("google.token")?, refresh)
}

/// Opens the browser, waits for the user to allow access and saves the refresh token.
#[tauri::command]
pub async fn google_connect() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(connect)
        .await
        .map_err(|_| "La conexión con Google se interrumpió".to_owned())?
}

/// Whether both the OAuth client and the refresh token are saved.
#[tauri::command]
pub fn google_status() -> bool {
    ["google.cliente", "google.token"].iter().all(|key| {
        checked(key)
            .ok()
            .and_then(|name| read(&name))
            .is_some_and(|value| !value.trim().is_empty())
    })
}

/// Forgets the refresh token and keeps the OAuth client.
#[tauri::command]
pub fn google_disconnect() -> Result<(), String> {
    delete(&checked("google.token")?);
    Ok(())
}
