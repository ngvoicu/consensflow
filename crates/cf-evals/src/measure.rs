//! What a chief did, read from the ledger of a finished eval run: the tasks it put
//! on the board and how many ran side by side, what it asked and told the human,
//! the advice and reviews it asked for, the edits it made with its own hands, its
//! last words, and which files of the fixture the run changed or added.
//!
//! The ledger file is read with SQLite and no ledger's code, whichever daemon wrote
//! it: the board is one of the things a run checks, so it cannot be what the tasks
//! are counted from. The daemon must have closed the ledger first, since it holds
//! its file; a ledger in use is read from a copy of the file and of its write-ahead
//! log ([`chief_open_question`] and [`devin_chief_questions`] do, while a run goes).
//! The recordings in `tests/fixtures/ledgers` are the ledgers the measure is held
//! to, a `.json` of the numbers each must give, made by a ledger file of
//! [`crate::ledger_file`].

mod board;
mod chief;
mod copy;
mod files;
mod metrics;
mod plumbing;
mod sentences;

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use board::TaskRow;

pub use chief::{chief_open_question, devin_chief_questions, devin_data, on_windows, OpenQuestion};
pub(crate) use copy::on_copy;
pub use files::changed;
pub use metrics::{
    owner_questions_value, Check, LongestResult, MemberQuestions, Metrics, Note, OwnerQuestions,
    Plumbing, ProjectRef, Task,
};
pub use plumbing::mechanics;
pub use sentences::{count_questions, owner_questions, question_sentences};

use crate::{js, Result};

/// How much of a note to the human a report keeps in its list of notes (the
/// whole text of them all is [`Metrics::notes_text`]).
const NOTE_SHOWN: usize = 200;

/// What a run adds to what the ledger holds.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options<'a> {
    /// The fixture the workspace was copied from, and the workspace the chief
    /// worked in: both, to say which files the run changed.
    pub files: Option<(&'a Path, &'a Path)>,
    /// How many question dialogs of the chief's harness the owner answered.
    pub pickers: i64,
}

/// The numbers of the ledger `file`, closed by the daemon.
pub fn measure(file: &Path, options: &Options) -> Result<Metrics> {
    let db = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let project = board::project(&db)?;
    let (human, chief) = board::human_and_chief(&db, project.id)?;
    let tasks = board::tasks(&db, project.id)?;
    let mine: Vec<&TaskRow> = tasks
        .iter()
        .filter(|row| row.is_the_chiefs(&chief.handle))
        .collect();
    let to_human = board::to_human(&db, human.id, &chief.handle)?;
    let notes: Vec<&board::ToHuman> = to_human.iter().filter(|m| m.kind == "note").collect();
    let window = chief::window(&db, chief.id, chief.harness.as_deref())?;
    let asked = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
    let in_pool = |pool: &str| asked(mine.iter().filter(|row| row.pool_is(pool)).count());
    Ok(Metrics {
        tasks: mine.iter().map(|row| row.task.clone()).collect(),
        parallel: board::most_at_once(&tasks, &chief.handle),
        advice: in_pool("advisor"),
        reviews: in_pool("reviewer"),
        // The human is asked in the chief's terminal: the ledger refuses a question
        // for them, so this stays 0, and a run says so.
        questions_on_board: asked(to_human.iter().filter(|m| m.kind == "question").count()),
        notes_to_human: notes
            .iter()
            .map(|note| Note {
                id: note.id,
                body: js::first(&note.body, NOTE_SHOWN),
            })
            .collect(),
        notes_text: notes
            .iter()
            .map(|note| note.body.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        longest_result: LongestResult {
            worker: longest_result(&mine, |row| {
                !row.pool_is("advisor") && !row.pool_is("reviewer")
            }),
            advisor: longest_result(&mine, |row| row.pool_is("advisor")),
            reviewer: longest_result(&mine, |row| row.pool_is("reviewer")),
        },
        task_count: asked(tasks.len()),
        chiefs: board::chiefs(&db, &chief)?,
        switches: asked(board::events_of(&db, project.id, "chief.switched")?.len()),
        history_reads: board::events_of(&db, project.id, "chief.history.read")?,
        chief_id: chief.id,
        chief_harness: chief.harness.clone(),
        human_id: human.id,
        chief_turns: window.turns,
        chief_edits: window.edits,
        chief_last_words: window.last_words,
        owner_messages: window.owner_messages,
        ask_refused: window.ask_refused,
        owner_questions: owner_questions(&window.turn_ends, options.pickers),
        plumbing: plumbing::plumbing(&db, chief.id, human.id)?,
        chief_words_now: chief::words_now(&db)?,
        member_questions_by: plumbing::member_questions_by(&db, chief.id, human.id)?,
        files_changed: match options.files {
            Some((fixture, workspace)) => changed(fixture, workspace)?,
            None => Vec::new(),
        },
        project,
        chief: chief.handle,
    })
}

/// The longest result, in characters, that the chief's tasks `of` a kind of member
/// got, or 0.
fn longest_result(tasks: &[&TaskRow], of: impl Fn(&TaskRow) -> bool) -> i64 {
    tasks
        .iter()
        .filter(|row| of(row))
        .flat_map(|row| row.messages.iter())
        .filter(|message| message.kind == "result")
        .map(|message| i64::try_from(js::length(&message.body)).unwrap_or(i64::MAX))
        .max()
        .unwrap_or(0)
}
