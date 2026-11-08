//! What a window last showed, for the note that says why it did not come up,
//! and for the pause note of a task whose window closed under it. A window that
//! fails says why on its own screen (Pi without a login: "No API key found for
//! the selected model … Use /login"; a harness that crashes, its last words),
//! and the screen is gone with the window. The pane host keeps it, and how the
//! program ended, and tells them with `pane.exit` or to a `pane.snapshot` that
//! asks (`cf_proto::panes`); [`Shown`] is what the engine makes of that.
//!
//! The words are held to a few lines and a few hundred characters, and what
//! looks like a key or a token in them is hidden ([`mask`]): a screen can echo
//! one, and the note goes to the board, the chief's window and the log.
//!
//! **A screen that names a secret is not quoted at all.** A credential is no token
//! by its shape (`password: correct horse battery staple`, a value in quotes that
//! goes on over the next line, YAML's `'it''s'`), and no masker of the syntaxes it
//! is written in holds (both reviews of 2026-10-16 found one gap after another).
//! So where any of the lines the host sent of the screen's end names a secret-like
//! word ([`secret`]: a password, `pwd`, an API or private key or a credential anywhere
//! in a name, whatever its case or split, such as `PassWord` or `MYSQL_PWD`; a secret,
//! a token, `auth` or `bearer` as a word or a part of a name such as `DB_SECRET`, never
//! a piece of a word such as `author`), the note says that the lines were left out,
//! and quotes none of them; how the program ended is said all the same. Every line
//! the host sent is looked at, not only those the quote would keep: a value that
//! goes on over a line keeps no name on the line it ends on. The cost is the screens
//! that name one and hold none: Pi without a login says "No API key found for the
//! selected model", and its note says its lines were left out, with the exit code;
//! so does a screen that shows the shell's `pwd`.
//!
//! **No row beside a secret is quoted, and no guess is made of where a secret ends.** T-823's
//! first version left out the row that named a secret and its neighbours and quoted the rest,
//! and a review (T-1216, then T-1222) put passwords in the ledger with it twice over: a label
//! with its value below, a YAML block, a value hard-wrapped over rows, a sentence that trails
//! off, an assignment wrapped over rows of one word. Guessing the extent failed both times,
//! and the base's rule stands in alpha.2. Quoting a row next to a secret, where the extent is
//! *proven*, is later work (`issues/per-line-quoting-of-a-screen-next-to-a-secret.md` in the
//! brain). The rows are also looked at together, without the frame a program draws round them,
//! and as one text (the re-check T-1224 read passwords out of the ledger that a test of one row
//! at a time did not see): a name or a token's beginning the window's wrap cut in two is whole at
//! the join; the beginning of a token of a known kind (`sk-`, `ghp_`) withholds the screen
//! whatever follows it; so do a key with no name and no delimiter in view (rows of base64 and
//! nothing else, a lone padding row `=`) and a value in groups (`YWJj-ZGVm-…`). All in
//! [`screen`].
//!
//! Three more notes quote less by the same rule. **The notes of what a window left running**
//! (`background`) name the program of each command where the list of the last tool call names
//! it, and `…` where it goes on, or the process's number: never an argument, a path or a header
//! (T-1178's known issue). **The note of a turn a Resume tells a window was cut** quotes
//! nothing the window showed (T-1328): a turn that began with a delivery the ledger holds, sent
//! to that window, is named by the ledger's row of it (its id, what it was, whom it was from, its
//! task), and any other turn by when it began (`resume`). Two readings (T-1322) found a secret
//! in a quote of the opening by one shape after another. **A process that may be a window's** is
//! named by a program of that list or by its number. **The cause of a failed start** is no quote at all (T-1328): a note
//! carries the kind of failure and its code, typed by the producer (`cf_base::startup`, the
//! engine's `startup`), and no word of a program's, a host's or an adapter's; the full words stay
//! on the person's own screen. **A window's own session id** on its screen is not a value in groups
//! ([`Shown::with_own_session`]), and no other check is given a text without it.
//!
//! **How the program ended** is said as the host said it: its signal where it sent one, and a
//! status above 128 as consistent with the signal it names (143 SIGTERM, 137 SIGKILL,
//! [`signalled`]), no more; whether ConsensFlow sent it is said only from what the engine
//! recorded ([`Part`]). **What the window was doing** is the last tool call its record shows,
//! said before its screen: the tool, and for a shell call the first word of the command where
//! it is a program the list names, never more of the command, and never the file a tool worked
//! on ([`Shown::with_last_call`]).
//!
//! **A program the host could not end** (`survivor`) is said in place of how it
//! ended: it may still run ([`Shown::outlived`]). Nothing waits for it: a window
//! that opens on its conversation opens beside it, which the words warn of.

mod call;
pub(crate) mod mask;
mod ours;
mod quotable;
mod screen;
mod secret;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::sync::LazyLock;

use cf_base::startup::{StartFailure, StartKind};
use cf_harness::records::LastCall;
use cf_ledger::MAY_STILL_RUN;
use cf_proto::panes::{BackgroundProcess, PaneExit, PossiblyLeft, SnapshotTail};
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) use call::program as command_program;
use mask::mask;

#[cfg(any(test, feature = "test-support"))]
pub(crate) use ours::ours;
pub(crate) use ours::Paste;
use ours::{owners, Owner};
pub use quotable::{quotable, WORDS_WITHHELD};

/// The most lines of a screen a note quotes: the last ones.
const QUOTE_LINES: usize = 6;
/// The most characters a quote takes, between its quotation marks; the older
/// lines go first, and a last line longer than this is cut.
const QUOTE_CHARS: usize = 400;
/// What comes between the lines of a quote, which a note holds on one line.
const BETWEEN: &str = " / ";
/// The first exit code that is a Windows status (an `NTSTATUS` of a warning or
/// an error) rather than a number the program returned.
const STATUS_CODES: u32 = 0x8000_0000;
/// The status Windows ends a console program with on the human's Ctrl+C
/// (`STATUS_CONTROL_C_EXIT`): their interrupt, as a signal is on the Mac.
const CONSOLE_CTRL_C: u32 = 0xC000_013A;

/// What the pane host said of a window: how its program ended where it had,
/// the last lines its screen showed, and what it left running when the window
/// closed. A host that did not say is not a screen that showed nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shown {
    code: Option<u32>,
    signal: Option<String>,
    tail: Option<Vec<String>>,
    background: Vec<BackgroundProcess>,
    /// What the host named as possibly the window's, which it did not end.
    possibly_left: Vec<PossiblyLeft>,
    /// The window's program, where the host could not prove it gone (0 where its
    /// number was not known): it may still run.
    survivor: Option<u32>,
    /// Lines of the screen were ConsensFlow's own and are left out ([`Shown::without_pastes`]).
    pasted: bool,
    /// What ConsensFlow itself had sent the window's program before it ended ([`Part`]).
    part: Part,
    /// The last tool call the window's record shows ([`Shown::with_last_call`]).
    last_call: Option<Arc<LastCall>>,
    /// The session of the conversation the window ran in, as ConsensFlow has it
    /// ([`Shown::with_own_session`]).
    own_session: Option<String>,
    /// Why the window's harness never ran, where the window's program (`cf codex-session`)
    /// ended with the status that says so ([`Shown::with_start`]).
    start: Option<StartFailure>,
}

impl Shown {
    /// A pane's end, as the host sent it.
    pub fn from_exit(exit: PaneExit) -> Self {
        Self {
            code: exit.exit_code,
            signal: exit.signal,
            tail: exit.tail,
            background: exit.background,
            possibly_left: exit.possibly_left,
            survivor: exit.survivor,
            pasted: false,
            part: Part::Unknown,
            last_call: None,
            own_session: None,
            start: None,
        }
    }

    /// What a note says of a window's program the host could not end, after the word that
    /// its window closed: the program may still run, in the ledger's words of it, by which
    /// a pause note that goes leaves the warning behind ([`MAY_STILL_RUN`]). None where it
    /// has gone.
    pub(crate) fn outlived(&self) -> Option<String> {
        let program = match self.survivor? {
            0 => "its program".to_owned(),
            pid => format!("its program (process {pid})"),
        };
        Some(format!(", but {program} {MAY_STILL_RUN}"))
    }

    /// What the window left running when it closed, which its close ended: the
    /// processes its turns started, as the host listed them.
    pub(crate) fn left_running(&self) -> &[BackgroundProcess] {
        &self.background
    }

    /// What the host named, as the window closed, as possibly the window's and not ended:
    /// it runs on, and nothing proved it the window's (`PaneExit::possibly_left`).
    pub(crate) fn possibly_left(&self) -> &[PossiblyLeft] {
        &self.possibly_left
    }

    /// A pane's snapshot, asked for its tail: the lines it answered. An answer
    /// that was a refusal, or that gave no tail, says nothing.
    pub fn from_snapshot(answer: &Value) -> Self {
        if answer.get("ok") != Some(&Value::Bool(true)) {
            return Self::default();
        }
        let tail = serde_json::from_value::<SnapshotTail>(answer.clone())
            .ok()
            .and_then(|answer| answer.tail);
        Self {
            tail,
            ..Self::default()
        }
    }

    /// The window's screen without what ConsensFlow itself typed into it, of
    /// the `pastes` it typed ([`owners`]): those lines are no reason of the window's.
    /// Where the newest paste's run is in view, only the window's lines after it
    /// are kept: they are what the window printed since the message its failed
    /// turn was on.
    ///
    /// Known limits (Calliope's re-check T-305):
    /// - P3-3: a newest paste the window shows as a placeholder has no run, so the
    ///   whole tail is read, older errors and all (no harness showed one after
    ///   sending, T-298);
    /// - P3-5: a window line that begins with the newest paste's header (a model
    ///   echoing it at a line's start) becomes the cut, and hides what is above it;
    /// - P3-1, filed as its own task: a paste still on its way (the harness queued
    ///   it in the failing turn) is the newest and becomes the cut; the cut is to be
    ///   bounded by the newest paste the window was given;
    /// - P3-2, filed as its own task: the pastes are ordered by id, which is not the
    ///   order a member's window took them in (a task's brief waits while the window
    ///   holds a task, and a later note goes first).
    pub(crate) fn without_pastes(mut self, pastes: &[Paste]) -> Self {
        if let Some(tail) = self.tail.as_mut() {
            let owners = owners(tail, pastes);
            // What the window printed since the newest paste, where its run is in
            // view: older pastes and the failures told of before it are not this
            // turn's (Calliope's re-check T-300; the chief's decision, T-304).
            let since = owners.iter().rposition(|owner| *owner == Owner::Paste(0));
            let before = tail.len();
            let mut at = 0;
            tail.retain(|_| {
                let kept = owners[at] == Owner::Window && since.is_none_or(|end| at > end);
                at += 1;
                kept
            });
            self.pasted = tail.len() < before;
        }
        self
    }

    /// What the screen says of a failure, to tell one outage from the next
    /// ([`hashed`]): its last lines with a word in them, which are the screen's own and may
    /// name a secret, so only their hash goes on.
    pub(crate) fn outage_key(&self) -> String {
        hashed(&self.screen_folded())
    }

    /// The last lines of the screen with a word in them, folded ([`folded_key`]): readable,
    /// and for [`Shown::outage_key`] alone.
    fn screen_folded(&self) -> String {
        self.tail
            .as_deref()
            .map_or_else(String::new, |tail| folded_key(tail, Keep::Last))
    }

    /// The last lines of the screen the host sent: none where it sent none.
    pub(crate) fn lines(&self) -> &[String] {
        self.tail.as_deref().unwrap_or_default()
    }

    /// Whether the host said anything of the window.
    /// Whether the window's program is gone, as far as the host said: yes where
    /// it said how the window ended and named no program it could not end, no
    /// where it named one, none where it said nothing (the engine's own close
    /// settles its window before the host's word comes).
    pub(crate) fn gone(&self) -> Option<bool> {
        self.is_known().then_some(self.survivor.is_none())
    }

    /// Whether the host said the program ended with a code of failure: not 0, and no signal
    /// (a close's, or the human's Ctrl-C), nor the status Windows gives the human's Ctrl+C
    /// ([`CONSOLE_CTRL_C`], Calliope's P2c, T-994).
    pub(crate) fn ended_failing(&self) -> bool {
        self.signal.is_none()
            && self
                .code
                .is_some_and(|code| code != 0 && code != CONSOLE_CTRL_C)
    }

    pub fn is_known(&self) -> bool {
        self.code.is_some()
            || self.signal.is_some()
            || self.tail.is_some()
            || self.survivor.is_some()
    }

    /// `because`, and after it what the window showed and how it ended, as far
    /// as the host said: `the window closed (exit code 127); its screen ended
    /// with: "codex: command not found"`. A screen with nothing on it says that,
    /// and one that names a secret that its lines were left out. What was hidden
    /// in the quote is counted after it.
    pub fn after(&self, because: &str) -> String {
        self.after_with(because, "")
    }

    /// [`Shown::after`], with `besides` said between how the program ended and
    /// what its screen showed: `@zeus's window closed (exit code 3) with
    /// background work running, which its close ended: sleep 9; its screen ended with: "…"`.
    pub(crate) fn after_with(&self, because: &str, besides: &str) -> String {
        let mut said = because.to_owned();
        if let Some(outlived) = self.outlived() {
            said.push_str(&outlived);
        } else if let Some(ended) = self.ended() {
            said.push_str(&format!(" ({ended})"));
        }
        said.push_str(besides);
        if let Some(call) = &self.last_call {
            said.push_str(&call::words(call));
        }
        // A harness that never ran has no screen worth quoting: it holds the file that was tried.
        if let Some(failure) = self.start {
            said.push_str(&format!("; {}", failure.words("Codex")));
            return said;
        }
        let Some(tail) = &self.tail else {
            return said;
        };
        // The base's rule (the lead's decision of round 5), and the reviews of `860010398` and
        // `4aeb48564`: where the extent of a secret cannot be proven the whole screen goes, and
        // no row beside a secret is ever quoted. Every line the host sent is looked at, not only
        // those the quote would keep, and the rows together as well as one by one (`screen`).
        match screen::withheld(tail, self.own_session.as_deref()) {
            Some(screen::Withheld::Secret) => {
                said.push_str("; its last screen lines named a secret, so they are left out");
                return said;
            }
            Some(screen::Withheld::Key) => {
                said.push_str(
                    "; its last screen lines held what may be a key, so they are left out",
                );
                return said;
            }
            None => {}
        }
        match Quote::of(tail, Keep::Last) {
            None if self.pasted => said.push_str("; its screen showed nothing of its own"),
            None => said.push_str("; its screen was empty"),
            Some(quote) => said.push_str(&quote.after("; its screen ended with")),
        }
        said
    }

    /// This exit with what ConsensFlow had sent the window's program before it ended: a
    /// signal's source is told only by that, never by an exit code.
    pub(crate) fn with_part(mut self, part: Part) -> Self {
        self.part = part;
        self
    }

    /// This exit with the session of the conversation its window ran in, as ConsensFlow has it
    /// (the ledger's `native_session`): the id a harness prints when it quits
    /// (`claude --resume <id>`) is no secret, and the screen that shows it is quoted. Any other
    /// id on the screen still withholds it ([`screen`]).
    pub(crate) fn with_own_session(mut self, session: Option<String>) -> Self {
        self.own_session = session;
        self
    }

    /// This exit read for the harness the window ran: a Codex window is `cf codex-session`,
    /// ConsensFlow's own program, which ends with a status of its own when it could not start
    /// Codex (`StartKind::exit_status`). Then the window never reached the harness, and its
    /// note says the kind of failure from the status, not what its screen showed: the screen
    /// holds the file that was tried (T-1328; the second reading of T-1322 found
    /// `C:\tools\hunter22\codex.cmd` quoted from it at every width). A status in that range
    /// from any other harness is that harness's own.
    pub(crate) fn with_start(mut self, harness: Option<&str>) -> Self {
        if harness == Some("codex") && self.signal.is_none() {
            self.start = self
                .code
                .and_then(|code| StartKind::of_exit_status(i64::from(code)))
                .map(StartFailure::of);
        }
        self
    }

    /// This exit with the last tool call the window's record shows, as a line
    /// (`Record::last_call`): what the window was doing when it closed, which the note says
    /// before its screen, masked and cut short. None where the record shows none.
    pub(crate) fn with_last_call(mut self, call: Option<Arc<LastCall>>) -> Self {
        self.last_call = call;
        self
    }

    /// How the program ended, in words. A Windows program that crashed, or was
    /// ended by its console's Ctrl+C, ends with the status that says why
    /// (`0xC0000005`, an access violation; `0xC000013A`, the Ctrl+C), which is
    /// written as Windows writes it: in decimal it is ten digits nobody knows.
    ///
    /// A signal the host reported is named. A status above 128 is a shell's convention for a
    /// program a signal ended (143 is SIGTERM, 137 SIGKILL), and the note says it is
    /// consistent with it, no more (a program can exit with 143). What ConsensFlow had sent
    /// the program is said only where the engine knows it ([`Part`]; the project's own tests
    /// killed a Claude Code worker's `cargo test` and then the worker, and the note said
    /// "exit code 143", T-823).
    fn ended(&self) -> Option<String> {
        // A signal the host reported is a fact; a status above 128 is a shell's convention
        // for one (`/bin/sh -c 'exit 143'` ends normally and says it), so the note says only
        // that the status is consistent with the signal (the review of `a26e0848e`, P2).
        let (said, reported) = match (&self.signal, self.code) {
            (Some(signal), _) => (format!("ended by signal {signal}"), true),
            (None, Some(code)) if code >= STATUS_CODES => {
                return Some(format!("exit code {code:#010X}"));
            }
            (None, Some(code)) => match signalled(code, cfg!(unix)) {
                Some(name) => (format!("exit code {code}, consistent with {name}"), false),
                None => return Some(format!("exit code {code}")),
            },
            (None, None) => return None,
        };
        Some(match (self.part, reported) {
            (Part::NotSent, true) => {
                format!("{said}, which ConsensFlow did not send: it had sent no kill")
            }
            (Part::Sent, true) => format!("{said}, after ConsensFlow had sent it a kill"),
            (Part::NotSent, false) => format!("{said}; ConsensFlow had sent it no kill"),
            (Part::Sent, false) => {
                format!("{said}; ConsensFlow had sent it a kill before it closed")
            }
            (Part::Unknown, _) => said,
        })
    }
}

/// What ConsensFlow had itself sent a window's program when it ended: a kill, by pane and
/// generation, from the moment it asked. It says a signal was not its own
/// only when it knows (`Part::NotSent`): every window it opened is one it records the sends of.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Part {
    /// Not known: no claim is made.
    #[default]
    Unknown,
    /// It sent a kill.
    Sent,
    /// It sent nothing.
    NotSent,
}

/// `because`, and after it what a harness said of it in its own record (an
/// API's error, a provider's message): `@zeus's harness reported a failure; it
/// said: "Selected model is at capacity."`. Its words are held to a quote's
/// bounds, from their first line, which is an error's headline: they go to the
/// board, the chief's window and the log. What looks like a key, a token or an
/// email address is masked in them, and they are quoted whatever secret they
/// name: an API's or an SDK's error names the key it lacks ("No API key found")
/// and never holds one in prose, and the login class is the failure whose fix
/// the note must say (the chief's decision of 2026-10-21, T-135). A screen can
/// echo anything, and keeps the stricter rule ([`Shown::after_with`]). None where
/// no line of the words has a letter or a digit.
pub(crate) fn after_words(because: &str, words: &str) -> Option<String> {
    let lines: Vec<String> = words.lines().map(str::to_owned).collect();
    Quote::of(&lines, Keep::First).map(|quote| format!("{because}{}", quote.after("; it said")))
}

/// What a harness's `words` say of a failure, to tell one outage from the next
/// ([`hashed`]): their first lines with a word in them. A provider's error can name the key
/// it refused ("Incorrect API key provided: sk-…"), so only their hash goes on.
pub(crate) fn words_key(words: &str) -> String {
    hashed(&words_folded(words))
}

/// The first lines of a harness's `words` with a word in them, folded ([`folded_key`]):
/// readable, and for [`words_key`] alone.
fn words_folded(words: &str) -> String {
    let lines: Vec<String> = words.lines().map(str::to_owned).collect();
    folded_key(&lines, Keep::First)
}

/// What an outage's key is as it goes on, to the engine's memory, the ledger, its log and the
/// trace: the SHA-256 of what the failure said as [`folded_key`] folds it, in hex. The words
/// of a failure and the lines of a screen can hold a login or a token, and the note that
/// quotes them masks them (`mask`, `names_a_secret`); the key only has to be told equal or
/// not, and keeps nothing readable. A failure that said nothing the engine could tell
/// (no line with a word in it) has no key: the empty one, which is no cause and is never
/// hashed into one.
fn hashed(folded: &str) -> String {
    if folded.is_empty() {
        return String::new();
    }
    Sha256::digest(folded.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The lines a quote of `lines` would keep, their blanks made one and what
/// changes by itself within one outage folded ([`folded`]). Readable: it never leaves the
/// functions that hash it ([`hashed`]).
fn folded_key(lines: &[String], keep: Keep) -> String {
    let lines: Vec<String> = lines
        .iter()
        .filter(|line| line.chars().any(char::is_alphanumeric))
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    let chosen: Vec<&str> = match keep {
        Keep::Last => lines
            .iter()
            .rev()
            .take(QUOTE_LINES)
            .rev()
            .map(String::as_str)
            .collect(),
        Keep::First => lines.iter().take(QUOTE_LINES).map(String::as_str).collect(),
    };
    folded(&chosen.join("\n"))
}

/// What changes by itself from one turn of an outage to the next, each with
/// what it is folded to: a UUID, an id-like token (a run of 16 letters and
/// digits or more with no separator, a digit among them: a request's id, a
/// completion's), a clock time, and a duration (a number joined to a unit of
/// time: a retry-after, a countdown). Every other number is the failure's own:
/// `gpt-5` is not `gpt-6`, a 500 is not a 502, and a model's id, its parts
/// joined by `-` (`claude-sonnet-5-5`, a dated one, `gpt-5-codex-high`), or a
/// code's joined by `_` (`ERR_HTTP2_GOAWAY_SESSION`) is no id (the chief's
/// decisions of 2026-10-22, T-286 and T-298).
///
/// A footer's counters, which a harness redraws each turn, fold too: a share
/// (`12%` of a context left), an amount of money (`$0.03`), and a count written
/// short or with thousands (`1.2k`, `12,345` tokens), but no count that ends a
/// model's id (`gpt-4-32k`, `o3-mini-2k`) and no list of numbers (`500,502,503`):
/// a thousands count only before a count's unit, a short one only where no `-`
/// joins it to what comes before. Claude Code, Codex and Pi
/// each draw some of them below the conversation; the probe of T-298
/// recorded counts of the brief's lines only, so which line holds which is not
/// shown there.
///
/// Known limit (Calliope's re-check T-300, P3-R6): a UUID joined to a word by
/// `_` (`session_123e4567-…`) is not folded, as no word boundary holds between
/// `_` and its first digit.
static CHANGING: LazyLock<[(Regex, &str); 7]> = LazyLock::new(|| {
    [
        (
            pattern(
                r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
            ),
            "#id",
        ),
        (pattern(r"[A-Za-z0-9]{16,}"), "#id"),
        (pattern(r"\$\d+(?:\.\d+)?"), "#money"),
        (pattern(r"\b\d+(?:\.\d+)?%"), "#share"),
        (
            pattern(
                r"\b(\d{1,3}(?:,\d{3})+)\s+(?:tokens?|chars?|characters|words?|lines?|bytes?)\b|(?:^|[^\w-])(\d+(?:\.\d+)?[kKM])\b",
            ),
            "#count",
        ),
        (pattern(r"\b\d{1,2}:\d{2}(?::\d{2})?\b"), "#time"),
        (
            pattern(
                r"(?i)\b\d+(?:\.\d+)?\s?(?:ms|milliseconds?|s|secs?|seconds?|m|mins?|minutes?|h|hrs?|hours?)\b",
            ),
            "#duration",
        ),
    ]
});

/// The patterns are constants of this file, and a test folds with each.
#[allow(clippy::expect_used)]
fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("a pattern of the outage key")
}

/// `text` with what changes by itself within one outage folded.
fn folded(text: &str) -> String {
    let mut text = text.to_owned();
    for (at, (changing, to)) in CHANGING.iter().enumerate() {
        text = changing
            .replace_all(&text, |found: &regex::Captures<'_>| {
                let whole = found.get(0).map_or("", |whole| whole.as_str());
                // An id is one only with a digit in it: a long word is a word.
                if at == 1 && !whole.chars().any(|c| c.is_ascii_digit()) {
                    return whole.to_owned();
                }
                // A pattern that names what changes in a group of its own folds that
                // alone, and keeps what it was told apart by.
                let start = found.get(0).map_or(0, |whole| whole.start());
                match (1..found.len()).find_map(|group| found.get(group)) {
                    Some(part) => format!(
                        "{}{to}{}",
                        &whole[..part.start() - start],
                        &whole[part.end() - start..]
                    ),
                    None => (*to).to_owned(),
                }
            })
            .into_owned();
    }
    text
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        "string"
    } else {
        "strings"
    }
}

/// The words of a screen's last lines, or a harness's first, held to the
/// quote's bounds.
struct Quote {
    text: String,
    /// How many secrets were hidden in the lines that were kept.
    masked: usize,
}

impl Quote {
    /// The quote after `label`, and how many strings it hid.
    fn after(&self, label: &str) -> String {
        let mut said = format!("{label}: \"{}\"", self.text);
        if self.masked > 0 {
            let (count, strings) = (self.masked, plural(self.masked));
            said.push_str(&format!(" ({count} key- or token-like {strings} masked)"));
        }
        said
    }

    /// The quote of `lines`: none where no line has a word in it. A line of no
    /// letter or digit is a screen's chrome (a box's edge, a footer's rule) and
    /// is no line of it. The lines `keep` says come first, the last of a screen
    /// or the first of a harness's words: each has its blanks made one, its
    /// secrets hidden and its quotation marks made apostrophes (a quote is not to
    /// end where the text says it does), and they are kept while they fit.
    fn of(lines: &[String], keep: Keep) -> Option<Self> {
        let lines: Vec<String> = lines
            .iter()
            .filter(|line| line.chars().any(char::is_alphanumeric))
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect();
        let chosen: Vec<&String> = match keep {
            Keep::Last => lines.iter().rev().take(QUOTE_LINES).collect(),
            Keep::First => lines.iter().take(QUOTE_LINES).collect(),
        };
        let mut kept: Vec<String> = Vec::new();
        let (mut masked, mut used) = (0, 0);
        for line in chosen {
            let hidden = mask(line);
            let text = hidden.text.replace('"', "'");
            let between = if kept.is_empty() { 0 } else { BETWEEN.len() };
            let length = text.chars().count();
            if used + between + length > QUOTE_CHARS {
                // Only the line kept first may be cut; one after it that does not fit is left out.
                if kept.is_empty() {
                    kept.push(cut(&text, QUOTE_CHARS));
                    masked += hidden.count;
                }
                break;
            }
            used += between + length;
            masked += hidden.count;
            kept.push(text);
        }
        if keep == Keep::Last {
            kept.reverse();
        }
        (!kept.is_empty()).then(|| Self {
            text: kept.join(BETWEEN),
            masked,
        })
    }
}

/// Which lines a quote keeps when not all fit: a screen's last, where it said
/// what it said last; a harness's first, where an error says what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keep {
    First,
    Last,
}

/// The first characters of `text` that, with the mark that says it was cut,
/// make `limit`.
fn cut(text: &str, limit: usize) -> String {
    let kept: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// `text`, cut to `limit` characters with the mark that says so where it is longer.
fn cut_to(text: &str, limit: usize) -> String {
    if text.chars().count() > limit {
        cut(text, limit)
    } else {
        text.to_owned()
    }
}

/// The signal a status above 128 says ended the program (a shell's `128 + signal`): its name
/// where every system the app runs on agrees on it, its number otherwise. None for a status
/// that is no signal's, and for every status on Windows, which has no signals: there it is
/// the number the program returned.
fn signalled(code: u32, unix: bool) -> Option<String> {
    if !unix || !(129..=159).contains(&code) {
        return None;
    }
    let number = code - 128;
    let name = match number {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => return Some(format!("signal {number}")),
    };
    Some(name.to_owned())
}
