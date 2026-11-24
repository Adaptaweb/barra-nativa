//! The clock with the date and the next calendar event, in one widget.
//!
//! The time is published on every minute. Events come from private iCal links
//! (Google Calendar "secret address in iCal format", Outlook "publish
//! calendar"), one per line in the credential `Barra/calendario.ics`. They are
//! read every 15 minutes for the next 14 days.

use super::{Calendar, Event, RELOJ_ON, RelojData, publish, secrets};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use reqwest::blocking::Client;
use rrule::RRuleSet;
use std::{
    str::FromStr,
    sync::atomic::Ordering,
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

/// Upcoming events from every link, soonest first. `None` when no link answered.
fn fetch(client: &Client, links: &[String]) -> Option<Vec<Event>> {
    let now = Utc::now();
    let until = now + chrono::Duration::days(DAYS);
    let mut list: Vec<Event> = Vec::new();
    let mut any = false;
    for link in links {
        let url = link.trim().replacen("webcal://", "https://", 1);
        let Some(text) = client.get(&url).send().ok().and_then(|r| r.text().ok()) else {
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
            for start in occurrences(event, now, until) {
                if event.rrule.is_some()
                    && moved
                        .iter()
                        .any(|(uid, t)| *uid == event.uid && (*t - start).num_minutes().abs() < 1)
                {
                    continue;
                }
                let end = start + length;
                if end < now {
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
    list.truncate(20);
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

pub(super) fn start() {
    let spawned = thread::Builder::new()
        .name("barra-reloj".into())
        .spawn(|| {
            let client = Client::builder().timeout(Duration::from_secs(10)).build().ok();
            let mut calendar = Calendar::Loading;
            let mut events: Vec<Event> = Vec::new();
            let mut next_fetch = Instant::now();
            let mut previous_links: Vec<String> = Vec::new();
            while RELOJ_ON.load(Ordering::Acquire) {
                let links = links();
                if links != previous_links {
                    previous_links.clone_from(&links);
                    next_fetch = Instant::now();
                }
                if Instant::now() >= next_fetch {
                    if links.is_empty() {
                        calendar = Calendar::NotConnected;
                        events.clear();
                        next_fetch = Instant::now() + Duration::from_secs(60);
                    } else {
                        match client.as_ref().and_then(|c| fetch(c, &links)) {
                            Some(list) => {
                                calendar = Calendar::Connected;
                                events = list;
                                next_fetch = Instant::now() + EVERY;
                            }
                            None => {
                                calendar = Calendar::Failed;
                                next_fetch = Instant::now() + RETRY;
                            }
                        }
                    }
                }
                let now = Utc::now().timestamp();
                events.retain(|e| e.end > now);
                let reading = RelojData {
                    clock: muelle_shell::system::clock_text(),
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
                // Wake on the next minute.
                let seconds = 60 - u64::from(chrono::Timelike::second(&Local::now()));
                thread::sleep(Duration::from_secs(seconds.clamp(1, 60)) + Duration::from_millis(50));
            }
        });
    if spawned.is_err() {
        RELOJ_ON.store(false, Ordering::Release);
    }
}

fn weekday() -> String {
    let names = ["LUN", "MAR", "MIÉ", "JUE", "VIE", "SÁB", "DOM"];
    let index = chrono::Datelike::weekday(&Local::now()).num_days_from_monday() as usize;
    names[index].to_owned()
}
