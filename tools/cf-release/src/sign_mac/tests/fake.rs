//! A script in place of Apple's tools. It records every call as a line, and
//! answers each from a table: the release's tools all working, unless a test
//! says otherwise. Any call can be made to fail by its place in the run or by
//! what it starts with, and every call can say a text (with the run's secrets in
//! it, for the tests that keep them out of what the run says).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::time::Duration;

use super::super::tools::Runner;
use crate::process::{Output, Unstarted};

/// How a call's line is told from the others: the whole line, or how it begins.
enum Line {
    Is(&'static str),
    Starts(&'static str),
}

impl Line {
    fn matches(&self, line: &str) -> bool {
        match self {
            Self::Is(whole) => line == *whole,
            Self::Starts(start) => line.starts_with(start),
        }
    }

    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Is(a), Self::Is(b)) | (Self::Starts(a), Self::Starts(b)) => a == b,
            _ => false,
        }
    }
}

pub fn said(stdout: &str) -> Output {
    Output {
        code: 0,
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

pub fn refused(code: i32, stdout: &str, stderr: &str) -> Output {
    Output {
        code,
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    }
}

pub struct Fake {
    calls: RefCell<Vec<String>>,
    waits: RefCell<Vec<Duration>>,
    /// What each kind of call is answered with, one answer after another; the last stays.
    answers: RefCell<Vec<(Line, VecDeque<Output>)>>,
    /// The place of the one call that fails, counting from 0.
    failing: Cell<Option<usize>>,
    /// Calls that fail while the number against them is more than 0, which each one takes one from.
    refusing: RefCell<Vec<(&'static str, usize)>>,
    /// Programs that cannot be started.
    missing: RefCell<Vec<&'static str>>,
    /// What every call says on its error stream, and a failing one on its output too.
    chatter: RefCell<String>,
    /// Looks at the arguments of the calls that start with a text, as they are made.
    watchers: Vec<(&'static str, Watcher)>,
}

type Watcher = Box<dyn Fn(&[OsString])>;

impl Fake {
    /// What `security list-keychains -d user` lists: the search list the run must leave as it was.
    pub const SEARCHED: [&str; 2] = [
        "/Users/runner/Library/Keychains/login.keychain-db",
        "/Library/Keychains/System.keychain",
    ];

    /// The release's tools, all working: two keychains in the user's search
    /// list, one Developer ID identity, the notary accepting what it is given, and
    /// Gatekeeper taking it as notarized.
    pub fn new() -> Self {
        let [login, system] = Self::SEARCHED;
        let identity = " 1) 0123456789ABCDEF0123456789ABCDEF01234567 \
                        \"Developer ID Application: ConsensFlow (TEAMID1234)\"\n     \
                        1 valid identities found\n";
        let accepted = r#"{"id":"2efe2717-52ef-43a5-96dc-0797e4ca1041","status":"Accepted"}"#;
        let assessed = Output {
            code: 0,
            stdout: String::new(),
            stderr: "accepted\nsource=Notarized Developer ID\n".to_string(),
        };
        let fake = Self {
            calls: RefCell::default(),
            waits: RefCell::default(),
            answers: RefCell::default(),
            failing: Cell::new(None),
            refusing: RefCell::default(),
            missing: RefCell::default(),
            chatter: RefCell::default(),
            watchers: Vec::new(),
        };
        fake.answer(
            Line::Is("security list-keychains -d user"),
            [said(&format!("    \"{login}\"\n    \"{system}\"\n"))],
        );
        fake.answer(Line::Starts("security find-identity"), [said(identity)]);
        fake.answer(
            Line::Starts("plutil -extract CFBundleIdentifier"),
            [said("dev.ngvoicu.consensflow\n")],
        );
        fake.answer(
            Line::Starts("plutil -extract CFBundleExecutable"),
            [said("app\n")],
        );
        fake.answer(Line::Starts("xcrun notarytool submit"), [said(accepted)]);
        fake.answer(Line::Starts("spctl"), [assessed]);
        fake
    }

    fn answer(&self, line: Line, outputs: impl IntoIterator<Item = Output>) {
        let mut answers = self.answers.borrow_mut();
        answers.retain(|(known, _)| !known.same(&line));
        answers.push((line, outputs.into_iter().collect()));
    }

    /// A call that starts with `start` is answered with `output`.
    pub fn answering(self, start: &'static str, output: Output) -> Self {
        self.answer(Line::Starts(start), [output]);
        self
    }

    /// The calls that start with `start` are answered with `outputs`, one each in
    /// turn; the last answers the rest.
    pub fn answering_in_turn(
        self,
        start: &'static str,
        outputs: impl IntoIterator<Item = Output>,
    ) -> Self {
        self.answer(Line::Starts(start), outputs);
        self
    }

    /// The `index`th call fails, counting from 0.
    pub fn failing_at(self, index: usize) -> Self {
        self.failing.set(Some(index));
        self
    }

    /// The first `times` calls that start with `start` fail.
    pub fn refusing(self, start: &'static str, times: usize) -> Self {
        self.refusing.borrow_mut().push((start, times));
        self
    }

    /// `program` cannot be started.
    pub fn without(self, program: &'static str) -> Self {
        self.missing.borrow_mut().push(program);
        self
    }

    /// Every call says `text` on its error stream, and a failing one on its output as well.
    pub fn saying(self, text: &str) -> Self {
        *self.chatter.borrow_mut() = text.to_string();
        self
    }

    /// `look` is shown the arguments of each call that starts with `start`, as it
    /// is made: what a file the call reads holds is then there to be seen.
    pub fn watching(mut self, start: &'static str, look: impl Fn(&[OsString]) + 'static) -> Self {
        self.watchers.push((start, Box::new(look)));
        self
    }

    /// Every call made, as a line each: the program and its arguments.
    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// Every time the run waited.
    pub fn waits(&self) -> Vec<Duration> {
        self.waits.borrow().clone()
    }

    fn fails(&self, index: usize, line: &str) -> bool {
        if self.failing.get() == Some(index) {
            return true;
        }
        let mut refusing = self.refusing.borrow_mut();
        refusing
            .iter_mut()
            .find(|(start, times)| line.starts_with(*start) && *times > 0)
            .is_some_and(|(_, times)| {
                *times -= 1;
                true
            })
    }
}

impl Runner for Fake {
    fn capture(&self, program: &str, args: &[OsString]) -> Result<Output, Unstarted> {
        let words: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
        let line = format!("{program} {}", words.join(" "));
        let index = self.calls.borrow().len();
        self.calls.borrow_mut().push(line.clone());
        if self.missing.borrow().contains(&program) {
            return Err(Unstarted::NotFound {
                program: program.to_string(),
            });
        }
        for (start, look) in &self.watchers {
            if line.starts_with(start) {
                look(args);
            }
        }
        let chatter = self.chatter.borrow().clone();
        if self.fails(index, &line) {
            return Ok(refused(1, &chatter, &chatter));
        }
        let mut answers = self.answers.borrow_mut();
        let mut output = answers
            .iter_mut()
            .find(|(known, _)| known.matches(&line))
            .and_then(|(_, outputs)| {
                if outputs.len() > 1 {
                    outputs.pop_front()
                } else {
                    outputs.front().cloned()
                }
            })
            .unwrap_or_else(|| said(""));
        output.stderr.push_str(&chatter);
        Ok(output)
    }

    fn wait(&self, time: Duration) {
        self.waits.borrow_mut().push(time);
    }

    /// A password of 00 01 02…, and a folder named abcdef.
    fn random(&self, bytes: &mut [u8]) -> io::Result<()> {
        for (place, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::try_from(place).unwrap();
        }
        Ok(())
    }
}
