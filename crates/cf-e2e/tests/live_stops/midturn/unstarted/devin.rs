//! Devin's own files of the run, said: its wire log from the start (what its TUI and
//! its agent said to each other, and whether it ever names the conversation the
//! window shows, which the daemon waits for before it gives the window its first
//! message), what the product's reader makes of its store (`sessions.db`), and where
//! on the disk the store is.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use cf_e2e::live::ask::{self, Record};
use cf_e2e::live::members::Members;
use cf_e2e::{Error, Result};

use super::super::wire::{self, Wire};
use crate::watch::settlement_of;

/// How many of the first records of a wire log that names no conversation are said,
/// and how much of each.
const HEAD: usize = 4;
const HEAD_WIDTH: usize = 220;

/// What the wire log says: its runs, and the conversation it names, or what Devin said
/// first where it names none.
fn wire_lines(wire: &Wire) -> Vec<String> {
    let mut lines = vec![format!("its wire log: {}", wire.all())];
    match wire.session() {
        Some(session) => lines.push(format!("its wire log names the conversation {session}")),
        None => {
            lines.push(
                "its wire log names no conversation (no config_option_update of its mode option), which the daemon waits for; its first records:"
                    .to_owned(),
            );
            lines.extend(
                wire.head(HEAD, HEAD_WIDTH)
                    .into_iter()
                    .map(|record| format!("  {record}")),
            );
        }
    }
    lines
}

/// What the product's reader makes of the store: of the conversation the wire names
/// (`named`), or else of the newest the store holds of the run's folder.
fn store_line(
    named: Option<String>,
    newest: impl FnOnce() -> Result<Option<String>>,
    records: impl FnOnce(&str) -> Result<Option<Record>>,
) -> String {
    let session = match named {
        Some(session) => Ok(Some(session)),
        None => newest(),
    };
    match session {
        Err(failed) => format!("its store: the question of its conversation failed: {failed}"),
        Ok(None) => "its store holds no conversation of this folder".to_owned(),
        Ok(Some(session)) => match records(&session) {
            Err(failed) => {
                format!("its store, the conversation {session}: it could not be read: {failed}")
            }
            Ok(read) => format!(
                "its store, the conversation {session}: {}",
                settlement_of(read.as_ref())
            ),
        },
    }
}

/// A file that may be Devin's store, as a line: how big it is and how long ago it was
/// written, or that it is not there.
fn file_line(place: &Path, now: SystemTime) -> String {
    let Ok(found) = std::fs::metadata(place) else {
        return format!("{}: not there", place.display());
    };
    let written = found
        .modified()
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .map_or_else(
            || "at a time that is unknown".to_owned(),
            |ago| format!("{} s ago", ago.as_secs()),
        );
    format!(
        "{}: {} bytes, written {written}",
        place.display(),
        found.len()
    )
}

/// Where Devin may keep its store under the run's data folders. The rig's `HOME` can be
/// the person's, so its fallback is considered only when the home belongs to the run.
/// Refuse foreign data folders and links before reading a store's metadata.
fn store_files(env: &[(String, String)], now: SystemTime) -> Result<Vec<String>> {
    let var = |name: &str| {
        env.iter()
            .rev()
            .find(|(given, _)| {
                if cfg!(windows) {
                    given.eq_ignore_ascii_case(name)
                } else {
                    given == name
                }
            })
            .filter(|(_, value)| !value.is_empty())
            .map(|(_, value)| PathBuf::from(value))
    };
    let required = if cfg!(windows) {
        "APPDATA"
    } else {
        "XDG_DATA_HOME"
    };
    if var(required).is_none() {
        return Err(Error::Daemon(format!("the rig gives Devin no {required}")));
    }
    let home_data = var("HOME")
        .or_else(|| var("USERPROFILE"))
        .filter(|home| cf_e2e::door::is_made_or_below(home))
        .map(|home| home.join(".local").join("share"));
    let mut places: Vec<PathBuf> = Vec::new();
    for base in [
        var("APPDATA"),
        var("LOCALAPPDATA"),
        var("XDG_DATA_HOME"),
        home_data,
    ]
    .into_iter()
    .flatten()
    {
        if !cf_e2e::door::is_made_or_below(&base) {
            return Err(Error::Daemon(
                "a Devin data folder is outside the run's own roots".to_owned(),
            ));
        }
        let place = base.join("devin").join("cli").join("sessions.db");
        if !cf_e2e::door::is_made_or_below(&place) {
            return Err(Error::Daemon(
                "a Devin store is outside the run's own roots".to_owned(),
            ));
        }
        if !places.contains(&place) {
            places.push(place);
        }
    }
    Ok(places.iter().map(|place| file_line(place, now)).collect())
}

/// Devin's own files, said, for the member whose window is in `members`.
pub fn devin_lines(members: &Members) -> Result<Vec<String>> {
    let rig = members.rig();
    let home = rig.home();
    let env = rig.environment();
    // Check the store's environment before asking the adapter to read it.
    let stores = store_files(env, SystemTime::from(jiff::Timestamp::now()))?;
    let wire = Wire::open(&home);
    let mut lines = match &wire {
        Some(wire) => wire_lines(wire),
        None => vec![format!("its wire log: none; {}", wire::missing(&home))],
    };
    lines.push(store_line(
        wire.as_ref().and_then(Wire::session),
        || ask::newest_session("devin", members.folder(), 0, env),
        |session| ask::records("devin", session, env),
    ));
    lines.push("where its store may be:".to_owned());
    lines.extend(stores.into_iter().map(|place| format!("  {place}")));
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use cf_e2e::live::ask::Item;
    use serde_json::{json, Value};

    use super::*;

    fn wire_in(folder: &Path, records: &[Value]) -> Wire {
        let launch = folder.join("integrations").join("devin").join("launch-1");
        cf_e2e::door::make_dir(&launch).unwrap();
        let text: String = records.iter().map(|record| format!("{record}\n")).collect();
        cf_e2e::door::write(launch.join("wire.jsonl"), text).unwrap();
        Wire::open(folder).unwrap()
    }

    #[test]
    fn a_wire_that_names_its_conversation_says_so_and_one_that_names_none_says_what_devin_said_first(
    ) {
        let folder = cf_e2e::door::made("t-").unwrap();
        let chunk = json!({ "update": { "sessionUpdate": "agent_message_chunk" } });
        let named = json!({
            "sessionId": "calm-river",
            "update": { "sessionUpdate": "config_option_update", "configOptions": [{ "id": "mode" }] },
        });
        let wire = wire_in(folder.path(), &[named, chunk.clone()]);
        assert_eq!(
            wire_lines(&wire),
            [
                "its wire log: 2 wire records: config_option_update, agent_message_chunk"
                    .to_owned(),
                "its wire log names the conversation calm-river".to_owned(),
            ]
        );
        let hello = json!({ "method": "initialize" });
        let other = cf_e2e::door::made("t-").unwrap();
        let wire = wire_in(other.path(), &[hello.clone(), chunk.clone(), chunk]);
        let lines = wire_lines(&wire);
        assert_eq!(
            lines[0],
            "its wire log: 3 wire records: method:initialize, agent_message_chunk ×2"
        );
        assert!(
            lines[1].starts_with("its wire log names no conversation"),
            "{lines:?}"
        );
        assert_eq!(lines[2], format!("  {hello}"));
        assert_eq!(lines.len(), 5, "its first records, up to {HEAD}: {lines:?}");
    }

    fn record(settled: bool) -> Record {
        Record {
            items: vec![Item {
                id: "1".to_owned(),
                role: "user".to_owned(),
                text: "Write the numbers".to_owned(),
                complete: false,
            }],
            settled,
            settlement: "in-flight".to_owned(),
            failed: Value::Null,
            quota: Value::Null,
            agents_pending: false,
            jobs_pending: 0,
            jobs_retired: 0,
            monitors_noticed: 0,
        }
    }

    #[test]
    fn the_store_is_read_for_the_conversation_the_wire_names_and_else_the_newest_of_the_folder() {
        let asked = Cell::new(false);
        let said = store_line(
            Some("calm-river".to_owned()),
            || {
                asked.set(true);
                Ok(None)
            },
            |session| {
                assert_eq!(session, "calm-river");
                Ok(Some(record(false)))
            },
        );
        assert!(!asked.get(), "the wire named it: the store is not asked");
        assert_eq!(
            said,
            "its store, the conversation calm-river: the record reader (cf_harness::records, before the adapter's own look) reads its transcript as NOT settled (settlement in-flight; 1 items, the last \"user: Write the numbers\")"
        );
        let newest = store_line(
            None,
            || Ok(Some("brisk-ember".to_owned())),
            |session| {
                assert_eq!(session, "brisk-ember");
                Ok(None)
            },
        );
        assert_eq!(
            newest,
            "its store, the conversation brisk-ember: the record reader finds no transcript"
        );
    }

    #[test]
    fn a_store_that_holds_nothing_or_cannot_be_asked_or_read_is_said_so() {
        let nothing = store_line(None, || Ok(None), |_| Ok(None));
        assert_eq!(nothing, "its store holds no conversation of this folder");
        let failed = || Error::Daemon("harness-ask ended with 1".to_owned());
        assert_eq!(
            store_line(None, || Err(failed()), |_| Ok(None)),
            "its store: the question of its conversation failed: harness-ask ended with 1"
        );
        assert_eq!(
            store_line(Some("s".to_owned()), || Ok(None), |_| Err(failed())),
            "its store, the conversation s: it could not be read: harness-ask ended with 1"
        );
    }

    #[test]
    fn the_store_diagnostic_does_not_look_in_the_persons_home() {
        let run = cf_e2e::door::made("devin-store-diagnostic-").unwrap();
        let person = cf_e2e::door::made("t-").unwrap();
        let data = run.path().join("data");
        let at = |path: &Path| path.to_string_lossy().into_owned();
        let env = vec![
            ("XDG_DATA_HOME".to_owned(), at(&data)),
            ("APPDATA".to_owned(), at(&data)),
            ("HOME".to_owned(), at(person.path())),
        ];
        let now = SystemTime::UNIX_EPOCH;
        let lines = store_files(&env, now).unwrap();
        assert_eq!(
            lines.len(),
            1,
            "the person's home must not be inspected: {lines:?}"
        );
        assert!(lines[0].starts_with(&at(&data)));
        let foreign = [("XDG_DATA_HOME".to_owned(), at(person.path()))];
        assert!(store_files(&foreign, now).is_err());
        assert!(store_files(&[], now).is_err());
        #[cfg(unix)]
        {
            let other = person.path().join("sessions.db");
            cf_e2e::door::write(&other, b"foreign").unwrap();
            let link = data.join("devin/cli/sessions.db");
            cf_e2e::door::make_dir(link.parent().unwrap()).unwrap();
            cf_e2e::door::symlink(other, link).unwrap();
            assert!(store_files(&env, now).is_err());
        }
    }

    #[test]
    fn where_devins_store_may_be_is_each_place_the_run_names_with_what_is_there() {
        let folder = cf_e2e::door::made("devin-store-places-").unwrap();
        let roaming = folder.path().join("Roaming");
        let local = folder.path().join("Local");
        let store = roaming.join("devin").join("cli");
        cf_e2e::door::make_dir(&store).unwrap();
        cf_e2e::door::write(store.join("sessions.db"), b"12345").unwrap();
        // A time of whole seconds, which no file system rounds.
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        cf_e2e::door::set_modified(store.join("sessions.db"), now - Duration::from_secs(7))
            .unwrap();
        let at = |path: &Path| path.to_string_lossy().into_owned();
        let env = vec![
            ("APPDATA".to_owned(), at(&roaming)),
            ("LOCALAPPDATA".to_owned(), at(&local)),
            // The same place twice is said once, and a variable that is empty is none.
            ("XDG_DATA_HOME".to_owned(), at(&roaming)),
            ("HOME".to_owned(), String::new()),
        ];
        let lines = store_files(&env, now).unwrap();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].ends_with("sessions.db: 5 bytes, written 7 s ago"),
            "{lines:?}"
        );
        assert!(lines[0].starts_with(&at(&roaming)), "{lines:?}");
        assert_eq!(
            lines[1],
            format!(
                "{}: not there",
                local
                    .join("devin")
                    .join("cli")
                    .join("sessions.db")
                    .display()
            )
        );
    }
}
