//! The ledger: the one durable record of every project, participant, task and
//! inbox message, in `<home>/consensflow.db`. The board and every inbox are
//! views of it. A port of Node's ledger, file for file; its rules are the long
//! comment of its index, each with a test, and every ledger the Node suite
//! opened is a trace this crate replays (`tests/replay.rs`).
//!
//! The daemon is the only process that opens the file. The connection runs
//! in SQLite's exclusive locking mode and takes the write lock when it opens,
//! so the lock is the instance lock: a second opener, in this process or
//! another, is refused with `ledger-locked`, and the operating system
//! releases the lock when the holder closes or dies. Every operation is one
//! transaction: it validates, writes and logs, or it is refused and writes
//! nothing. This crate never reads the environment and never logs: the file
//! and the clock are arguments, and every refusal has a stable code.

#![forbid(unsafe_code)]

mod conversations;
pub mod handles;
mod ledger;
mod meeting;
mod messages;
/// The ledger's vocabulary: the words its records hold, their limits, and
/// the parsers a caller reading JSON (the API, the page) checks a request with.
pub mod model;
mod model_lists;
mod names;
mod page_reads;
mod projects;
mod questions;
mod queue;
pub mod roles;
mod saved_agents;
mod schema;
mod specs;
mod staff;
mod store;
mod tasks;
/// What the players of Node's recordings share, and the tests of this crate use.
#[cfg(feature = "test-support")]
pub mod testing;
mod text;
mod views;
mod wake_ups;

pub use cf_proto::ledger::{
    Begun, Board, Candidate, ChiefConversation, ChiefOpenWork, Claim, ConversationView, Cut,
    DeletedProject, EventView, HeldTask, HistoryFrom, HistoryItem, HistoryKey, HistoryRead,
    HistoryRow, ItemDetail, Lane, LastSwitch, LatestMessages, LatestTranscript, Links, MemberView,
    MessageParties, MessageView, ParticipantView, ProjectView, Question, QuestionOption,
    RemovedMember, Resume, Scanned, StaffMember, Stop, TaskCard, TaskCreated, TaskEdit, TaskEdited,
    TaskMoved, TaskReleased, TaskRetiered, TaskThatFits, TaskThread, TaskTranscript, TaskView,
    TextCut, TextPart,
};
pub use cf_proto::wake_ups::WakeUpView;
pub use conversations::{
    ChiefSwitch, HistoryAsk, FIELD_MAX, HISTORY_FIND_MAX, HISTORY_PART, HISTORY_SCAN_BYTES,
    ID_SHOWN, NATIVE_SESSION_MAX, TRANSCRIPT_ITEM_MAX,
};
#[cfg(feature = "test-support")]
pub use ledger::Watcher;
pub use ledger::{open_ledger, Event, Ledger, Options};
pub use meeting::{CHIEF_CUT, HANDOFF_TITLE, LEAD_HANDOFF_TITLE, SWITCH_ASKED};
pub use messages::{
    continuation, continuation_in, continuation_tasks, cut_turn, cut_turn_unquoted, is_busy_note,
    is_told, waiting_for_the_reopen, GivenUp, HandOver, NewNote, NewQuestion, Read, MAY_STILL_RUN,
    NOT_PROVEN_GONE, WITHDRAWN_AFTER_HANDED,
};
pub use model::LedgerError;
pub use page_reads::PAGE_BYTES;
pub use projects::{
    launch_size, parse_advisor_tool, parse_compaction, parse_description, parse_name,
    parse_prompt_suggestion, parse_staff_rules, NewChief, NewMember, NewProject,
    COMPACTION_DEFAULT, COMPACTION_MAX, COMPACTION_MIN, STAFF_RULES_MAX_BYTES,
    STAFF_RULES_MAX_LINES,
};
pub use queue::{OVER, SHOWN};
pub use schema::{migrate, MIGRATIONS, SCHEMA_VERSION};
pub use specs::SpecsChange;
pub use staff::MemberEdit;
pub use tasks::{reopen_or_accept, window_closed, NewTask, RESUME_WORDS, SAME_ERROR_WITHIN_MS};
pub use tasks::{resumed_in_vain, NEVER_BEGAN};
pub use wake_ups::{NewWakeUp, WakeUpSetting, WakeUpUnit, DEFAULT_WAKE_UP_TEXT, WAKE_UP_FLOOR_MS};
