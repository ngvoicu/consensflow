//! What the measure says of a run: the numbers a scenario's expectations are
//! closures over, and the shape a report keeps them in. A report is read back by
//! the summary years later, so the keys are the ones JavaScript wrote, in its
//! order (`camelCase`), and a number that is not there is `null`.

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
}

impl Metrics {
    /// How many questions the chief put to the owner in its terminal, in words or
    /// in a picker.
    pub fn asked_in_terminal(&self) -> i64 {
        self.owner_questions.questions + self.owner_questions.pickers
    }

    /// The numbers as a report keeps them: the keys JavaScript wrote, in its order.
    pub fn to_value(&self) -> Value {
        json!({
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
        })
    }
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub ok: bool,
}

impl Check {
    /// A check as a report keeps it.
    pub fn to_value(&self) -> Value {
        json!({ "name": self.name, "ok": self.ok })
    }
}
