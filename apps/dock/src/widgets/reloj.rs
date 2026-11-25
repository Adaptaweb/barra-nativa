//! The clock with the date and the next calendar event, in one widget.
//!
//! The time is published every second. Events come from Google Calendar (API,
//! see `google.rs`) when it is connected, otherwise from private iCal links
//! (Google Calendar "secret address in iCal format", Outlook "publish
//! calendar"), one per line in the credential `Barra/calendario.ics`. They are
//! read every 15 minutes, in a background thread. Both sources cover the same
//! three months (the agenda); the next event is the first one in the next 14 days.

use super::{Agenda, AgendaEvent, Calendar, Event, RELOJ_ON, RelojData, google, publish, secrets};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use reqwest::blocking::Client;
use rrule::RRuleSet;
use std::{
    str::FromStr,
    sync::{atomic::Ordering, mpsc},
    thread,
    time::{Duration, Instant},
};

const EVERY: Duration = Duration::from_secs(15 * 60);
const RETRY: Duration = Duration::from_secs(2 * 60);
const DAYS: i64 = 14;

#[derive(Default, Clone)]
struct Raw {
    uid: String,
    title: String,
    start: String,
    start_params: String,
    end: String,
    end_params: String,
    rrule: Option<String>,
    exdates: Vec<String>,
    recurrence: Option<String>,
    cancelled: bool,
}

/// Joins folded lines (RFC 5545: a line starting with a space continues the previous).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        match lines.last_mut() {
            Some(last) if line.starts_with(' ') || line.starts_with('\t') => {
                last.push_str(&line[1..]);
            }
            _ => lines.push(line.to_owned()),
        }
    }
    lines
}

fn unescape(s: &str) -> String {
    s.replace("\\n", " ")
        .replace("\\N", " ")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

fn parse(text: &str) -> Vec<Raw> {
    let mut events = Vec::new();
    let mut current: Option<Raw> = None;
    for line in unfold(text) {
        if line == "BEGIN:VEVENT" {
            current = Some(Raw::default());
            continue;
        }
        if line == "END:VEVENT" {
            events.extend(current.take());
            continue;
        }
        let Some(event) = current.as_mut() else {
            continue;
        };
        let Some((head, value)) = line.split_once(':') else {
            continue;
        };
        let (name, params) = head.split_once(';').unwrap_or((head, ""));
        match name {
            "UID" => event.uid = value.to_owned(),
            "SUMMARY" => event.title = unescape(value),
            "DTSTART" => {
                event.start = value.to_owned();
                event.start_params = params.to_owned();
            }
            "DTEND" => {
                event.end = value.to_owned();
                event.end_params = params.to_owned();
            }
            "RRULE" => event.rrule = Some(value.to_owned()),
            "EXDATE" => event.exdates.push(line.clone()),
            "RECURRENCE-ID" => event.recurrence = Some(value.to_owned()),
            "STATUS" => event.cancelled = value == "CANCELLED",
            _ => {}
        }
    }
    events
}

/// An iCal date or date-time with its parameters, in UTC; `true` = all day.
fn instant(value: &str, params: &str) -> Option<(DateTime<Utc>, bool)> {
    if (params.contains("VALUE=DATE") && !params.contains("VALUE=DATE-TIME")) || value.len() == 8 {
        let day = NaiveDate::parse_from_str(value, "%Y%m%d").ok()?;
        let local = Local
            .from_local_datetime(&day.and_hms_opt(0, 0, 0)?)
            .earliest()?;
        return Some((local.with_timezone(&Utc), true));
    }
    let naive = NaiveDateTime::parse_from_str(value.trim_end_matches('Z'), "%Y%m%dT%H%M%S").ok()?;
    if value.ends_with('Z') {
        return Some((Utc.from_utc_datetime(&naive), false));
    }
    if let Some(zone) = params
        .split(';')
        .find_map(|p| p.strip_prefix("TZID="))
        .and_then(|tz| chrono_tz::Tz::from_str(tz.trim_matches('"')).ok())
    {
        return Some((
            zone.from_local_datetime(&naive)
                .earliest()?
                .with_timezone(&Utc),
            false,
        ));
    }
    Some((
        Local
            .from_local_datetime(&naive)
            .earliest()?
            .with_timezone(&Utc),
        false,
    ))
}

/// Start times of an event in [from, to], expanding its RRULE if any.
fn occurrences(event: &Raw, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<DateTime<Utc>> {
    let Some((start, _)) = instant(&event.start, &event.start_params) else {
        return Vec::new();
    };
    let Some(rule) = &event.rrule else {
        return if start >= from - chrono::Duration::days(1) && start <= to {
            vec![start]
        } else {
            Vec::new()
        };
    };
    let mut text = format!("DTSTART:{}\nRRULE:{rule}", start.format("%Y%m%dT%H%M%SZ"));
    for line in &event.exdates {
        let (head, values) = line.split_once(':').unwrap_or(("", ""));
        let params = head.split_once(';').map_or("", |x| x.1);
        for value in values.split(',') {
            if let Some((time, _)) = instant(value, params) {
                text.push_str(&format!("\nEXDATE:{}", time.format("%Y%m%dT%H%M%SZ")));
            }
        }
    }
    let Ok(set) = RRuleSet::from_str(&text) else {
        return Vec::new();
    };
    set.after((from - chrono::Duration::days(1)).with_timezone(&rrule::Tz::UTC))
        .before(to.with_timezone(&rrule::Tz::UTC))
        .all(200)
        .dates
        .into_iter()
        .map(|d| d.with_timezone(&Utc))
        .collect()
}

/// Events between `from` and `to` (Unix seconds) from every link, soonest
/// first. `None` when no link answered.
fn fetch(client: &Client, links: &[String], from: i64, to: i64) -> Option<Vec<Event>> {
    let from = DateTime::from_timestamp(from, 0)?;
    let to = DateTime::from_timestamp(to, 0)?;
    let mut list: Vec<Event> = Vec::new();
    let mut any = false;
    for link in links {
        let url = link.trim().replacen("webcal://", "https://", 1);
        // A revoked or mistyped link answers 404: that link is not read.
        let Some(text) = client
            .get(&url)
            .send()
            .ok()
            .filter(|r| r.status().is_success())
            .and_then(|r| r.text().ok())
            .filter(|t| t.contains("BEGIN:VCALENDAR"))
        else {
            continue;
        };
        any = true;
        let events = parse(&text);
        // Edited single occurrences of a series replace the generated one.
        let moved: Vec<(String, DateTime<Utc>)> = events
            .iter()
            .filter_map(|e| Some((e.uid.clone(), instant(e.recurrence.as_ref()?, "")?.0)))
            .collect();
        for event in events.iter().filter(|e| !e.cancelled) {
            let Some((first, all_day)) = instant(&event.start, &event.start_params) else {
                continue;
            };
            let length = instant(&event.end, &event.end_params).map_or_else(
                || {
                    if all_day {
                        chrono::Duration::days(1)
                    } else {
                        chrono::Duration::hours(1)
                    }
                },
                |(end, _)| end - first,
            );
            for start in occurrences(event, from, to) {
                if event.rrule.is_some()
                    && moved
                        .iter()
                        .any(|(uid, t)| *uid == event.uid && (*t - start).num_minutes().abs() < 1)
                {
                    continue;
                }
                let end = start + length;
                if end < from {
                    continue;
                }
                list.push(Event {
                    title: if event.title.is_empty() {
                        "(Sin título)".to_owned()
                    } else {
                        event.title.clone()
                    },
                    start: start.timestamp(),
                    end: end.timestamp(),
                    all_day,
                });
            }
        }
    }
    if !any {
        return None;
    }
    list.sort_by_key(|e| e.start);
    list.truncate(500);
    Some(list)
}

fn links() -> Vec<String> {
    // `evento.ics` is where the first Barra editor saved the links.
    secrets::read("calendario.ics")
        .or_else(|| secrets::read("evento.ics"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// Result of one background read of the calendars.
struct Outcome {
    calendar: Calendar,
    /// `None` keeps the previous agenda (the read failed).
    agenda: Option<Agenda>,
    /// The Google session, handed back to the main loop for the next read.
    session: Option<google::Session>,
}

/// Unix seconds of the first day of the month at index `month` (year * 12 +
/// month0), at 00:00 local time.
fn month_start(month: i32) -> Option<i64> {
    let day = NaiveDate::from_ymd_opt(month.div_euclid(12), month.rem_euclid(12) as u32 + 1, 1)?
        .and_hms_opt(0, 0, 0)?;
    Some(
        Local
            .from_local_datetime(&day)
            .earliest()
            .map_or_else(|| day.and_utc().timestamp(), |d| d.timestamp()),
    )
}

/// From the 1st of the previous month to the 1st of the month after next.
fn agenda_range() -> (i64, i64) {
    use chrono::Datelike;
    let today = Local::now();
    let index = today.year() * 12 + today.month0() as i32;
    let now = today.timestamp();
    let month = 30 * 24 * 3600;
    (
        month_start(index - 1).unwrap_or(now - month),
        month_start(index + 2).unwrap_or(now + 2 * month),
    )
}

/// Reads Google Calendar when connected, the iCal links otherwise.
fn work(links: &[String], google_on: bool, mut session: Option<google::Session>) -> Outcome {
    let failed = |session| Outcome {
        calendar: Calendar::Failed,
        agenda: None,
        session,
    };
    let Ok(client) = Client::builder().timeout(Duration::from_secs(10)).build() else {
        return failed(session);
    };
    if google_on {
        let (from, to) = agenda_range();
        return match google::fetch(&client, &mut session, from, to) {
            Ok(events) => Outcome {
                calendar: Calendar::Connected,
                agenda: Some(Agenda { events }),
                session,
            },
            Err(google::Error::Auth | google::Error::Other) => failed(session),
        };
    }
    if links.is_empty() {
        return Outcome {
            calendar: Calendar::NotConnected,
            agenda: Some(Agenda::default()),
            session,
        };
    }
    let (from, to) = agenda_range();
    match fetch(&client, links, from, to) {
        Some(list) => Outcome {
            calendar: Calendar::Connected,
            agenda: Some(Agenda {
                events: list
                    .into_iter()
                    .map(|e| AgendaEvent {
                        title: e.title,
                        start: e.start,
                        end: e.end,
                        all_day: e.all_day,
                        location: String::new(),
                        link: String::new(),
                        calendar: "iCal".to_owned(),
                        color: 0,
                    })
                    .collect(),
            }),
            session,
        },
        None => failed(session),
    }
}

pub(super) fn start() {
    let spawned = thread::Builder::new()
        .name("barra-reloj".into())
        .spawn(|| {
            let mut calendar = Calendar::Loading;
            let mut events: Vec<Event> = Vec::new();
            let mut next_fetch = Instant::now();
            let mut previous_links: Vec<String> = Vec::new();
            let mut previous_google = false;
            let mut last_minute: Option<u32> = None;
            let mut session: Option<google::Session> = None;
            let mut fetching = false;
            let (sender, receiver) = mpsc::channel::<Outcome>();
            while RELOJ_ON.load(Ordering::Acquire) {
                // A finished background read.
                if let Ok(outcome) = receiver.try_recv() {
                    fetching = false;
                    session = outcome.session;
                    calendar = outcome.calendar;
                    next_fetch = Instant::now()
                        + match calendar {
                            Calendar::Connected => EVERY,
                            Calendar::NotConnected => Duration::from_secs(60),
                            _ => RETRY,
                        };
                    if let Some(agenda) = outcome.agenda {
                        let now = Utc::now().timestamp();
                        let horizon = now + DAYS * 24 * 3600;
                        events = agenda
                            .events
                            .iter()
                            .filter(|e| e.end > now && e.start < horizon)
                            .map(|e| Event {
                                title: e.title.clone(),
                                start: e.start,
                                end: e.end,
                                all_day: e.all_day,
                            })
                            .collect();
                        events.sort_by_key(|e| e.start);
                        events.truncate(20);
                        publish(|r| {
                            let changed = r.agenda.as_ref() != Some(&agenda);
                            r.agenda = Some(agenda.clone());
                            changed
                        });
                    }
                }
                // Credential Manager and the fetch check run only on the first
                // turn and when the minute changes.
                let minute = chrono::Timelike::minute(&Local::now());
                if last_minute != Some(minute) {
                    last_minute = Some(minute);
                    let links = links();
                    let google_on = google::configured();
                    if links != previous_links || google_on != previous_google {
                        previous_links.clone_from(&links);
                        previous_google = google_on;
                        next_fetch = Instant::now();
                    }
                    if !fetching && Instant::now() >= next_fetch {
                        // The read runs apart so the seconds keep ticking.
                        let sender = sender.clone();
                        let taken = session.take();
                        fetching = thread::Builder::new()
                            .name("barra-reloj-fetch".into())
                            .spawn(move || {
                                let _ = sender.send(work(&links, google_on, taken));
                            })
                            .is_ok();
                        if !fetching {
                            calendar = Calendar::Failed;
                            next_fetch = Instant::now() + RETRY;
                        }
                    }
                }
                let now = Utc::now().timestamp();
                events.retain(|e| e.end > now);
                let reading = RelojData {
                    clock: clock_24h(),
                    second: chrono::Timelike::second(&Local::now()),
                    day: Local::now().format("%d").to_string().trim_start_matches('0').to_owned(),
                    weekday: weekday(),
                    calendar,
                    next: events.first().cloned(),
                };
                publish(|r| {
                    let changed = r.reloj.as_ref() != Some(&reading);
                    r.reloj = Some(reading);
                    changed
                });
                // Wake on the next second.
                let subsec = u64::from(chrono::Timelike::nanosecond(&Local::now()) / 1_000_000 % 1000);
                thread::sleep(Duration::from_millis(1000 - subsec + 20));
            }
        });
    if spawned.is_err() {
        RELOJ_ON.store(false, Ordering::Release);
    }
}

/// "mié 25 nov 14:19": always 24 hours, whatever the regional format.
fn clock_24h() -> String {
    use chrono::{Datelike, Timelike};
    const DAYS: [&str; 7] = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];
    const MONTHS: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"];
    let now = Local::now();
    format!(
        "{} {} {}  {:02}:{:02}",
        DAYS[now.weekday().num_days_from_monday() as usize],
        now.day(),
        MONTHS[now.month0() as usize],
        now.hour(),
        now.minute()
    )
}

fn weekday() -> String {
    let names = ["LUN", "MAR", "MIÉ", "JUE", "VIE", "SÁB", "DOM"];
    let index = chrono::Datelike::weekday(&Local::now()).num_days_from_monday() as usize;
    names[index].to_owned()
}
