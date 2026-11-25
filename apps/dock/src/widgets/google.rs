//! Google Calendar (API v3). Ajustes saves the OAuth client (`google.cliente`)
//! and a refresh token (`google.token`) in the Credential Manager; here the
//! token is exchanged for short-lived access tokens and every selected
//! calendar is read for a time range. Secrets are never logged.

use super::{AgendaEvent, secrets};
use chrono::{DateTime, Local, NaiveDate, SecondsFormat, TimeZone, Utc};
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::{Duration, Instant};

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const API: &str = "https://www.googleapis.com/calendar/v3";
const MAX_PAGES: usize = 4;

pub(super) enum Error {
    /// The refresh token was revoked or expired: Google must be connected again.
    Auth,
    /// Network or server error; worth retrying later.
    Other,
}

/// An access token and when it stops being valid.
pub(super) struct Session {
    access: String,
    expires: Instant,
}

/// True when both credentials exist.
pub(super) fn configured() -> bool {
    secrets::read("google.cliente").is_some() && secrets::read("google.token").is_some()
}

/// Percent-encodes everything but the unreserved characters.
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

/// `client_id` and `client_secret` from the saved OAuth client JSON.
fn client_keys(json: &str) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(json).ok()?;
    let section = value.get("installed").or_else(|| value.get("web"))?;
    Some((
        section.get("client_id")?.as_str()?.to_owned(),
        section.get("client_secret")?.as_str()?.to_owned(),
    ))
}

/// Exchanges the refresh token for a new access token.
fn refresh(client: &Client) -> Result<Session, Error> {
    let (Some(json), Some(token)) = (secrets::read("google.cliente"), secrets::read("google.token"))
    else {
        return Err(Error::Auth);
    };
    let (id, secret) = client_keys(&json).ok_or(Error::Auth)?;
    let body = format!(
        "client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token",
        encode(&id),
        encode(&secret),
        encode(token.trim())
    );
    let response = client
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .map_err(|_| Error::Other)?;
    let status = response.status();
    if status.as_u16() == 400 || status.as_u16() == 401 {
        return Err(Error::Auth);
    }
    if !status.is_success() {
        return Err(Error::Other);
    }
    let value: Value = serde_json::from_str(&response.text().map_err(|_| Error::Other)?)
        .map_err(|_| Error::Other)?;
    let access = value
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or(Error::Other)?
        .to_owned();
    let seconds = value.get("expires_in").and_then(Value::as_u64).unwrap_or(3600);
    Ok(Session {
        access,
        expires: Instant::now() + Duration::from_secs(seconds),
    })
}

/// GET with the bearer token. A 401 drops the session and retries once with a
/// fresh one; if that also fails the answer is `Auth`.
fn get(client: &Client, session: &mut Option<Session>, url: &str) -> Result<Value, Error> {
    for attempt in 0..2 {
        let stale = session
            .as_ref()
            .is_none_or(|s| s.expires <= Instant::now() + Duration::from_secs(60));
        if stale {
            *session = Some(refresh(client)?);
        }
        let access = session.as_ref().map(|s| s.access.clone()).ok_or(Error::Auth)?;
        let response = client
            .get(url)
            .header("Authorization", format!("Bearer {access}"))
            .send()
            .map_err(|_| Error::Other)?;
        let status = response.status();
        if status.as_u16() == 401 {
            *session = None;
            if attempt == 0 {
                continue;
            }
            return Err(Error::Auth);
        }
        if !status.is_success() {
            return Err(Error::Other);
        }
        return serde_json::from_str(&response.text().map_err(|_| Error::Other)?)
            .map_err(|_| Error::Other);
    }
    Err(Error::Auth)
}

/// "#rrggbb" to 0xRRGGBB.
fn color(text: &str) -> u32 {
    u32::from_str_radix(text.trim_start_matches('#'), 16).map_or(0, |c| c & 0x00FF_FFFF)
}

/// Unix seconds of a Google `start`/`end` object; `true` = all day.
fn moment(value: &Value) -> Option<(i64, bool)> {
    if let Some(text) = value.get("dateTime").and_then(Value::as_str) {
        return Some((DateTime::parse_from_rfc3339(text).ok()?.timestamp(), false));
    }
    let day = NaiveDate::parse_from_str(value.get("date")?.as_str()?, "%Y-%m-%d").ok()?;
    let local = Local.from_local_datetime(&day.and_hms_opt(0, 0, 0)?).earliest()?;
    Some((local.timestamp(), true))
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

/// Turns one API event into an agenda event; `None` if cancelled, declined
/// or without usable dates.
fn convert(item: &Value, calendar: &str, colour: u32) -> Option<AgendaEvent> {
    if item.get("status").and_then(Value::as_str) == Some("cancelled") {
        return None;
    }
    let declined = item
        .get("attendees")
        .and_then(Value::as_array)
        .is_some_and(|list| {
            list.iter().any(|a| {
                a.get("self").and_then(Value::as_bool) == Some(true)
                    && a.get("responseStatus").and_then(Value::as_str) == Some("declined")
            })
        });
    if declined {
        return None;
    }
    let (start, all_day) = moment(item.get("start")?)?;
    let end = item.get("end").and_then(moment).map_or(start, |(e, _)| e);
    let link = {
        let hangout = text(item, "hangoutLink");
        if hangout.is_empty() {
            let video = item
                .get("conferenceData")
                .and_then(|c| c.get("entryPoints"))
                .and_then(Value::as_array)
                .and_then(|points| {
                    points.iter().find(|p| {
                        p.get("entryPointType").and_then(Value::as_str) == Some("video")
                    })
                })
                .map(|p| text(p, "uri"))
                .unwrap_or_default();
            if video.is_empty() { text(item, "htmlLink") } else { video }
        } else {
            hangout
        }
    };
    let title = text(item, "summary");
    Some(AgendaEvent {
        title: if title.is_empty() { "(sin título)".to_owned() } else { title },
        start,
        end,
        all_day,
        location: text(item, "location"),
        link,
        calendar: calendar.to_owned(),
        color: colour,
    })
}

/// Events of every selected calendar between two Unix timestamps, by start.
pub(super) fn fetch(
    client: &Client,
    session: &mut Option<Session>,
    from: i64,
    to: i64,
) -> Result<Vec<AgendaEvent>, Error> {
    let min = Utc
        .timestamp_opt(from, 0)
        .single()
        .ok_or(Error::Other)?
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    let max = Utc
        .timestamp_opt(to, 0)
        .single()
        .ok_or(Error::Other)?
        .to_rfc3339_opts(SecondsFormat::Secs, true);

    let list = get(client, session, &format!("{API}/users/me/calendarList?minAccessRole=reader"))?;
    let calendars: Vec<(String, String, u32)> = list
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|c| {
                    c.get("selected").and_then(Value::as_bool).unwrap_or(false)
                        || c.get("primary").and_then(Value::as_bool).unwrap_or(false)
                })
                .filter_map(|c| {
                    let id = c.get("id")?.as_str()?.to_owned();
                    let name = c
                        .get("summaryOverride")
                        .and_then(Value::as_str)
                        .filter(|n| !n.is_empty())
                        .or_else(|| c.get("summary").and_then(Value::as_str))
                        .unwrap_or_default()
                        .to_owned();
                    let colour = c
                        .get("backgroundColor")
                        .and_then(Value::as_str)
                        .map_or(0, color);
                    Some((id, name, colour))
                })
                .collect()
        })
        .unwrap_or_default();

    let mut events: Vec<AgendaEvent> = Vec::new();
    for (id, name, colour) in &calendars {
        let mut page: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut url = format!(
                "{API}/calendars/{}/events?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime&maxResults=250",
                encode(id),
                encode(&min),
                encode(&max)
            );
            if let Some(token) = &page {
                url.push_str(&format!("&pageToken={}", encode(token)));
            }
            let answer = get(client, session, &url)?;
            if let Some(items) = answer.get("items").and_then(Value::as_array) {
                events.extend(items.iter().filter_map(|i| convert(i, name, *colour)));
            }
            page = answer
                .get("nextPageToken")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if page.is_none() {
                break;
            }
        }
    }
    events.sort_by_key(|e| e.start);
    Ok(events)
}
