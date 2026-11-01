//! The kit the engine's tests run it with, as Node's dispatcher suite made its
//! fakes: the engine's own executor, which the kit drains to stillness, the
//! answers of the work a test starts on it, a gate, holds a test puts on the
//! fakes' calls ([`Holds`]), the time (a clock the test moves, and timers that
//! are the loop's), a pane host and an adapter whose agents do what the test
//! tells them, fakes of the other seams, a test's engine made with them all
//! ([`Context`]), and a recorder that writes down what the engine asks of its
//! seams in the shape of the Node traces the tests are held to. What Node's
//! dispatcher never asked of its host (the work a turn left running) is the
//! one thing the recorder leaves out ([`FakeBackground`]).

mod adapter;
mod adapters;
pub mod ask;
mod background;
mod context;
mod executor;
mod holds;
mod host;
mod ledger;
mod operations;
mod recorder;
mod seams;
mod time;
mod window;

pub use adapter::{
    AfterPrepare, CameUp, Deliver, FakeAdapter, FakeAgent, Prepare, Ready, Started, Taking,
};
pub use adapters::{FakeAdapters, FakeRecords};
pub use background::FakeBackground;
pub use context::{paced_mode, Closed, Context, Made, PACED_VARIABLE, START_MS};
pub use executor::{Answer, Gate, GateWait};
pub use holds::Holds;
pub use host::{window_of, FakeHost, OnRequest};
pub use operations::Restarted;
pub use recorder::Recorder;
pub use seams::{
    CountingLaunchIds, FakeCredentials, FakeLaunchFiles, FakeLog, FakePaneEnv, FakeRoles,
    FakeRoster, FakeTrace, MODELS,
};
pub use time::TestTime;
pub use window::{COMPACTING, TRUST_PROMPT, TRUST_QUESTION, TRUST_QUOTED, TRUST_UNPLACED};

/// What a note that replaces a role says before the role text, which a test
/// that quotes it to the window of a worker needs word for word.
pub const REPLACEMENT_OPENING: &str = crate::role_change::OPENING;
