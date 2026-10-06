//! The records' worker, as a stand-in. There is no transcript a test can have
//! `cf_harness::records::Thread` read to the moment it chooses, so each look is
//! read on a thread of its own, which waits for the test to let it go and then
//! answers over a channel that is woken from that thread, as the worker's
//! answers are (its `oneshot`, sent from its own thread): the one mechanism of
//! the real worker that the executor is given from outside.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;

use cf_harness::contract::{Records, Work};
use cf_harness::records::{Options, Reading};
use cf_proto::agents::Harness;
use tokio::sync::oneshot;

use crate::seams::{DaemonRecords, DaemonSpawn};

/// The worker: the looks asked of it, each on a thread that waits to be let
/// go. They are let go in the order they were asked.
#[derive(Default)]
pub struct Standing {
    asked: RefCell<Vec<(mpsc::Sender<()>, JoinHandle<()>)>>,
}

impl Standing {
    /// The daemon's records over this worker, its answers callbacks on
    /// `spawn`'s executor, as the daemon starts them.
    pub fn over(self: &Rc<Self>, spawn: &Rc<DaemonSpawn>) -> Rc<DaemonRecords> {
        Rc::new(DaemonRecords::new(
            Rc::clone(self) as Rc<dyn Records>,
            Rc::clone(spawn),
        ))
    }

    /// Lets the next `count` looks go, each answered from its own thread, and
    /// waits until every one of them has answered: when this returns the
    /// answers are all there, and nothing has run.
    pub fn answer(&self, count: usize) {
        let letting_go: Vec<_> = self.asked.borrow_mut().drain(..count).collect();
        for (go, thread) in letting_go {
            go.send(()).expect("the thread waits");
            thread.join().expect("the thread answered");
        }
    }
}

impl Records for Standing {
    fn look<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
        _options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        // Asked when first polled, as the worker is.
        Box::pin(async move {
            let (answer, answered) = oneshot::channel();
            let (go, goes) = mpsc::channel::<()>();
            let thread = std::thread::spawn(move || {
                goes.recv().expect("the test lets it go");
                let _ = answer.send(Arc::new(Reading::Unknown("unknown".to_owned())));
            });
            self.asked.borrow_mut().push((go, thread));
            answered.await.expect("the worker answers")
        })
    }

    fn has_transcript<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        Box::pin(async { Ok(false) })
    }
}

/// A look at nothing, which the worker answers: a wait on it.
pub async fn look(records: &DaemonRecords) {
    let session = "a-session".to_owned();
    records
        .look(Harness::Claude, &session, &Options::default())
        .await;
}
