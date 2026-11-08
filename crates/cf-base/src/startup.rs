//! Why a program could not be started, as a note says it (T-1328).
//!
//! A note about a window that did not start goes to the chief, a member, the board, a result or
//! the ledger, and a secret quoted in a note is a blocker. A start that failed has words of its
//! own (`C:\tools\pi hunter22.cmd is an npm shim that runs on Node …`, `spawn pi hunter22.cmd
//! ENOENT`, what a harness said on its error stream), and a file's name is the person's: it can
//! be a credential. Guessing which of those words are safe to quote failed twice (the joint
//! review T-1322 found five blockers, then two left), so **a note never carries them**. It
//! carries a [`StartFailure`]: the kind of failure, the error code where the system gave one,
//! and the advice, all of them fixed here. Its fields are a `Copy` enum and a `&'static str`,
//! so a `String` of anyone's cannot be built into one.
//!
//! The full words stay where the person reads them on their own screen: the harness's terminal,
//! and the page's own fields. Whoever produces a failure keeps them beside it ([`StartRefusal`]).

use std::fmt;

/// Which way a program could not be started or read. Each kind has its words and its advice,
/// and nothing else of a failure goes into a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartKind {
    /// The program, or the script it names, was not found.
    NotFound,
    /// The system would not let ConsensFlow run it.
    NotAllowed,
    /// An npm shim runs on Node, and no Node is to be found for it.
    NoNode,
    /// The shim Pi's own installer writes runs on Node, and no Node is to be found for it.
    NoNodeForPi,
    /// A script that is no npm shim, which only cmd.exe could run in a window.
    NotAnNpmShim,
    /// The system would not start it, for another reason.
    NotStarted,
    /// It started and did not answer in the time it was given.
    NoAnswer,
    /// It started and ended with an error: the code is the status it exited with.
    Failed,
}

/// What a failed start came to, for a note: its kind and the code of its failure where the system
/// or the program gave one. Both are closed, so no word of any program, path or screen is in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartFailure {
    kind: StartKind,
    code: Option<StartCode>,
}

/// The code of a failed start: the system's name for it (`ENOENT`, from a fixed table), or the
/// status the program exited with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartCode {
    Errno(&'static str),
    Exit(i64),
}

impl StartFailure {
    /// A failure of `kind`, with no code.
    pub const fn of(kind: StartKind) -> Self {
        Self { kind, code: None }
    }

    /// The failure the system's name `code` (`error_code` of an I/O error, or a code of the table
    /// read back from text) says: a program that was not found, one the system would not let run,
    /// or one it would not start; and `code` beside it, but for libuv's `UNKNOWN`, which names
    /// nothing.
    pub const fn of_errno(code: &'static str) -> Self {
        let kind = match code.as_bytes() {
            b"ENOENT" => StartKind::NotFound,
            b"EACCES" | b"EPERM" => StartKind::NotAllowed,
            _ => StartKind::NotStarted,
        };
        let failure = Self::of(kind);
        match code.as_bytes() {
            b"UNKNOWN" => failure,
            _ => failure.with_errno(code),
        }
    }

    /// The same, with the system's name for what went wrong (`error_code` of an I/O error).
    #[must_use]
    pub const fn with_errno(mut self, name: &'static str) -> Self {
        self.code = Some(StartCode::Errno(name));
        self
    }

    /// The same, with the status the program exited with.
    #[must_use]
    pub const fn with_exit(mut self, status: i64) -> Self {
        self.code = Some(StartCode::Exit(status));
        self
    }

    pub const fn kind(&self) -> StartKind {
        self.kind
    }

    pub const fn code(&self) -> Option<StartCode> {
        self.code
    }

    /// What failed, in plain words, for `who`'s program (`Pi`, `Codex`), with the code after it:
    /// `Pi's program was not found (ENOENT)`.
    pub fn what(&self, who: &str) -> String {
        let said = match self.kind {
            StartKind::NotFound => format!("{who}'s program was not found"),
            StartKind::NotAllowed => {
                format!("ConsensFlow may not run {who}'s program")
            }
            StartKind::NoNode | StartKind::NoNodeForPi => {
                format!("Node.js was not found for {who}")
            }
            StartKind::NotAnNpmShim => {
                format!("{who}'s program is a script that is not an npm shim")
            }
            StartKind::NotStarted => format!("{who}'s program could not be started"),
            StartKind::NoAnswer => format!("{who}'s program did not answer in time"),
            StartKind::Failed => format!("{who}'s program ended with an error"),
        };
        match self.code {
            Some(StartCode::Errno(name)) => format!("{said} ({name})"),
            Some(StartCode::Exit(status)) => format!("{said} (exit code {status})"),
            None => said,
        }
    }

    /// What to do about it, where there is something: no sentence of its own, to follow
    /// [`StartFailure::what`] after a semicolon.
    pub fn advice(&self, who: &str) -> Option<String> {
        Some(match self.kind {
            StartKind::NotFound => {
                format!("check that {who} is installed and that ConsensFlow can find it")
            }
            StartKind::NotAllowed => "check the program's permissions".to_owned(),
            StartKind::NoNode => "make the harness's Node visible to ConsensFlow, or install \
                                  the harness's own build instead of the npm one"
                .to_owned(),
            StartKind::NoNodeForPi => {
                "install Node 22.19 or newer, or run Pi's installer again".to_owned()
            }
            StartKind::NotAnNpmShim => {
                format!("install {who}'s own build, or its npm shim")
            }
            StartKind::NotStarted | StartKind::NoAnswer | StartKind::Failed => return None,
        })
    }

    /// [`StartFailure::what`], and after it the advice: `Pi's program was not found (ENOENT);
    /// check that Pi is installed and that ConsensFlow can find it`.
    pub fn words(&self, who: &str) -> String {
        match self.advice(who) {
            Some(advice) => format!("{}; {advice}", self.what(who)),
            None => self.what(who),
        }
    }
}

/// The exit status of a program of ConsensFlow's that stands in for a harness in a window
/// (`cf codex-session`) when it could not start the harness: one status for each kind, in a
/// range no harness uses (sysexits.h keeps 64 to 78), so that the engine reads from how the
/// window ended, and not from its screen, that the harness never ran.
const START_STATUS_BASE: i64 = 70;

impl StartKind {
    const ALL: [StartKind; 8] = [
        StartKind::NotFound,
        StartKind::NotAllowed,
        StartKind::NoNode,
        StartKind::NoNodeForPi,
        StartKind::NotAnNpmShim,
        StartKind::NotStarted,
        StartKind::NoAnswer,
        StartKind::Failed,
    ];

    /// The status a window's program ends with when this is why it could not start the harness.
    pub fn exit_status(self) -> i32 {
        let at = Self::ALL.iter().position(|kind| *kind == self).unwrap_or(0);
        i32::try_from(START_STATUS_BASE).unwrap_or(70) + i32::try_from(at).unwrap_or(0)
    }

    /// The kind a window's exit status says, where it is one of those [`StartKind::exit_status`]
    /// gives.
    pub fn of_exit_status(status: i64) -> Option<StartKind> {
        let at = usize::try_from(status.checked_sub(START_STATUS_BASE)?).ok()?;
        Self::ALL.get(at).copied()
    }
}

/// Why a program could not be run: the words as the person reads them where they are the
/// person's own (the harness's terminal, the page's fields), which name the file that was
/// tried, and [`StartFailure`], which is all a note says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartRefusal {
    words: String,
    failure: StartFailure,
}

impl StartRefusal {
    /// A refusal that `words` tell the person, and a note says as `failure`.
    pub fn new(failure: StartFailure, words: String) -> Self {
        Self { words, failure }
    }

    /// The words for the person's own screen.
    pub fn words(&self) -> &str {
        &self.words
    }

    /// What a note says of it.
    pub const fn failure(&self) -> StartFailure {
        self.failure
    }
}

impl fmt::Display for StartRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.words)
    }
}

impl std::error::Error for StartRefusal {}

impl From<StartRefusal> for String {
    fn from(refusal: StartRefusal) -> Self {
        refusal.words
    }
}

#[cfg(test)]
mod tests;
