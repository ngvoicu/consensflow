//! What each Claude window of a project used, as its own record counts tokens: the
//! turns it took, what its first turn held in context, what it read from the cache,
//! wrote and output over them all. Claude Code keeps its conversations under
//! `<config folder>/projects/<folder>/<session>.jsonl` (the user's `~/.claude`, or the throwaway
//! folder of an eval); the ledger names the session of each window.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use crate::js::{pad_end, pad_start, thousands};
use crate::Result;

/// What a record counts of a conversation.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Used {
    pub turns: u64,
    /// What the first turn had in its context: the cache it read and what it wrote.
    pub first: Option<u64>,
    pub read: u64,
    pub written: u64,
    pub output: u64,
}

/// What the lines of a Claude Code conversation say was used: each message of the
/// assistant counted once (a message is written once for each of its content blocks).
pub(super) fn used(record: &str) -> Used {
    let mut seen: HashSet<Option<String>> = HashSet::new();
    let mut used = Used::default();
    for line in record.lines().filter(|line| line.contains("\"usage\"")) {
        let Ok(parsed) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let message = &parsed["message"];
        let id = message["id"].as_str().map(str::to_owned);
        if parsed["type"] != "assistant" || !seen.insert(id) {
            continue;
        }
        let usage = &message["usage"];
        let count = |key: &str| usage[key].as_u64().unwrap_or(0);
        let read = count("cache_read_input_tokens");
        let written = count("cache_creation_input_tokens") + count("input_tokens");
        used.turns += 1;
        used.first.get_or_insert(read + written);
        used.read += read;
        used.written += written;
        used.output += count("output_tokens");
    }
    used
}

/// The file `session.jsonl` in any folder of `projects`.
fn record_of(projects: &Path, session: &str) -> Option<PathBuf> {
    fs::read_dir(projects)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|folder| folder.path().join(format!("{session}.jsonl")))
        .find(|file| file.exists())
}

/// A count in a column of ten.
fn column(count: u64) -> String {
    pad_start(&thousands(count), 10)
}

/// Says what each Claude window of the project `project` used, as the records in `projects` (the
/// `projects` folder of their Claude config folder) count it.
pub(super) fn say(db: &Connection, project: i64, projects: &Path, out: &mut dyn Write) -> Result {
    writeln!(out, "\nClaude windows, as their records count tokens")?;
    writeln!(
        out,
        "  {} turns  first turn  cache read  written  output",
        pad_end("session", 26)
    )?;
    let mut statement = db.prepare(
        "SELECT p.handle, c.native_session FROM conversation c
         JOIN participant p ON p.id = c.participant_id
         WHERE p.project_id = ?1 AND p.harness = 'claude-code' ORDER BY c.id",
    )?;
    let windows = statement
        .query_map([project], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (handle, session) in windows {
        let Some(file) = session.and_then(|session| record_of(projects, &session)) else {
            continue;
        };
        let used = used(&fs::read_to_string(&file).map_err(crate::Error::file("read", &file))?);
        writeln!(
            out,
            "  {} {} {} {} {} {}",
            pad_end(&handle, 26),
            pad_start(&used.turns.to_string(), 5),
            column(used.first.unwrap_or(0)),
            column(used.read),
            column(used.written),
            column(used.output)
        )?;
    }
    Ok(())
}
