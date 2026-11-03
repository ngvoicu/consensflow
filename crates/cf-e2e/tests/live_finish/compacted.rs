//! The third step, Devin's alone: a worker whose turn Devin's compactor compacts before it
//! answers (T-655, from the live board's T-602 of 2026-10-26). The compactor begins a
//! chain of its own below a summary, the steps it keeps copied below it with the ids
//! Devin gave them, and the task's brief, the user's message the turn answers, is left in
//! the store outside the main chain: a turn's end that looked for the brief on the chain
//! found none, and the task stayed working for good with its window at rest.
//!
//! Devin compacts a session once its context passes `agent.compaction_threshold_tokens` of
//! its config (an integer from 1 up, Devin 3000.11.205). The run writes the machine's Devin
//! config, read as a launch reads it, with the threshold at [`THRESHOLD`] tokens, into the
//! config folder (`XDG_CONFIG_HOME`) of the home the kit gives Devin's windows
//! (`live::isolation`), beside the kit's copy of the login; the machine's own folder is only
//! read. The task is several calls of the worker's shell, so that the compactor runs in
//! the middle of the turn: two probes of 2026-10-26 on SWE-2 compacted three times in five
//! calls, in about 30 s. The run checks what the first step checks (the task done, its
//! result at the chief with the command's output), and that Devin did compact the turn: a
//! session whose main chain holds no compaction's summary proves nothing, and fails the
//! run.
//!
//! On Windows Devin keeps its config in its data folder, which `XDG_CONFIG_HOME` does not
//! move, so the step is not run there.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::devin_store;
use cf_e2e::live::ask;
use cf_e2e::live::machine::{home, live_folder};
use cf_e2e::live::members::{self, Plan};
use cf_e2e::live::{Harness, Log, Who};
use cf_e2e::process::own_var;
use cf_e2e::{Error, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};

use crate::check::{self, Finished};
use crate::task::Task;

/// The context, in tokens, past which the worker's Devin compacts its session.
pub const THRESHOLD: u64 = 1000;

/// How many times the worker runs the command, each in a call of its own.
const CALLS: usize = 5;

/// The project of the run: the stand-in chief and the Devin worker, in a folder of its
/// own, with `config` as Devin's config in the home of the run's own its windows run on.
pub fn plan(config: &Value) -> Plan {
    Plan::new("finish-devin-compacted", members::stand_in_chief())
        .with(members::member(Harness::Devin))
        .with_home_file(
            Harness::Devin,
            "XDG_CONFIG_HOME",
            "devin/config.json",
            format!("{config}\n"),
        )
}

/// Devin's config for the run: the machine's, its compaction threshold at [`THRESHOLD`].
fn config() -> Result<Value> {
    let machine = own_var("XDG_CONFIG_HOME")
        .filter(|folder| !folder.is_empty())
        .map_or_else(
            || home().map(|home| home.join(".config")),
            |folder| Ok(PathBuf::from(folder)),
        )?;
    config_in(&machine)
}

/// Devin's config for the run: the config in the config folder `machine`, read as a
/// launch reads it (`ask::devin_config`: its comments and trailing commas left out, none
/// as an empty one), its compaction threshold at [`THRESHOLD`].
fn config_in(machine: &Path) -> Result<Value> {
    // The folder the adapter reads Devin's config in, on each system: `XDG_CONFIG_HOME` on
    // macOS and Linux, Windows' data folder (`APPDATA`) there. Without the second, the case
    // read the Windows machine's own config (integration B round 2, T-800).
    let env = [
        ("HOME".to_owned(), home()?.to_string_lossy().into_owned()),
        (
            "XDG_CONFIG_HOME".to_owned(),
            machine.to_string_lossy().into_owned(),
        ),
        ("APPDATA".to_owned(), machine.to_string_lossy().into_owned()),
    ];
    let mut config = ask::devin_config(&env)?;
    let Some(fields) = config.as_object_mut() else {
        return Err(Error::Daemon(
            "the machine's Devin config is no object".to_owned(),
        ));
    };
    let agent = fields.entry("agent").or_insert_with(|| json!({}));
    agent["compaction_threshold_tokens"] = json!(THRESHOLD);
    Ok(config)
}

/// Runs the Devin worker on a task its compactor compacts: what it came to, which holds
/// when the task was done, its result reached the chief, and Devin compacted the turn.
pub fn run(stand_in: &Path, log: &Log) -> Result<Finished> {
    if cfg!(windows) {
        return Err(Error::Daemon(
            "Devin keeps its config in its data folder on Windows, which the run cannot give a threshold of its own".to_owned(),
        ));
    }
    let plan = plan(&config()?);
    let task = Task::several(CALLS)?;
    let who = Who::Harness(Harness::Devin);
    let mut finished = check::run(who, &plan, &task, stand_in, log)?;
    match compaction(&live_folder(&plan.folder)?)? {
        Some(session) if session.summaries > 0 => {
            finished
                .case
                .check("Devin compacted the worker's turn", true, session.said())
        }
        Some(session) => finished.case.fail(format!(
            "Devin did not compact the worker's turn, which proves nothing of one it compacts: {}",
            session.said()
        )),
        None => finished
            .case
            .fail("Devin's store holds no session in the worker's folder"),
    }
    Ok(finished)
}

/// What Devin's store holds of the session in a folder: its id, and how many of its main
/// chain's messages are a compaction's summary and how many a user's.
#[derive(Debug, PartialEq, Eq)]
struct Session {
    id: String,
    summaries: usize,
    users: usize,
}

impl Session {
    fn said(&self) -> String {
        format!(
            "session {}: of its main chain's messages, {} a compaction's summary and {} a user's",
            self.id, self.summaries, self.users
        )
    }
}

/// The newest session of Devin's store in `folder`, read as the daemon reads the store
/// (read-only, never waiting on Devin for long), or none where it holds none.
fn compaction(folder: &Path) -> Result<Option<Session>> {
    let home = home()?;
    let store = devin_store::location(
        cfg!(windows),
        &home.to_string_lossy(),
        own_var("APPDATA").as_deref(),
        own_var("XDG_DATA_HOME").as_deref(),
    );
    let failed = |cause: rusqlite::Error| Error::Daemon(format!("{}: {cause}", store.display()));
    let db =
        Connection::open_with_flags(&store, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(failed)?;
    db.busy_timeout(Duration::from_secs(5)).map_err(failed)?;
    let mut places = vec![folder.to_string_lossy().into_owned()];
    if let Ok(real) = folder.canonicalize() {
        places.push(real.to_string_lossy().into_owned());
    }
    for place in places {
        let found: Option<(String, Option<i64>)> = db
            .query_row(
                "select id, main_chain_id from sessions where working_directory = ? order by created_at desc limit 1",
                [&place],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(failed)?;
        if let Some((id, head)) = found {
            return chain_of(&db, &id, head).map(Some).map_err(failed);
        }
    }
    Ok(None)
}

/// What the main chain of the session `id`, from `head` up, holds.
fn chain_of(db: &Connection, id: &str, head: Option<i64>) -> rusqlite::Result<Session> {
    let mut statement = db.prepare(
        "select node_id, parent_node_id, chat_message from message_nodes where session_id = ?",
    )?;
    let nodes: HashMap<i64, (Option<i64>, String)> = statement
        .query_map([id], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))?
        .collect::<rusqlite::Result<_>>()?;
    let mut session = Session {
        id: id.to_owned(),
        summaries: 0,
        users: 0,
    };
    let mut at = head;
    let mut walked = 0;
    while let Some((parent, message)) = at.and_then(|node| nodes.get(&node)) {
        let message: Value = serde_json::from_str(message).unwrap_or_default();
        if message["metadata"]["extensions"]["devin-rs/summary"].is_object() {
            session.summaries += 1;
        }
        if message["role"] == "user" {
            session.users += 1;
        }
        walked += 1;
        if walked > nodes.len() {
            break;
        }
        at = *parent;
    }
    Ok(session)
}

#[cfg(test)]
mod tests {
    use cf_e2e::files;

    use super::*;

    #[test]
    fn the_project_of_the_step_has_the_devin_worker_reading_its_config_from_the_runs_home() {
        let config = json!({ "agent": { "compaction_threshold_tokens": THRESHOLD } });
        let plan = plan(&config);
        assert_eq!(plan.folder, "finish-devin-compacted");
        assert_eq!(plan.roster[1]["id"], "live-devin");
        // No variable of the case's own: Devin reads its config in the kit's home.
        assert!(plan.vars.is_empty(), "{:?}", plan.vars);
        let [file] = plan.home_files.as_slice() else {
            panic!("{:?}", plan.home_files);
        };
        assert_eq!(
            (file.harness, file.variable, file.name.as_str()),
            (Harness::Devin, "XDG_CONFIG_HOME", "devin/config.json")
        );
        assert_eq!(serde_json::from_str::<Value>(&file.text).unwrap(), config);
    }

    /// The config a run gives Devin, made from the config folder `machine`.
    fn given(machine: &Path) -> Value {
        config_in(machine).unwrap()
    }

    #[test]
    fn the_owners_config_is_read_as_a_launch_reads_it_with_its_comments_and_none_as_empty() {
        // Calliope's P3-1 on T-655: the adapter reads the owner's config as JSONC, and
        // one that is not there as empty; the case read it as JSON, and required it.
        let machine = tempfile::tempdir().unwrap();
        assert_eq!(
            given(machine.path()),
            json!({ "agent": { "compaction_threshold_tokens": THRESHOLD } })
        );
        files::write(
            &machine.path().join("devin").join("config.json"),
            "// the owner's\n{ \"agent\": { \"model\": \"swe-2\", /* theirs */ }, \"theme_mode\": \"dark\", }\n",
        )
        .unwrap();
        assert_eq!(
            given(machine.path()),
            json!({
                "agent": { "model": "swe-2", "compaction_threshold_tokens": THRESHOLD },
                "theme_mode": "dark",
            })
        );
    }

    #[test]
    fn a_sessions_main_chain_is_counted_from_its_head_up_and_no_further() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "create table message_nodes (session_id text, node_id integer, parent_node_id integer, chat_message text)",
        )
        .unwrap();
        let summary = json!({ "role": "system", "metadata": { "extensions": { "devin-rs/summary": { "source": "file_compactor" } } } });
        let rows = [
            (1, None, json!({ "role": "user" })),
            (2, Some(1), json!({ "role": "assistant" })),
            (3, None, summary),
            (4, Some(3), json!({ "role": "assistant" })),
        ];
        for (node, parent, message) in rows {
            db.execute(
                "insert into message_nodes values ('s', ?, ?, ?)",
                (node, parent, message.to_string()),
            )
            .unwrap();
        }
        let compacted = chain_of(&db, "s", Some(4)).unwrap();
        assert_eq!((compacted.summaries, compacted.users), (1, 0));
        assert_eq!(
            compacted.said(),
            "session s: of its main chain's messages, 1 a compaction's summary and 0 a user's"
        );
        let whole = chain_of(&db, "s", Some(2)).unwrap();
        assert_eq!((whole.summaries, whole.users), (0, 1));
    }
}
