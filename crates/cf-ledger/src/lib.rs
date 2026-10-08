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
mod ledger;
mod messages;
/// The ledger's vocabulary: the words its records hold, their limits, and
/// the parsers a caller reading JSON (the API, the page) checks a request with.
pub mod model;
mod names;
mod page_reads;
mod projects;
mod questions;
mod queue;
mod schema;
mod staff;
mod store;
mod tasks;
/// What the players of Node's recordings share, and the tests of this crate use.
#[cfg(feature = "test-support")]
pub mod testing;
mod views;

pub use cf_proto::ledger::{
    Begun, Board, Candidate, ChiefConversation, ChiefOpenWork, Claim, ConversationView, Cut,
    DeletedProject, EventView, HeldTask, Lane, LastSwitch, LatestMessages, LatestTranscript,
    MemberView, MessageView, ParticipantView, ProjectView, Question, QuestionOption, RemovedMember,
    StaffMember, Stop, TaskCard, TaskCreated, TaskMoved, TaskReleased, TaskThatFits, TaskThread,
    TaskTranscript, TaskView, TierChange,
};
pub use conversations::{ChiefSwitch, TRANSCRIPT_ITEM_MAX};
#[cfg(feature = "test-support")]
pub use ledger::Watcher;
pub use ledger::{open_ledger, Event, Ledger, Options};
pub use messages::{NewNote, NewQuestion, Read};
pub use model::LedgerError;
pub use page_reads::PAGE_BYTES;
pub use projects::{NewChief, NewMember, NewProject};
pub use schema::{migrate, MIGRATIONS, SCHEMA_VERSION};
pub use tasks::{NewTask, RESUME_WORDS};
