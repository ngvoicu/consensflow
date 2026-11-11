//! What the measure says of a run: the numbers a scenario's expectations are
//! closures over, and the shape a report keeps them in. A report is read back by
//! the summary years later, so the keys are the ones JavaScript wrote, in its
//! order (`camelCase`), and a number that is not there is `null`.

use std::collections::BTreeMap;

use serde_json::{json, Value};

/// The project the ledger holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectRef {
    pub id: i64,
    pub name: String,
}

/// A task the chief put on the board.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Task {
    pub number: i64,
    pub title: String,
    /// `worker`, `advisor` or `reviewer`; none for a task given to a window by name.
    pub pool: Option<String>,
    pub tier: Option<String>,
    pub state: String,
    pub assignee: Option<String>,
}

/// A file a scenario reads whole: what it held as the run began, at the first look
/// that saw the first worker task's result in, and at the end; none where it was
/// not there, or not looked at.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileRead {
    pub start: Option<String>,
    pub at_result: Option<String>,
    pub end: Option<String>,
}

impl FileRead {
    /// The file as a report keeps it.
    pub fn to_value(&self) -> Value {
        json!({ "start": self.start, "atResult": self.at_result, "end": self.end })
    }
}

/// A time a task was sent back to its window: when, and the chief's words.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reopen {
    pub at: String,
    pub body: String,
}

/// The rounds of one task the chief asked for: when it was asked for and with what
/// brief, when each result came in, each time the chief sent it back and with what
/// words, and when it was accepted. The times are the ledger's own, which sort as
/// text; a scenario that holds the chief to what it did after a result compares them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rounds {
    pub number: i64,
    pub pool: Option<String>,
    pub state: String,
    pub created_at: String,
    /// The brief the chief wrote.
    pub brief: String,
    /// When each result that stood came in, in order (one withdrawn is none).
    pub results: Vec<String>,
    pub reopens: Vec<Reopen>,
    /// When the task was accepted last, if it is accepted.
    pub accepted_at: Option<String>,
}

/// Whether a task of `pool` is work for a worker: the worker's pool, or none (a task given
/// to a window by name). Said by the worker's own pool, not by a list of the others: a role
/// this crate knows nothing of yet (another row of the product's roles' table, which an
/// eval does not read) is no worker's.
pub fn is_work_pool(pool: Option<&str>) -> bool {
    matches!(pool, None | Some("worker"))
}

impl Rounds {
    /// Whether the task went to a reviewer.
    pub fn is_review(&self) -> bool {
        self.pool.as_deref() == Some("reviewer")
    }

    /// Whether the task is work for a worker (a task given to a window by name has no
    /// pool): not a review, advice, a test or an image.
    pub fn is_work(&self) -> bool {
        is_work_pool(self.pool.as_deref())
    }

    /// When the first result that stood came in.
    pub fn first_result(&self) -> Option<&str> {
        self.results.first().map(String::as_str)
    }

    /// The times the chief sent it back after `time`.
    pub fn reopens_after(&self, time: &str) -> Vec<&Reopen> {
        self.reopens
            .iter()
            .filter(|reopen| reopen.at.as_str() > time)
            .collect()
    }
}

/// A turn of the chief that ended by asking the owner in its terminal: when, and how many
/// question sentences. The time is the one the harness gave the message, else the one the
/// daemon copied it at; it sorts as text with the times of the board.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Asking {
    pub at: String,
    pub questions: i64,
}

/// A note the chief sent the human: the start of its words.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Note {
    pub id: i64,
    pub body: String,
}

/// The longest result each kind of member sent the chief, in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LongestResult {
    pub worker: i64,
    pub advisor: i64,
    pub reviewer: i64,
}

/// What the chief asked the owner in its terminal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OwnerQuestions {
    /// How many question sentences its turns ended with.
    pub questions: i64,
    /// In how many of those turns.
    pub turns_asking: i64,
    /// How many question dialogs the owner answered in its window.
    pub pickers: i64,
    /// The turn ends that asked, whole, for a person to check.
    pub texts: Vec<String>,
}

/// The board's plumbing, counted from the ledger whatever the chief decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Plumbing {
    pub briefs: i64,
    pub briefs_delivered: i64,
    pub results: i64,
    pub results_delivered: i64,
    pub member_questions: i64,
    pub member_questions_answered: i64,
    pub answers_delivered: i64,
    pub accepted: i64,
    pub tells: i64,
    pub tells_answered: i64,
    pub placed: i64,
    pub pauses: i64,
    pub resumes: i64,
    pub continuations: i64,
}

/// The questions the members of one role and harness put to the chief.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemberQuestions {
    pub role: String,
    pub harness: Option<String>,
    pub asked: i64,
    pub answered: i64,
    /// How many answers reached the member's window.
    pub delivered: i64,
    /// The longest question, in characters.
    pub longest: i64,
}

/// What a chief did, read from the ledger of a finished run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Metrics {
    pub project: ProjectRef,
    pub chief: String,
    /// The tasks the chief asked for, by number.
    pub tasks: Vec<Task>,
    /// The most tasks whose windows had the brief and no result yet at one moment.
    pub parallel: i64,
    pub advice: i64,
    pub reviews: i64,
    /// The questions the chief put to the human on the board: the ledger refuses one, so none.
    pub questions_on_board: i64,
    pub notes_to_human: Vec<Note>,
    /// The whole text of the chief's notes: a scenario may look for words at the end.
    pub notes_text: String,
    pub longest_result: LongestResult,
    /// Every task of the project, whoever asked for it.
    pub task_count: i64,
    /// The harnesses the chief ran on, in order: each switch starts a conversation.
    pub chiefs: Vec<Option<String>>,
    pub switches: i64,
    /// The history the chief read with `cf history`, one event each.
    pub history_reads: Vec<Value>,
    pub chief_id: i64,
    pub chief_harness: Option<String>,
    pub human_id: i64,
    pub chief_turns: i64,
    /// The edits it made with its own hands, counted for a Claude Code chief only.
    pub chief_edits: Option<i64>,
    pub chief_last_words: String,
    /// What the owner typed into its window, in characters.
    pub owner_messages: Vec<i64>,
    /// How often it tried `cf ask` and was told to ask in its terminal.
    pub ask_refused: i64,
    pub owner_questions: OwnerQuestions,
    pub plumbing: Plumbing,
    /// What the chief wrote in its current conversation: the one the last switch started.
    pub chief_words_now: String,
    pub member_questions_by: Vec<MemberQuestions>,
    /// The files of the fixture the run changed or added, by their paths.
    pub files_changed: Vec<String>,
    /// The files of the fixture the run deleted, for a scenario that counts them
    /// ([`crate::scenarios::Scenario::counts_gone_files`]); none for any other.
    pub files_gone: Option<Vec<String>>,
    /// The files the scenario reads whole ([`crate::scenarios::Scenario::reads`]), by
    /// their paths.
    pub files_read: BTreeMap<String, FileRead>,
    /// The rounds of each task the chief asked for, by number, for a scenario that
    /// holds the chief to what it did with a result ([`crate::scenarios::Scenario::reads_the_rounds`]);
    /// none for any other.
    pub rounds: Option<Vec<Rounds>>,
    /// When the chief put questions to the owner in its terminal, for the same scenarios as
    /// `rounds`: a question counts for or against the chief by where it falls among the moments
    /// of the board ([`Metrics::questions_between`]). A question dialog of the harness (a picker)
    /// carries no time and is not among them.
    pub asked_at: Option<Vec<Asking>>,
}

impl Metrics {
    /// How many question sentences the chief put to the owner in its terminal after `from` and
    /// before `until` (none for the start, none for the end of the run): zero where the measure
    /// did not read them. Both bounds are moments of the board, and a question at one of them
    /// is outside.
    pub fn questions_between(&self, from: Option<&str>, until: Option<&str>) -> i64 {
        self.asked_at
            .iter()
            .flatten()
            .filter(|asking| from.is_none_or(|from| asking.at.as_str() > from))
            .filter(|asking| until.is_none_or(|until| asking.at.as_str() < until))
            .map(|asking| asking.questions)
            .sum()
    }

    /// How many questions the chief put to the owner in its terminal, in words or
    /// in a picker.
    pub fn asked_in_terminal(&self) -> i64 {
        self.owner_questions.questions + self.owner_questions.pickers
    }

    /// `value`, a report's numbers, with what JavaScript had none of at its end, for a
    /// scenario that looks at it only: `filesGone`, then `filesRead`, then `rounds`, then
    /// `askedAt`.
    pub fn with_files(&self, mut value: Value) -> Value {
        if let Some(gone) = &self.files_gone {
            value["filesGone"] = json!(gone);
        }
        if !self.files_read.is_empty() {
            value["filesRead"] = self
                .files_read
                .iter()
                .map(|(file, read)| (file.clone(), read.to_value()))
                .collect::<serde_json::Map<_, _>>()
                .into();
        }
        if let Some(rounds) = &self.rounds {
            value["rounds"] = rounds.iter().map(rounds_value).collect::<Vec<_>>().into();
        }
        if let Some(asked) = &self.asked_at {
            value["askedAt"] = asked
                .iter()
                .map(|asking| json!({ "at": asking.at, "questions": asking.questions }))
                .collect::<Vec<_>>()
                .into();
        }
        value
    }

    /// The numbers as a report keeps them: the keys JavaScript wrote, in its order,
    /// and then the files a scenario looks at ([`Metrics::with_files`]).
    pub fn to_value(&self) -> Value {
        self.with_files(json!({
            "project": { "id": self.project.id, "name": self.project.name },
            "chief": self.chief,
            "tasks": self.tasks.iter().map(task_value).collect::<Vec<_>>(),
            "parallel": self.parallel,
            "advice": self.advice,
            "reviews": self.reviews,
            "questionsOnBoard": self.questions_on_board,
            "notesToHuman": self.notes_to_human.iter()
                .map(|note| json!({ "id": note.id, "body": note.body }))
                .collect::<Vec<_>>(),
            "notesText": self.notes_text,
            "longestResult": {
                "worker": self.longest_result.worker,
                "advisor": self.longest_result.advisor,
                "reviewer": self.longest_result.reviewer,
            },
            "taskCount": self.task_count,
            "chiefs": self.chiefs,
            "switches": self.switches,
            "historyReads": self.history_reads,
            "chiefId": self.chief_id,
            "chiefHarness": self.chief_harness,
            "humanId": self.human_id,
            "chiefTurns": self.chief_turns,
            "chiefEdits": self.chief_edits,
            "chiefLastWords": self.chief_last_words,
            "ownerMessages": self.owner_messages,
            "askRefused": self.ask_refused,
            "ownerQuestions": owner_questions_value(&self.owner_questions),
            "plumbing": plumbing_value(&self.plumbing),
            "chiefWordsNow": self.chief_words_now,
            "memberQuestionsBy": self.member_questions_by.iter()
                .map(member_questions_value)
                .collect::<Vec<_>>(),
            "filesChanged": self.files_changed,
        }))
    }
}

fn rounds_value(rounds: &Rounds) -> Value {
    json!({
        "number": rounds.number,
        "pool": rounds.pool,
        "state": rounds.state,
        "createdAt": rounds.created_at,
        "brief": rounds.brief,
        "results": rounds.results,
        "reopens": rounds.reopens.iter()
            .map(|reopen| json!({ "at": reopen.at, "body": reopen.body }))
            .collect::<Vec<_>>(),
        "acceptedAt": rounds.accepted_at,
    })
}

fn task_value(task: &Task) -> Value {
    json!({
        "number": task.number,
        "title": task.title,
        "pool": task.pool,
        "tier": task.tier,
        "state": task.state,
        "assignee": task.assignee,
    })
}

/// What the chief asked the owner, as a report keeps it.
pub fn owner_questions_value(asked: &OwnerQuestions) -> Value {
    json!({
        "questions": asked.questions,
        "turnsAsking": asked.turns_asking,
        "pickers": asked.pickers,
        "texts": asked.texts,
    })
}

fn plumbing_value(plumbing: &Plumbing) -> Value {
    json!({
        "briefs": plumbing.briefs,
        "briefsDelivered": plumbing.briefs_delivered,
        "results": plumbing.results,
        "resultsDelivered": plumbing.results_delivered,
        "memberQuestions": plumbing.member_questions,
        "memberQuestionsAnswered": plumbing.member_questions_answered,
        "answersDelivered": plumbing.answers_delivered,
        "accepted": plumbing.accepted,
        "tells": plumbing.tells,
        "tellsAnswered": plumbing.tells_answered,
        "placed": plumbing.placed,
        "pauses": plumbing.pauses,
        "resumes": plumbing.resumes,
        "continuations": plumbing.continuations,
    })
}

fn member_questions_value(asked: &MemberQuestions) -> Value {
    json!({
        "role": asked.role,
        "harness": asked.harness,
        "asked": asked.asked,
        "answered": asked.answered,
        "delivered": asked.delivered,
        "longest": asked.longest,
    })
}

/// One expectation or one check of the board's plumbing, and whether it held.
///
/// An expectation can also be **unmet**: it reads something the run never made (the second
/// review of a chief that asked for none), so it judged nothing. An unmet check is not `ok`,
/// and is no failure of the chief's either; a report says it apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub unmet: bool,
}

impl Check {
    /// A check that held, or did not.
    pub fn new(name: impl Into<String>, ok: bool) -> Self {
        Self {
            name: name.into(),
            ok,
            unmet: false,
        }
    }

    /// A check that judged nothing, since what it reads was not there.
    pub fn unmet(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok: false,
            unmet: true,
        }
    }

    /// A check as a report keeps it: `unmet` is a key only of a check that is.
    pub fn to_value(&self) -> Value {
        if self.unmet {
            json!({ "name": self.name, "ok": self.ok, "unmet": true })
        } else {
            json!({ "name": self.name, "ok": self.ok })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_files_gone_and_read_are_the_last_keys_of_a_report_and_no_keys_when_none_is_looked_at() {
        let none = Metrics::default().to_value();
        assert!(none.get("filesGone").is_none(), "{none}");
        assert!(none.get("filesRead").is_none(), "{none}");
        let looked = Metrics {
            files_gone: Some(vec!["site/index.html".to_owned()]),
            files_read: BTreeMap::from([(
                "site/evaluare.html".to_owned(),
                FileRead {
                    start: Some("0 și 10".to_owned()),
                    at_result: Some("0 și 10 · revizuită".to_owned()),
                    end: None,
                },
            )]),
            ..Metrics::default()
        }
        .to_value();
        let keys: Vec<&String> = looked.as_object().unwrap().keys().collect();
        assert_eq!(keys[keys.len() - 2..], ["filesGone", "filesRead"]);
        assert_eq!(looked["filesGone"], json!(["site/index.html"]));
        assert_eq!(
            looked["filesRead"],
            json!({ "site/evaluare.html": {
                "start": "0 și 10", "atResult": "0 și 10 · revizuită", "end": null
            } })
        );
        // A scenario that counts the files gone and finds none says so.
        let counted = Metrics {
            files_gone: Some(Vec::new()),
            ..Metrics::default()
        }
        .to_value();
        assert_eq!(counted["filesGone"], json!([]));
    }
}
