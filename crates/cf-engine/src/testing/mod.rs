//! The kit the engine's tests run it with, as `core-dispatcher.test.mjs`
//! made its fakes: an executor that runs the engine's work to stillness, a
//! gate, the time (a clock the test moves, and timers that are the loop's),
//! a pane host and an adapter whose agents do what the test tells them,
//! fakes of the other seams, a test's engine made with them all
//! ([`Context`]), and a recorder that writes down what the engine asks of
//! its seams in the shape of the Node traces the tests are held to.

mod adapter;
mod adapters;
mod context;
mod executor;
mod host;
mod ledger;
mod operations;
mod recorder;
mod seams;
mod time;
mod window;

pub use adapter::{AfterPrepare, Deliver, FakeAdapter, FakeAgent, Prepare, Ready, Started, Taking};
pub use adapters::{FakeAdapters, FakeRecords};
pub use context::{Closed, Context, Made, START_MS};
pub use executor::{Answer, Executor, Gate, GateWait};
pub use host::{window_of, FakeHost, OnRequest};
pub use operations::Restarted;
pub use recorder::Recorder;
pub use seams::{
    CountingLaunchIds, FakeCredentials, FakeLaunchFiles, FakeLog, FakePaneEnv, FakeRoles,
    FakeRoster, FakeTrace, MODELS,
};
pub use time::TestTime;
