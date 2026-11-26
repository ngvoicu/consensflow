//! A launch's plan reads the project's folder and its ancestors, looks for the harness's program
//! and writes the launch's files, and each of those can wait with no limit (a folder macOS asks
//! the person about, a share, a slow disk). The daemon runs its engine on one thread, so such a
//! call made there holds every pass, the API and the heartbeat (T-2203).
//!
//! [`plan`] and [`work`] make them on a thread of their own, one for each call, and wait for it
//! as for any other work: the plan is pending, the engine's thread goes on, and a call that never
//! ends holds no other launch's (T-2232). A call is waited for [`DEADLINE`] at most: past it the
//! launch fails in words that name the step ([`late_step`]), and its thread is left to end by
//! itself. A write is made beside and renamed into place (`cf_base::file::write_whole`), so one
//! left behind half way leaves no half a file.

use std::cell::{Cell, RefCell};
use std::future::{poll_fn, Future};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cf_base::env::Env;
use cf_proto::specs::Variant;
use tokio::sync::oneshot;

use crate::contract::{Agent, Launch, LaunchId, Skill, Specs};

/// How long a step of a plan is waited for.
const DEADLINE: Duration = Duration::from_secs(60);

/// The variable that sets how long, in milliseconds: a test of a step that does not end would
/// otherwise wait a minute; nothing in a person's environment sets it.
const DEADLINE_VARIABLE: &str = "CONSENSFLOW_PLAN_DEADLINE_MS";

/// The steps of a launch's plan, as a note names the one that did not end.
pub(crate) mod step {
    pub(crate) const CLAUDE: &str = "reading the project's folder and writing the launch's files";
    pub(crate) const FILES: &str = "writing the launch's files";
    pub(crate) const PROGRAM: &str = "finding the harness's program";
    pub(crate) const CONFIGURATION: &str = "reading the harness's configuration";
    pub(crate) const SETTINGS: &str = "reading the project's settings";
}

const STEPS: [&str; 5] = [
    step::CLAUDE,
    step::FILES,
    step::PROGRAM,
    step::CONFIGURATION,
    step::SETTINGS,
];

/// What a plan says of a step that did not end in its time.
fn late(step: &str, deadline: Duration) -> String {
    format!(
        "{step} took more than {} s",
        deadline.as_millis().div_ceil(1000)
    )
}

/// The step `said` names as one that did not end in its time: its words and the seconds it was
/// given, where `said` is nothing else. Words of ConsensFlow's own, which a note may carry.
pub fn late_step(said: &str) -> Option<(&'static str, u64)> {
    let (words, seconds) = said.strip_suffix(" s")?.rsplit_once(" took more than ")?;
    let step = STEPS.into_iter().find(|step| *step == words)?;
    Some((step, seconds.parse().ok()?))
}

/// What a step says where the launch it belongs to is over: nothing of it is told to anybody,
/// as whoever prepared the launch finds it over too.
const OVER: &str = "the launch was called off before its window opened";

/// Whether a launch is still wanted, as its preparer says it.
type Wanted = Rc<dyn Fn() -> bool>;

thread_local! {
    /// What says whether the launch being polled on this thread is wanted.
    static WANTED: RefCell<Option<Wanted>> = const { RefCell::new(None) };
    /// Whether a poll on this thread found a call made off it still unanswered.
    static AWAITING: Cell<bool> = const { Cell::new(false) };
}

/// Whether the launch being prepared is still wanted: true outside [`while_wanted`].
pub fn launch_wanted() -> bool {
    let wanted = WANTED.with(|wanted| wanted.borrow().clone());
    wanted.is_none_or(|wanted| wanted())
}

/// Prepares a launch that is wanted for as long as `wanted` says so: no step of it made off the
/// thread is begun, and none is gone on from, once it says no, so that whoever waits for the
/// launch (a project's Close) waits for one step at most (T-2263).
pub async fn while_wanted<T>(wanted: Wanted, work: impl Future<Output = T>) -> T {
    /// Puts back what was said before, however the poll ends.
    struct Back(Option<Wanted>);
    impl Drop for Back {
        fn drop(&mut self) {
            WANTED.with(|wanted| *wanted.borrow_mut() = self.0.take());
        }
    }
    let mut work = std::pin::pin!(work);
    poll_fn(move |context| {
        let _back = Back(WANTED.with(|said| said.borrow_mut().replace(Rc::clone(&wanted))));
        work.as_mut().poll(context)
    })
    .await
}

/// Whether a poll since the last time this was asked found a call made off this thread still
/// unanswered, which will wake it: a test that polls a plan by hand waits for that wake only
/// then. It is what the polls of one driver waited on, and no other's.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn took_awaiting() -> bool {
    AWAITING.with(|awaiting| awaiting.replace(false))
}

/// The answer's way back: the call's, or that it was late, whichever comes first.
type Answer<T> = Arc<Mutex<Option<oneshot::Sender<Option<T>>>>>;

fn answered<T>(to: &Answer<T>, answer: Option<T>) {
    let sender = to.lock().ok().and_then(|mut sender| sender.take());
    if let Some(sender) = sender {
        let _ = sender.send(answer);
    }
}

/// What `call` makes, made on a thread of its own and waited for no longer than the deadline.
/// A launch that is over ([`while_wanted`]) begins none, and goes on from none.
async fn apart<T: Send + 'static>(
    env: &Env,
    step: &'static str,
    call: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    if !launch_wanted() {
        return Err(OVER.to_owned());
    }
    let deadline = env
        .text(DEADLINE_VARIABLE)
        .and_then(|ms| ms.parse().ok())
        .map_or(DEADLINE, Duration::from_millis);
    let (answer, answer_of) = oneshot::channel::<Option<T>>();
    let answer: Answer<T> = Arc::new(Mutex::new(Some(answer)));
    let (ended, ending) = channel::<()>();
    let made = Arc::clone(&answer);
    let spawned = std::thread::Builder::new()
        .name("launch-plan".to_owned())
        .spawn(move || {
            if let Ok(value) = catch_unwind(AssertUnwindSafe(call)) {
                answered(&made, Some(value));
            }
            // A call that panicked: its sender is let go, which says so.
            drop(made);
            let _ = ended.send(());
        });
    if spawned.is_err() {
        return Err("the launch's files could not be made: no thread to make them".to_owned());
    }
    let watched = std::thread::Builder::new()
        .name("launch-plan-time".to_owned())
        .spawn(move || {
            if ending.recv_timeout(deadline) == Err(RecvTimeoutError::Timeout) {
                answered(&answer, None);
            }
        });
    if watched.is_err() {
        return Err("the launch's files could not be made: no thread to time them".to_owned());
    }
    let mut answer_of = answer_of;
    let answered = poll_fn(|context| {
        let polled = Pin::new(&mut answer_of).poll(context);
        if polled.is_pending() {
            AWAITING.with(|awaiting| awaiting.set(true));
        }
        polled
    })
    .await;
    match answered {
        Ok(Some(_)) if !launch_wanted() => Err(OVER.to_owned()),
        Ok(Some(value)) => Ok(value),
        Ok(None) => Err(late(step, deadline)),
        Err(_) => Err("the launch's files could not be made: their worker failed".to_owned()),
    }
}

/// An agent's model, effort and thinking, and whether it draws, made its own.
type OwnedAgent = (Option<String>, Option<String>, Option<String>, bool);

/// A chief's specs, made their own.
type OwnedSpecs = (Variant, String, Skill);

/// A launch with what it borrows made its own, to cross to the worker.
struct Owned {
    id: LaunchId,
    project: i64,
    handle: String,
    role: String,
    directory: String,
    resume: Option<String>,
    previous_gone: bool,
    message: Option<String>,
    agent: Option<OwnedAgent>,
    instructions: String,
    specs: Option<OwnedSpecs>,
    compaction: Option<u32>,
    advisor_off: bool,
    suggestion_off: bool,
    first_message_ms: i64,
    starts_given_up: u32,
}

impl Owned {
    fn of(launch: &Launch<'_>) -> Self {
        let own = |text: &str| text.to_owned();
        Self {
            id: launch.id.clone(),
            project: launch.project,
            handle: own(launch.handle),
            role: own(launch.role),
            directory: own(launch.directory),
            resume: launch.resume.map(own),
            previous_gone: launch.previous_gone,
            message: launch.message.map(own),
            agent: launch.agent.map(|agent| {
                (
                    agent.model.map(own),
                    agent.effort.map(own),
                    agent.thinking.map(own),
                    agent.designer,
                )
            }),
            instructions: own(launch.instructions),
            specs: launch
                .specs
                .map(|specs| (specs.variant, own(specs.folder), specs.skill.clone())),
            compaction: launch.compaction,
            advisor_off: launch.advisor_off,
            suggestion_off: launch.suggestion_off,
            first_message_ms: launch.first_message_ms,
            starts_given_up: launch.starts_given_up,
        }
    }

    /// Calls `work` with the launch as it was given.
    fn with<T>(&self, work: impl FnOnce(&Launch<'_>) -> T) -> T {
        let agent = self
            .agent
            .as_ref()
            .map(|(model, effort, thinking, designer)| Agent {
                model: model.as_deref(),
                effort: effort.as_deref(),
                thinking: thinking.as_deref(),
                designer: *designer,
            });
        let specs = self.specs.as_ref().map(|(variant, folder, skill)| Specs {
            variant: *variant,
            folder,
            skill,
        });
        work(&Launch {
            id: &self.id,
            project: self.project,
            handle: &self.handle,
            role: &self.role,
            directory: &self.directory,
            resume: self.resume.as_deref(),
            previous_gone: self.previous_gone,
            message: self.message.as_deref(),
            agent,
            instructions: &self.instructions,
            specs,
            compaction: self.compaction,
            advisor_off: self.advisor_off,
            suggestion_off: self.suggestion_off,
            first_message_ms: self.first_message_ms,
            starts_given_up: self.starts_given_up,
        })
    }
}

/// What `work` makes of `launch` and `env`, made on a thread of its own as the step `step`.
pub(crate) async fn plan<T: Send + 'static>(
    env: &Env,
    launch: &Launch<'_>,
    step: &'static str,
    work: impl FnOnce(&Env, &Launch<'_>) -> T + Send + 'static,
) -> Result<T, String> {
    let (own, launch) = (env.clone(), Owned::of(launch));
    apart(env, step, move || launch.with(|launch| work(&own, launch))).await
}

/// What `work` makes of `env`, made on a thread of its own as the step `step`, for a part of a
/// plan that needs no more of the launch than what it names itself.
pub(crate) async fn work<T: Send + 'static>(
    env: &Env,
    step: &'static str,
    work: impl FnOnce(&Env) -> T + Send + 'static,
) -> Result<T, String> {
    let own = env.clone();
    apart(env, step, move || work(&own)).await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{Context, Waker};

    use super::*;
    use crate::testing::{finished, Driver};
    use crate::{launch_wanted, while_wanted};

    /// A launch that is over between two of its steps ends at the end of the first: the second
    /// is never begun (T-2263, finding 12).
    #[test]
    fn a_launch_that_is_over_begins_no_further_step() {
        let env = Env::from_vars(std::iter::empty::<(String, String)>());
        let over = Rc::new(Cell::new(false));
        let wanted = {
            let over = Rc::clone(&over);
            Rc::new(move || !over.get())
        };
        let second = Arc::new(AtomicBool::new(false));
        let ran = Arc::clone(&second);
        let closes = Rc::clone(&over);
        let answer = finished(Box::pin(while_wanted(wanted, async move {
            let first = work(&env, step::FILES, |_| ());
            // The Close comes while the first step is made.
            closes.set(true);
            first.await?;
            work(&env, step::FILES, move |_| {
                ran.store(true, Ordering::SeqCst)
            })
            .await
        })));
        assert!(answer.is_err(), "the launch went on: {answer:?}");
        assert!(!second.load(Ordering::SeqCst), "the second step was made");
        assert!(launch_wanted(), "outside a launch, everything is wanted");
    }

    /// A step whose launch was over before it began is not made.
    #[test]
    fn a_step_of_a_launch_already_over_is_not_made() {
        let env = Env::from_vars(std::iter::empty::<(String, String)>());
        let made = Arc::new(AtomicBool::new(false));
        let ran = Arc::clone(&made);
        let answer = finished(Box::pin(while_wanted(Rc::new(|| false), async move {
            work(&env, step::FILES, move |_| {
                ran.store(true, Ordering::SeqCst)
            })
            .await
        })));
        assert!(answer.is_err());
        assert!(!made.load(Ordering::SeqCst));
    }

    /// A driver waits for its own work's calls, never for another call outstanding on the same
    /// thread (T-2263, finding 8).
    #[test]
    fn a_driver_does_not_wait_for_a_call_that_is_not_its_own() {
        let env = Env::from_vars(std::iter::empty::<(String, String)>());
        let (release, held) = channel::<()>();
        let mut long = Box::pin(async move {
            work(&env, step::FILES, move |_| {
                let _ = held.recv_timeout(Duration::from_secs(30));
            })
            .await
        });
        // Begins the call, and leaves it outstanding on this thread.
        assert!(long
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        let releasing = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            let _ = release.send(());
        });
        let mut unrelated: Driver<()> = Driver::default();
        unrelated.begin(2, std::future::pending());
        let began = std::time::Instant::now();
        let settled = unrelated.run();
        let took = began.elapsed();
        assert!(settled.is_empty());
        assert!(
            took < Duration::from_secs(2),
            "the driver waited for a call that is not its own: {took:?}"
        );
        assert!(finished(long).is_ok(), "the call's own wait waits for it");
        releasing.join().unwrap();
    }
}
