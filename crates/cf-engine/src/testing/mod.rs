//! The kit the engine's tests run it with, as `core-dispatcher.test.mjs`
//! made its fakes: an executor that runs the engine's work to stillness, a
//! gate, a pane host and an adapter whose agents do what the test tells
//! them, fakes of the other seams, a test's engine made with them all
//! ([`Context`]), and a recorder that writes down what the engine asks of
//! its seams in the shape of the Node traces the tests are held to.

mod adapter;
mod adapters;
mod context;
mod driver;
mod executor;
mod host;
mod recorder;
mod seams;
mod window;

pub use adapter::{FakeAdapter, FakeAgent, Ready, Started};
pub use adapters::{FakeAdapters, FakeRecords};
pub use context::{Closed, Context, Made, START_MS};
pub use driver::Driver;
pub use executor::{next_turn, Executor, Gate, GateWait, NextTurn, Pending};
pub use host::{window_of, FakeHost};
pub use recorder::Recorder;
pub use seams::{
    CountingLaunchIds, FakeCredentials, FakeLaunchFiles, FakeLog, FakeOperations, FakePaneEnv,
    FakeRoles, FakeRoster, FakeTrace, MODELS,
};
