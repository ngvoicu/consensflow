//! The harness admin's two seams, scripted, the twins of the Node
//! recorder's (`tests/goldens/admin`): the programs it runs, answered by the
//! program's name and arguments as [`ScriptedProcesses`](super::ScriptedProcesses)
//! answers them, and the latest release of each harness. Each answer comes at
//! once or when the test releases it, and what was asked is kept.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::Duration;

use cf_process::{CaptureFailed, Captured, Limits};
use cf_proto::agents::Harness;

use super::{named, Later};
use crate::admin::feed::{Answer, Network, Request, TERMINATED};
use crate::admin::{Capture, Latest, Source};
use crate::contract::Work;
use crate::seams::processes::Program;

/// How a scripted seam answers one call.
#[derive(Debug, Clone)]
pub enum Response<T> {
    /// At once.
    Now(T),
    /// When the test releases it.
    Held,
}

/// What a scripted program answers: what it wrote, or how it failed.
pub type Said = Result<Captured, CaptureFailed>;

/// What a scripted feed answers: the release, or the words of why not.
pub type Told = Result<String, String>;

/// A held answer, once given.
struct Waiting<T>(Rc<Later<T>>);

impl<T> Future for Waiting<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<T> {
        self.0.poll_take(context)
    }
}

/// The answers scripted for each key, in the order they are asked, and the
/// held ones waiting to be released.
struct Script<T> {
    answers: RefCell<HashMap<String, VecDeque<Response<T>>>>,
    held: RefCell<VecDeque<(String, Rc<Later<T>>)>>,
}

impl<T> Default for Script<T> {
    fn default() -> Self {
        Self {
            answers: RefCell::new(HashMap::new()),
            held: RefCell::new(VecDeque::new()),
        }
    }
}

impl<T: 'static> Script<T> {
    fn push(&self, key: &str, replies: impl IntoIterator<Item = Response<T>>) {
        self.answers
            .borrow_mut()
            .entry(key.to_owned())
            .or_default()
            .extend(replies);
    }

    /// The next answer scripted for `key`, `missing` when none is left.
    fn ask(&self, key: &str, missing: impl FnOnce() -> T) -> Work<'static, T> {
        let next = self
            .answers
            .borrow_mut()
            .get_mut(key)
            .and_then(VecDeque::pop_front);
        match next {
            Some(Response::Now(answer)) => Box::pin(async move { answer }),
            Some(Response::Held) => {
                let slot = Rc::new(Later::default());
                self.held
                    .borrow_mut()
                    .push_back((key.to_owned(), Rc::clone(&slot)));
                Box::pin(Waiting(slot))
            }
            None => {
                let answer = missing();
                Box::pin(async move { answer })
            }
        }
    }

    /// Answers the call to `key` held longest: whether one was held.
    fn release(&self, key: &str, answer: T) -> bool {
        let mut held = self.held.borrow_mut();
        let Some(at) = held.iter().position(|(held, _)| held == key) else {
            return false;
        };
        let released = held.remove(at);
        drop(held);
        if let Some((_, slot)) = released {
            slot.give(answer);
        }
        true
    }

    /// The keys with answers scripted and not yet asked for, sorted.
    fn unused(&self) -> Vec<String> {
        let mut unused: Vec<String> = self
            .answers
            .borrow()
            .iter()
            .filter(|(_, left)| !left.is_empty())
            .map(|(key, _)| key.clone())
            .collect();
        unused.sort();
        unused
    }
}

/// Programs the admin runs, answered by the program's name and its
/// arguments (`codex --version`, `brew upgrade --cask codex`). One asked for
/// with no answer left does not start, as a stand-in that is not there.
#[derive(Default)]
pub struct ScriptedCapture {
    script: Script<Said>,
    ran: RefCell<Vec<(Program, Limits)>>,
}

impl ScriptedCapture {
    /// Scripts what `named` answers each time it is run, after the answers
    /// scripted for it already.
    pub fn answer(&self, named: &str, replies: impl IntoIterator<Item = Response<Said>>) {
        self.script.push(named, replies);
    }

    /// Scripts a program that wrote `stdout` and ended with 0.
    pub fn says(&self, named: &str, stdout: &str) {
        self.answer(
            named,
            [Response::Now(Ok(Captured {
                stdout: stdout.to_owned(),
                stderr: String::new(),
            }))],
        );
    }

    /// Answers the run of `named` held longest: whether one was held.
    pub fn release(&self, named: &str, answer: Said) -> bool {
        self.script.release(named, answer)
    }

    /// Every program run since the last time they were taken, as it was
    /// given, with its limits.
    pub fn take_ran(&self) -> Vec<(Program, Limits)> {
        std::mem::take(&mut self.ran.borrow_mut())
    }

    /// The programs with answers scripted and not yet asked for.
    pub fn unused(&self) -> Vec<String> {
        self.script.unused()
    }
}

impl Capture for ScriptedCapture {
    fn capture(&self, program: Program, limits: Limits) -> Work<'_, Said> {
        let named = named(&program);
        self.ran.borrow_mut().push((program, limits));
        self.script.ask(&named, || {
            Err(CaptureFailed {
                message: format!("spawn {named} ENOENT"),
                code: None,
                killed: false,
                stdout: String::new(),
                stderr: String::new(),
            })
        })
    }
}

/// The latest release of each harness, answered in the order asked.
#[derive(Default)]
pub struct ScriptedLatest {
    script: Script<Told>,
    asked: RefCell<Vec<(Harness, Source)>>,
}

impl ScriptedLatest {
    /// Scripts what the feed of `id` answers each time it is asked, after
    /// the answers scripted for it already.
    pub fn answer(&self, id: Harness, replies: impl IntoIterator<Item = Response<Told>>) {
        self.script.push(id.as_str(), replies);
    }

    /// Scripts the release the feed of `id` says once.
    pub fn says(&self, id: Harness, release: &str) {
        self.answer(id, [Response::Now(Ok(release.to_owned()))]);
    }

    /// Answers the call for `id` held longest: whether one was held.
    pub fn release(&self, id: Harness, answer: Told) -> bool {
        self.script.release(id.as_str(), answer)
    }

    /// Every feed asked since the last time they were taken, with the source
    /// it was asked of.
    pub fn take_asked(&self) -> Vec<(Harness, Source)> {
        std::mem::take(&mut self.asked.borrow_mut())
    }

    /// The harnesses with answers scripted and not yet asked for.
    pub fn unused(&self) -> Vec<String> {
        self.script.unused()
    }
}

impl Latest for ScriptedLatest {
    fn latest<'a>(&'a self, id: Harness, source: &'a Source) -> Work<'a, Told> {
        self.asked.borrow_mut().push((id, source.clone()));
        self.script.ask(id.as_str(), || {
            Err(format!("no release scripted for {}", id.as_str()))
        })
    }
}

/// How the body of a scripted answer ends once its chunks are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyEnding {
    /// At its end.
    Whole,
    /// The connection closes: the words of [`TERMINATED`].
    Cut,
    /// It never ends: only the request's time does.
    Never,
}

/// What a scripted network does with one request.
#[derive(Debug, Clone)]
pub enum Delivery {
    /// The head never arrives.
    Silence,
    /// The connection fails, in these words.
    Failure(String),
    /// An answer of this status, the chunks of its body and how it ends.
    Answer {
        status: u16,
        chunks: Vec<Vec<u8>>,
        ending: BodyEnding,
    },
}

/// A request as the network was asked it: its address, its time, and whether
/// a redirect would be followed.
pub type Asked = (String, Duration, bool);

/// A network that serves each request as scripted, in the order asked, and
/// keeps what it was asked and how many chunks of bodies were read.
#[derive(Default)]
pub struct ScriptedNetwork {
    served: RefCell<VecDeque<Delivery>>,
    asked: RefCell<Vec<Asked>>,
    chunks_read: Rc<Cell<usize>>,
}

impl ScriptedNetwork {
    /// Scripts what the next request left is served.
    pub fn serve(&self, served: Delivery) {
        self.served.borrow_mut().push_back(served);
    }

    /// Every request asked since the last time they were taken.
    pub fn take_asked(&self) -> Vec<Asked> {
        std::mem::take(&mut self.asked.borrow_mut())
    }

    /// How many chunks of bodies were read in all.
    pub fn chunks_read(&self) -> usize {
        self.chunks_read.get()
    }

    /// How many scripted answers were never asked for.
    pub fn unused(&self) -> usize {
        self.served.borrow().len()
    }
}

/// The body of a scripted answer.
struct ScriptedAnswer {
    status: u16,
    chunks: VecDeque<Vec<u8>>,
    ending: BodyEnding,
    read: Rc<Cell<usize>>,
}

impl Answer for ScriptedAnswer {
    fn status(&self) -> u16 {
        self.status
    }

    fn chunk(&mut self) -> Work<'_, Result<Option<Vec<u8>>, String>> {
        Box::pin(async move {
            if let Some(chunk) = self.chunks.pop_front() {
                self.read.set(self.read.get() + 1);
                return Ok(Some(chunk));
            }
            match self.ending {
                BodyEnding::Whole => Ok(None),
                BodyEnding::Cut => Err(TERMINATED.to_owned()),
                BodyEnding::Never => std::future::pending().await,
            }
        })
    }
}

impl Network for ScriptedNetwork {
    fn get<'a>(&'a self, request: &'a Request<'a>) -> Work<'a, Result<Box<dyn Answer>, String>> {
        self.asked.borrow_mut().push((
            request.url.to_owned(),
            request.timeout,
            request.follow_redirects,
        ));
        let served = self.served.borrow_mut().pop_front();
        let read = Rc::clone(&self.chunks_read);
        Box::pin(async move {
            match served {
                Some(Delivery::Answer {
                    status,
                    chunks,
                    ending,
                }) => Ok(Box::new(ScriptedAnswer {
                    status,
                    chunks: chunks.into(),
                    ending,
                    read,
                }) as Box<dyn Answer>),
                Some(Delivery::Failure(words)) => Err(words),
                Some(Delivery::Silence) => std::future::pending().await,
                None => Err("nothing scripted".to_owned()),
            }
        })
    }
}
