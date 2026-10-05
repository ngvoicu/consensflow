//! Where the runtime's tests run their work: on the executor, drained by
//! hand, as the kit's tests do, or on a real tokio `LocalSet` under the
//! executor's driver, as the daemon does. A rule of the runtime holds on both
//! or it is no rule.

use std::cell::RefCell;
use std::convert::Infallible;
use std::future::Future;
use std::rc::Rc;

use tokio::runtime::{Builder, Runtime};
use tokio::task::{yield_now, JoinHandle, LocalSet};

use super::Executor;
use crate::testing::Gate;

/// What a test's pieces of work did, in order.
#[derive(Clone, Default)]
pub(super) struct Log(Rc<RefCell<Vec<&'static str>>>);

impl Log {
    pub(super) fn push(&self, what: &'static str) {
        self.0.borrow_mut().push(what);
    }

    pub(super) fn taken(&self) -> Vec<&'static str> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}

/// A piece of work that says it started, waits on `gate`, and says it ended.
pub(super) async fn gated(log: Log, gate: Gate, start: &'static str, end: &'static str) {
    log.push(start);
    gate.wait().await;
    log.push(end);
}

/// A tokio runtime with its `LocalSet`, the executor's driver a task of it.
pub(super) struct Local {
    runtime: Runtime,
    pub(super) set: LocalSet,
    driver: JoinHandle<Infallible>,
}

impl Local {
    /// A `LocalSet` that runs the driver of `executor`.
    pub(super) fn driving(executor: &Rc<Executor>) -> Self {
        let runtime = Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("a current-thread runtime");
        let set = LocalSet::new();
        let driver = set.spawn_local(executor.driver());
        Self {
            runtime,
            set,
            driver,
        }
    }

    /// Runs `work` on the `LocalSet` until it ends: its output.
    pub(super) fn block_on<T>(&self, work: impl Future<Output = T>) -> T {
        let output = self.set.block_on(&self.runtime, work);
        // A strict executor lets a panic in work out of its drain, which ends
        // the driver, where the daemon's would go on without the work.
        assert!(!self.driver.is_finished(), "a piece of work panicked");
        output
    }
}

/// Where a test's work runs.
pub(super) struct Stage {
    pub(super) executor: Rc<Executor>,
    local: Option<Local>,
}

impl Stage {
    /// One stage of each kind, to run a test on both.
    pub(super) fn both() -> [Self; 2] {
        [Self::by_hand(), Self::local_set()]
    }

    /// Work drained by the test, as the kit does.
    pub(super) fn by_hand() -> Self {
        Self {
            executor: Rc::new(Executor::strict()),
            local: None,
        }
    }

    /// Work drained by the driver on a `LocalSet`, as the daemon does.
    pub(super) fn local_set() -> Self {
        let executor = Rc::new(Executor::strict());
        let local = Local::driving(&executor);
        Self {
            executor,
            local: Some(local),
        }
    }

    /// The `LocalSet` of a stage that has one.
    pub(super) fn local(&self) -> &Local {
        self.local.as_ref().expect("a stage on a LocalSet")
    }

    /// Runs everything woken until nothing can move.
    pub(super) fn run(&self) {
        match &self.local {
            None => {
                self.executor.drain();
            }
            // One yield lets the `LocalSet` run what is woken, the driver
            // first, which drains all of it before the yield comes back.
            Some(local) => local.block_on(yield_now()),
        }
    }

    /// Runs `work` as work of its own, with everything else, until nothing
    /// can move: its answer, if it has one by then.
    pub(super) fn finish<T: 'static>(&self, work: impl Future<Output = T> + 'static) -> Option<T> {
        let answer = self.executor.start(work);
        self.run();
        answer.take()
    }
}
