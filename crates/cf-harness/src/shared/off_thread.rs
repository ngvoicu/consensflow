//! A launch's plan reads the project's folder and its ancestors, looks for the harness's program
//! and writes the launch's files, and each of those can wait with no limit (a folder macOS asks
//! the person about, a share, a slow disk). The daemon runs its engine on one thread, so such a
//! call made there holds every pass, the API and the heartbeat (T-2203).
//!
//! [`plan`] makes them on a thread of its own and waits for them as for any other work: the
//! plan is pending, the engine's thread goes on. It gives up on nothing. A write has no bound
//! that stops it half-way, and a plan whose future is dropped finishes what it had begun.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};

use cf_base::env::Env;
use cf_proto::specs::Variant;
use tokio::sync::oneshot;

use crate::contract::{Agent, Launch, LaunchId, Skill, Specs};

type Job = Box<dyn FnOnce() + Send>;

/// The plans the worker has been given and has not finished.
static BUSY: AtomicUsize = AtomicUsize::new(0);

/// The plans the worker has finished, ever.
static DONE: AtomicUsize = AtomicUsize::new(0);

/// How many plans the worker has finished: a test that polls by hand sees one end between two
/// polls by this moving.
pub(crate) fn finished_count() -> usize {
    DONE.load(Ordering::SeqCst)
}

/// Waits until the worker has finished what it was given, and says whether there was any:
/// a test that polls a plan by hand has nothing else to be woken by.
pub(crate) fn wait_idle() -> bool {
    let began = std::time::Instant::now();
    let mut waited = false;
    while BUSY.load(Ordering::SeqCst) > 0 && began.elapsed() < std::time::Duration::from_secs(20) {
        waited = true;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    waited
}

/// Counts a job as given, and as finished once it has been run.
fn counted(job: Job) -> Job {
    BUSY.fetch_add(1, Ordering::SeqCst);
    Box::new(move || {
        job();
        DONE.fetch_add(1, Ordering::SeqCst);
        BUSY.fetch_sub(1, Ordering::SeqCst);
    })
}

/// The worker's queue: one thread for every plan, which makes them one at a time as the engine
/// began them.
fn queue() -> &'static Mutex<mpsc::Sender<Job>> {
    static QUEUE: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (jobs, work) = mpsc::channel::<Job>();
        let spawned = std::thread::Builder::new()
            .name("launch-plan".to_owned())
            .spawn(move || {
                while let Ok(job) = work.recv() {
                    job();
                }
            });
        drop(spawned);
        Mutex::new(jobs)
    })
}

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
    agent: Option<(Option<String>, Option<String>, Option<String>, bool)>,
    instructions: String,
    specs: Option<(Variant, String, Skill)>,
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

/// What `work` makes of `launch` and `env`, made on the plan's worker thread and waited for as
/// long as it takes. Fails only where the worker is gone, or `work` panicked.
pub(crate) async fn plan<T: Send + 'static>(
    env: &Env,
    launch: &Launch<'_>,
    work: impl FnOnce(&Env, &Launch<'_>) -> T + Send + 'static,
) -> Result<T, String> {
    let (env, launch) = (env.clone(), Owned::of(launch));
    let (answer, answered) = oneshot::channel();
    let job: Job = Box::new(move || {
        if let Ok(made) = catch_unwind(AssertUnwindSafe(|| {
            launch.with(|launch| work(&env, launch))
        })) {
            let _ = answer.send(made);
        }
    });
    let sent = queue().lock().map(|jobs| jobs.send(counted(job)));
    if !matches!(sent, Ok(Ok(()))) {
        return Err("the launch's files could not be made: their worker is gone".to_owned());
    }
    answered
        .await
        .map_err(|_| "the launch's files could not be made: their worker failed".to_owned())
}

/// What `work` makes of `env`, made on the plan's worker thread, for a part of a plan that needs
/// no more of the launch than what it names itself.
pub(crate) async fn work<T: Send + 'static>(
    env: &Env,
    work: impl FnOnce(&Env) -> T + Send + 'static,
) -> Result<T, String> {
    let env = env.clone();
    let (answer, answered) = oneshot::channel();
    let job: Job = Box::new(move || {
        if let Ok(made) = catch_unwind(AssertUnwindSafe(|| work(&env))) {
            let _ = answer.send(made);
        }
    });
    let sent = queue().lock().map(|jobs| jobs.send(counted(job)));
    if !matches!(sent, Ok(Ok(()))) {
        return Err("the launch's files could not be made: their worker is gone".to_owned());
    }
    answered
        .await
        .map_err(|_| "the launch's files could not be made: their worker failed".to_owned())
}
