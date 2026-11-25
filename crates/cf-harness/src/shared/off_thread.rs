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

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cf_base::env::Env;
use cf_proto::specs::Variant;
use tokio::sync::oneshot;

use crate::contract::{Agent, Launch, LaunchId, Skill, Specs};

/// How long a step of a plan is waited for.
pub const DEADLINE: Duration = Duration::from_secs(60);

/// The variable that sets how long, in milliseconds: a test of a step that does not end would
/// otherwise wait a minute; nothing in a person's environment sets it.
const DEADLINE_VARIABLE: &str = "CONSENSFLOW_PLAN_DEADLINE_MS";

/// The steps of a launch's plan, as a note names the one that did not end.
pub mod step {
    pub const CLAUDE: &str = "reading the project's folder and writing the launch's files";
    pub const FILES: &str = "writing the launch's files";
    pub const PROGRAM: &str = "finding the harness's program";
    pub const CONFIGURATION: &str = "reading the harness's configuration";
    pub const SETTINGS: &str = "reading the project's settings";
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

thread_local! {
    /// The calls made off this thread that have not ended.
    static OUTSTANDING: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
}

/// Whether a call this thread asked for is still being made: a test that polls a plan by hand
/// waits for its wake only while one is.
pub(crate) fn outstanding() -> bool {
    OUTSTANDING.with(|count| count.load(Ordering::SeqCst) > 0)
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
async fn apart<T: Send + 'static>(
    env: &Env,
    step: &'static str,
    call: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let deadline = env
        .text(DEADLINE_VARIABLE)
        .and_then(|ms| ms.parse().ok())
        .map_or(DEADLINE, Duration::from_millis);
    let (answer, answer_of) = oneshot::channel::<Option<T>>();
    let answer: Answer<T> = Arc::new(Mutex::new(Some(answer)));
    let (ended, ending) = channel::<()>();
    let outstanding = OUTSTANDING.with(Arc::clone);
    outstanding.fetch_add(1, Ordering::SeqCst);
    let (made, counted) = (Arc::clone(&answer), Arc::clone(&outstanding));
    let spawned = std::thread::Builder::new()
        .name("launch-plan".to_owned())
        .spawn(move || {
            if let Ok(value) = catch_unwind(AssertUnwindSafe(call)) {
                answered(&made, Some(value));
            }
            // A call that panicked: its sender is let go, which says so.
            drop(made);
            let _ = ended.send(());
            counted.fetch_sub(1, Ordering::SeqCst);
        });
    if spawned.is_err() {
        outstanding.fetch_sub(1, Ordering::SeqCst);
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
    match answer_of.await {
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
