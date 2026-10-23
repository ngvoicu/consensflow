//! How long a pass waits for its participants' steps: [`STEP_WAIT`] at most. A step
//! still waiting on its window then (a pane host or a record that does not answer)
//! goes on apart, holding its participant, so the passes after this one step the
//! other windows and leave that one be until it ends. A pass that fails at its first
//! failing step leaves the rest apart the same way. Whatever a step left apart ends
//! with is written down, and the daemon's log says which windows a pass went on
//! without, and how long their steps took (at most once in [`LEFT_TOLD_MS`]): a host
//! that stalls leaves a trace, never what a window shows.

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;

use futures_util::future::{select, Either};
use futures_util::stream::{FuturesUnordered, StreamExt};
use futures_util::FutureExt;

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::Begun;
use crate::seams::EngineError;

/// How long a pass waits for its steps: a step answers in well under a second,
/// and the slowest pass in a day of a busy board took three.
pub const STEP_WAIT: Duration = Duration::from_millis(5_000);

/// How often at most the log says a pass went on without some steps.
const LEFT_TOLD_MS: i64 = 60_000;

/// One participant's step, begun by a pass.
pub(crate) struct Step {
    /// The participant's handle, which the log names it by.
    pub(crate) handle: String,
    pub(crate) record: Rc<Record>,
    pub(crate) begun: Begun<Result<(), EngineError>>,
}

/// What the passes keep of the steps they went on without.
#[derive(Default)]
pub(crate) struct StepsState {
    /// When the log last said a pass went on without some steps.
    told: Cell<Option<i64>>,
    /// The passes that went on without some steps since then, unsaid.
    unsaid: Cell<u32>,
}

/// A step's answer, with whose it is.
type Answer = (String, Result<(), EngineError>);
type Waiting = FuturesUnordered<Pin<Box<dyn Future<Output = Answer>>>>;

/// What a pass's wait for its steps came to next.
enum Next {
    One(Answer),
    AllEnded,
    TimedOut,
}

impl Dispatcher {
    /// Waits for `steps` [`STEP_WAIT`] at most, and answers the first failure as it
    /// comes; what is still going on then goes on apart ([`Dispatcher::go_on_without`]).
    pub(crate) async fn wait_for_steps(
        self: &Rc<Self>,
        steps: Vec<Step>,
    ) -> Result<(), EngineError> {
        let began = self.now();
        let mut named = Vec::with_capacity(steps.len());
        let mut waiting: Waiting = FuturesUnordered::new();
        for step in steps {
            let ended = Rc::new(Cell::new(false));
            named.push((step.handle.clone(), step.record, Rc::clone(&ended)));
            let (handle, begun) = (step.handle, step.begun);
            waiting.push(Box::pin(async move {
                let answer = begun.await;
                ended.set(true);
                (handle, answer)
            }));
        }
        // Most passes end at once: the wait is armed only for one that does not.
        let mut timer = None;
        loop {
            let next = match timer.as_mut() {
                None => match waiting.next().now_or_never() {
                    Some(Some(answer)) => Next::One(answer),
                    Some(None) => Next::AllEnded,
                    None => {
                        timer = Some(self.seams.time.sleep(STEP_WAIT));
                        continue;
                    }
                },
                Some(timer) => match select(waiting.next(), timer).await {
                    Either::Left((Some(answer), _)) => Next::One(answer),
                    Either::Left((None, _)) => Next::AllEnded,
                    Either::Right(((), _)) => Next::TimedOut,
                },
            };
            match next {
                Next::AllEnded => return Ok(()),
                Next::TimedOut => {
                    let left: Vec<_> = named
                        .iter()
                        .filter(|(_, _, ended)| !ended.get())
                        .map(|(handle, record, _)| (handle.clone(), Rc::clone(record)))
                        .collect();
                    self.go_on_without(waiting, &left, began);
                    return Ok(());
                }
                Next::One((_, Ok(()))) => {}
                Next::One((_, Err(cause))) => {
                    self.go_on_without(waiting, &[], began);
                    return Err(cause);
                }
            }
        }
    }

    /// The steps still `waiting` go on apart, and what each ends with is written
    /// down. `left` are those a pass's wait ran out on: the log names them, and the
    /// message on its way into each, and says how long each step took once it ends,
    /// from `began`, when the pass began waiting.
    fn go_on_without(
        self: &Rc<Self>,
        mut waiting: Waiting,
        left: &[(String, Rc<Record>)],
        began: i64,
    ) {
        if waiting.is_empty() {
            return;
        }
        let told = !left.is_empty() && self.tell_left(left);
        let this = Rc::clone(self);
        self.seams.spawn.spawn(Box::pin(async move {
            while let Some((handle, answer)) = waiting.next().await {
                if let Err(cause) = answer {
                    this.seams.log.error("a step failed", &cause.to_string());
                }
                if told {
                    let took = (this.now() - began) / 1000;
                    this.seams.log.warn(&format!(
                        "the step of @{handle} ended {took} s after its pass began"
                    ));
                }
            }
        }));
    }

    /// Says which windows a pass went on without, unless it was said less than
    /// [`LEFT_TOLD_MS`] ago: whether it said so.
    fn tell_left(&self, left: &[(String, Rc<Record>)]) -> bool {
        let now = self.now();
        let state = &self.steps;
        if state
            .told
            .get()
            .is_some_and(|told| now - told < LEFT_TOLD_MS)
        {
            state.unsaid.set(state.unsaid.get() + 1);
            return false;
        }
        let windows: Vec<String> = left
            .iter()
            .map(|(handle, record)| {
                let delivering = record
                    .delivery
                    .borrow()
                    .delivering
                    .as_ref()
                    .map(|d| d.message);
                match delivering {
                    Some(message) => format!("@{handle} (m-{message} on its way)"),
                    None => format!("@{handle}"),
                }
            })
            .collect();
        let unsaid = match state.unsaid.replace(0) {
            0 => String::new(),
            more => format!(" ({more} passes more went on without some since the last line)"),
        };
        self.seams.log.warn(&format!(
            "a pass went on after {} s without the steps of {}{unsaid}",
            STEP_WAIT.as_secs(),
            windows.join(", ")
        ));
        state.told.set(Some(now));
        true
    }
}
