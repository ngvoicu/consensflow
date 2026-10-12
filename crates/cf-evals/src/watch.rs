//! Watch a live project from outside: what its tasks and windows are doing, read
//! from copies of the app's ledger and trace (the live ledger is never opened), the
//! way poker-lab was watched on 2026-10-03.
//!
//! ```text
//! cargo xtask watch --project poker-lab
//! cargo xtask watch --project 5 --minutes 30 --tokens
//! ```
//!
//! It says: the tasks not finished, and who holds each; the messages still on their
//! way; each window's changes of state in the last minutes (one that flips between
//! working and idle is flagged), the last one and why; the windows that stopped on a
//! prompt; deliveries held, retried or failed; members out of quota. With
//! `--tokens`, what each Claude window's own record says it used: its first turn's
//! context, what it read from the cache, wrote and output.

mod tokens;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use cf_e2e::live::machine;
use cf_e2e::process::own_var;
use jiff::tz::TimeZone;
use jiff::Timestamp;
use rusqlite::Connection;
use serde_json::Value;

use crate::cli::{self, Flag};
use crate::js::{first, pad_end, pad_start, squeeze};
use crate::measure::on_copy;
use crate::{clock, Error, Result};

/// The flags of `watch`.
const FLAGS: [Flag; 4] = [
    Flag::value("project"),
    Flag::or("minutes", "60"),
    Flag::switch("tokens"),
    Flag::value("home"),
];

/// A window flips when its state keeps changing within seconds, most of its changes
/// so: a chief that takes a message the moment its turn ends changes quickly too,
/// now and then, a misread window nearly every time.
const QUICK_MS: i64 = 5_000;
const FLIPPING: i64 = 10;

/// What a command line of `watch` asked for.
#[derive(Debug)]
struct Asked {
    /// The project, by its name or its id.
    project: String,
    /// How many minutes back the events are looked at, as it was written.
    minutes: String,
    tokens: bool,
    /// The ConsensFlow home to watch: the app's own, unless another is named.
    home: PathBuf,
}

fn asked(words: &[String]) -> Result<Asked> {
    let flags = cli::parse(&FLAGS, words)?;
    let Some(project) = flags.get("project").filter(|project| !project.is_empty()) else {
        return Err(Error::Usage(
            "usage: cargo xtask watch --project <name or id> [--minutes 60] [--tokens] [--home DIR]"
                .to_owned(),
        ));
    };
    let home = match flags.get("home") {
        Some(home) => PathBuf::from(home),
        None => match own_var("CONSENSFLOW_HOME").filter(|home| !home.is_empty()) {
            Some(home) => PathBuf::from(home),
            None => machine::home()?.join(".consensflow"),
        },
    };
    Ok(Asked {
        project: project.to_owned(),
        minutes: flags.get("minutes").unwrap_or("60").to_owned(),
        tokens: flags.has("tokens"),
        home,
    })
}

/// `watch`: says what the project is doing on `out`.
pub fn command(words: &[String], out: &mut dyn Write) -> Result<i32> {
    let asked = asked(words)?;
    // A number of minutes as JavaScript's `Number` reads one: what is not a number
    // is none, and the events of none are none.
    let minutes = asked.minutes.trim().parse::<f64>().unwrap_or(f64::NAN);
    watch(&asked, minutes, out)?;
    Ok(0)
}

/// The time of day an ISO time says it is, here: `09:30:00`, or `-` for none.
fn clock_of(at: Option<&str>) -> String {
    at.and_then(|at| at.parse::<Timestamp>().ok()).map_or_else(
        || "-".to_owned(),
        |at| {
            at.to_zoned(TimeZone::system())
                .strftime("%H:%M:%S")
                .to_string()
        },
    )
}

/// The project the ledger holds that `wanted` names: by its id, or by its name.
fn project_of(db: &Connection, wanted: &str) -> Result<(i64, String, String)> {
    let id = wanted
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|id| *id != 0.0 && id.fract() == 0.0)
        .map_or(-1, |id| id as i64);
    let mut statement =
        db.prepare("SELECT id, name, state FROM project WHERE id = ?1 OR name = ?2")?;
    let mut rows = statement.query_map(rusqlite::params![id, wanted], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.next()
        .transpose()?
        .ok_or_else(|| Error::Refused(format!("no project {wanted}")))
}

/// What the daemon's events of one window say of it so far.
#[derive(Debug, Default)]
struct Window {
    changes: i64,
    quick: i64,
    /// Its last event: when, and its state and the reason for it.
    last: Option<(Option<i64>, Value)>,
    prompts: Vec<Value>,
}

fn watch(asked: &Asked, minutes: f64, out: &mut dyn Write) -> Result {
    let file = asked.home.join("consensflow.db");
    let events = events_since(&asked.home.join("events.jsonl"), minutes)?;
    on_copy(&file, |db| {
        let (id, name, state) = project_of(db, &asked.project)?;
        let say = |out: &mut dyn Write, line: &str| -> Result { Ok(writeln!(out, "{line}")?) };
        say(
            out,
            &format!(
                "{name} (project {id}, {state}); the last {} minutes",
                asked.minutes
            ),
        )?;
        unfinished(db, id, out)?;
        on_their_way(db, id, out)?;
        let events: Vec<&Value> = events
            .iter()
            .filter(|event| event["project"].as_i64() == Some(id))
            .collect();
        windows(&events, out)?;
        deliveries(&events, out)?;
        out_of_quota(db, id, out)?;
        if asked.tokens {
            tokens::say(db, id, out)?;
        }
        Ok(())
    })
}

/// The events of the trace, a JSON object to a line, that are of the last
/// `minutes` minutes: with the time they happened, in milliseconds.
fn events_since(file: &Path, minutes: f64) -> Result<Vec<Value>> {
    let Ok(text) = fs::read_to_string(file) else {
        return Ok(Vec::new());
    };
    let since = Timestamp::now().as_millisecond() as f64 - minutes * 60_000.0;
    Ok(text
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| {
            event["at"]
                .as_str()
                .and_then(clock::millis)
                .is_some_and(|at| at as f64 >= since)
        })
        .collect())
}

/// The tasks that are not accepted or called off, and who holds each.
fn unfinished(db: &Connection, project: i64, out: &mut dyn Write) -> Result {
    writeln!(out, "\nTasks not finished")?;
    let mut statement = db.prepare(
        "SELECT t.number, t.state, a.handle, a.harness, t.updated_at, t.title
         FROM task t LEFT JOIN participant a ON a.id = t.assignee_id
         WHERE t.project_id = ?1 AND t.state NOT IN ('accepted', 'cancelled') ORDER BY t.number",
    )?;
    let rows = statement
        .query_map([project], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (number, state, assignee, harness, at, title) in rows {
        writeln!(
            out,
            "  T-{number} {} {} {} since {}  {}",
            pad_end(&state, 8),
            pad_end(assignee.as_deref().unwrap_or("-"), 26),
            pad_end(harness.as_deref().unwrap_or_default(), 12),
            clock_of(Some(&at)),
            first(&title, 50)
        )?;
    }
    Ok(())
}

/// The messages not yet delivered.
fn on_their_way(db: &Connection, project: i64, out: &mut dyn Write) -> Result {
    writeln!(out, "\nMessages on their way")?;
    let mut statement = db.prepare(
        "SELECT m.id, m.kind, m.state, s.handle, r.handle, t.number, m.created_at, m.body
         FROM message m LEFT JOIN participant s ON s.id = m.sender_id
         LEFT JOIN participant r ON r.id = m.recipient_id LEFT JOIN task t ON t.id = m.task_id
         WHERE m.project_id = ?1 AND m.state IN ('queued', 'delivering', 'gated') ORDER BY m.id",
    )?;
    let rows = statement
        .query_map([project], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (id, kind, state, sender, recipient, number, at, body) in rows {
        // A task number of 0 is no task, as JavaScript's truth has it.
        let task = number
            .filter(|number| *number != 0)
            .map(|number| format!(" T-{number}"))
            .unwrap_or_default();
        writeln!(
            out,
            "  m-{id} {kind} {state} {} -> {}{task} since {}: {}",
            sender.as_deref().unwrap_or("ConsensFlow"),
            recipient.as_deref().unwrap_or("null"),
            clock_of(Some(&at)),
            first(&squeeze(&body), 60)
        )?;
    }
    Ok(())
}

/// Each window's changes of state, the flipping ones flagged, and the prompts the
/// windows stopped on.
fn windows(events: &[&Value], out: &mut dyn Write) -> Result {
    // In the order each window was first heard of.
    let mut order: Vec<String> = Vec::new();
    let mut seen: BTreeMap<String, Window> = BTreeMap::new();
    for event in events
        .iter()
        .filter(|event| event["kind"] == "window.activity")
    {
        let handle = event["participant"]
            .as_str()
            .unwrap_or("undefined")
            .to_owned();
        if !seen.contains_key(&handle) {
            order.push(handle.clone());
        }
        let window = seen.entry(handle).or_default();
        window.changes += 1;
        let at = event["at"].as_str().and_then(clock::millis);
        if let (Some((Some(before), _)), Some(at)) = (&window.last, at) {
            if at - before < QUICK_MS {
                window.quick += 1;
            }
        }
        if event["state"] == "waiting" {
            window.prompts.push((*event).clone());
        }
        window.last = Some((at, (*event).clone()));
    }
    writeln!(out, "\nWindows")?;
    for handle in &order {
        let Some((window, (_, last))) = seen
            .get(handle)
            .and_then(|w| w.last.as_ref().map(|l| (w, l)))
        else {
            continue;
        };
        let reason = last["reason"]
            .as_str()
            .filter(|reason| !reason.is_empty())
            .map(|reason| format!(" ({reason})"))
            .unwrap_or_default();
        let flipping = window.quick >= FLIPPING && window.quick * 2 >= window.changes;
        writeln!(
            out,
            "  {} {} changes ({} within {} s), now {}{reason} since {}{}",
            pad_end(handle, 26),
            pad_start(&window.changes.to_string(), 4),
            window.quick,
            QUICK_MS / 1000,
            last["state"].as_str().unwrap_or("undefined"),
            clock_of(last["at"].as_str()),
            if flipping { "  FLIPPING" } else { "" }
        )?;
    }
    let prompts: Vec<String> = order
        .iter()
        .flat_map(|handle| {
            seen.get(handle)
                .into_iter()
                .flat_map(|window| window.prompts.iter())
                .map(move |event| {
                    format!(
                        "  {} {handle}: {}",
                        clock_of(event["at"].as_str()),
                        event["reason"].as_str().unwrap_or("waiting")
                    )
                })
        })
        .collect();
    if !prompts.is_empty() {
        writeln!(out, "\nStopped on a prompt")?;
        for line in prompts {
            writeln!(out, "{line}")?;
        }
    }
    Ok(())
}

/// The deliveries held, retried or failed.
fn deliveries(events: &[&Value], out: &mut dyn Write) -> Result {
    let held: Vec<&&Value> = events
        .iter()
        .filter(|event| {
            matches!(
                event["kind"].as_str(),
                Some("delivery.held" | "delivery.retried" | "delivery.failed")
            )
        })
        .collect();
    if held.is_empty() {
        return Ok(());
    }
    writeln!(out, "\nDeliveries held, retried or failed")?;
    let text = |value: &Value| match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    for event in held {
        let message = [&event["message"], &event["data"]["message"]]
            .into_iter()
            .find(|value| !value.is_null())
            .map_or_else(|| "undefined".to_owned(), text);
        let reason = [&event["reason"], &event["data"]["reason"]]
            .into_iter()
            .find(|value| !value.is_null())
            .map_or_else(String::new, text);
        writeln!(
            out,
            "  {} {} m-{message} {} {reason}",
            clock_of(event["at"].as_str()),
            pad_end(
                event["kind"]
                    .as_str()
                    .unwrap_or_default()
                    .get(9..)
                    .unwrap_or_default(),
                7
            ),
            text(&event["participant"]),
        )?;
    }
    Ok(())
}

/// The members who are out of quota now.
fn out_of_quota(db: &Connection, project: i64, out: &mut dyn Write) -> Result {
    let mut statement = db.prepare(
        "SELECT handle, out_until FROM participant
         WHERE project_id = ?1 AND out_until IS NOT NULL AND out_until > ?2",
    )?;
    let members = statement
        .query_map(rusqlite::params![project, clock::now()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if members.is_empty() {
        return Ok(());
    }
    writeln!(out, "\nOut of quota")?;
    for (handle, until) in members {
        writeln!(out, "  {handle} until {}", clock_of(Some(&until)))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
