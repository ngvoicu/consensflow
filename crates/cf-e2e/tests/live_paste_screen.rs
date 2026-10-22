//! Live: what a window's screen shows of a long message once it is sent. It is opt-in
//! (`cargo xtask live paste-screen`, `npm run live:paste-screen`): it spends the
//! machine's quota on one short task of each harness named, and needs each installed
//! and logged in, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! Why it is asked (T-298, Calliope's re-check T-294): a failed turn's note quotes
//! the window's last screen lines, less what ConsensFlow typed into it
//! (`crates/cf-engine/src/shown/ours.rs`). That rule finds a paste by its header, and
//! where the header scrolled off, by the newest paste's words at the top of what a
//! look reads; a harness that shows a sent message as a placeholder (`[Pasted text #1
//! +42 lines]`, `[Pasted Content 1200 chars]`) shows neither. Which harness shows
//! what, once the message is sent, is what this finds out.
//!
//! Each harness is the one worker of a project on the native daemon, whose chief is
//! the stand-in; the check, as the chief, gives it a brief of forty numbered lines
//! that ends asking for a sum, through the real path a message takes to it (pasted
//! into Claude Code and Devin, put to Codex, Pi and OpenCode through their own
//! queues). The worker's window is kept open as the human keeps one (`session.open`):
//! it closes with its task otherwise. Through the turn, twice a second, and once at
//! rest after it, the check asks the pane host for the window's screen as the
//! engine's look asks for it (`pane.snapshot`, its last twelve lines) and for a wider
//! view of eighty, and says of each what it showed: the message's header, a
//! placeholder, and how many of the brief's numbered lines. It says counts, never what
//! the screen said.
//!
//! It is a test as well: the screen at rest is held to the engine's own rule for what
//! of it ConsensFlow typed (`engine-ask`'s `ours`, which is `shown::ours`). Every line
//! that holds the brief must be ConsensFlow's, and the first line with a word after
//! the last of them the window's: a harness whose screen shows a sent message in a
//! shape the rule does not know fails the run.
//!
//! The windows run as the other live suites run them: on each harness's cheap model,
//! Claude Code through the kit's wrapper that shuts the human's MCP servers,
//! connectors and browser out (`--strict-mcp-config --no-chrome`). Live Claude runs
//! wait for a throwaway config seeded with the login (T-302, Gabriel's decision of
//! 2026-10-22): until the kit's rig has it, Claude is run only when asked. Known limit
//! (Calliope's re-check T-300, P3-R7): the Claude leg of T-298's runs left two lines
//! in the owner's Claude history, which T-302 keeps from happening again. Codex runs in a home of the
//! run's own that holds a copy of the machine's `auth.json` and nothing else, so it
//! has no MCP servers and never writes the machine's `~/.codex`; the run says whether
//! that copy changed (a token Codex refreshed in it), which would leave the machine's
//! own login stale, and the home goes with the run.
//!
//! ```text
//! cargo xtask live paste-screen                         all five, one after another
//! cargo xtask live paste-screen claude devin            those named
//! cargo xtask live paste-screen pi --keep               leaves the run's home and project
//! ```
//!
//! The run fails where a worker's task was not done, or no look found its window at
//! rest: what the screen showed is said either way.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::members::{self, find, is_over, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, text, Harness, Log, Who};
use cf_e2e::live::{agents, ask};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// How many numbered lines the brief has.
const LINES: usize = 40;

/// The sum the brief asks for, which it does not hold.
const SUM: (u32, u32) = (2357, 1113);

/// How many lines the engine's look asks the pane host for (`cf_proto::panes::TAIL_LINES`).
const LOOK_LINES: usize = 12;

/// The wider view of the screen.
const WIDE_LINES: usize = 80;

/// How long the window has to open, and the task to end.
const OPENS: Duration = Duration::from_secs(180);
const WORK: Duration = Duration::from_secs(300);

/// How often the screen is looked at through the turn.
const LOOK: Duration = Duration::from_millis(500);

/// How long the screen is left to settle after the turn before it is looked at.
const SETTLES: Duration = Duration::from_secs(3);

/// The mark of the brief's line `n`, which a screen that shows the line holds.
fn mark(n: usize) -> String {
    format!("PSL-{n:02}")
}

/// The brief: forty numbered lines, and the sum it asks for.
fn brief() -> String {
    let mut lines: Vec<String> = (1..=LINES)
        .map(|n| format!("{}: line {n} of the brief, nothing to do for it.", mark(n)))
        .collect();
    lines.push(format!(
        "Reply with only the sum of {} and {}, in digits, and run no tools.",
        SUM.0, SUM.1
    ));
    lines.join("\n")
}

/// What a screen's lines showed of the message.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Shows {
    /// A line that begins with a message's header, after the window's frame.
    header: bool,
    /// A line that holds a harness's placeholder for a paste.
    placeholder: bool,
    /// How many of the brief's numbered lines it held.
    lines: usize,
}

impl Shows {
    /// What `tail` shows.
    fn of(tail: &[String]) -> Self {
        let header = tail.iter().any(|line| {
            line.trim_start_matches(|c: char| !(c.is_alphanumeric() || c == '['))
                .starts_with("[ConsensFlow m-")
        });
        let placeholder = tail
            .iter()
            .any(|line| line.contains("[Pasted text #") || line.contains("[Pasted Content "));
        let lines = (1..=LINES)
            .filter(|n| tail.iter().any(|line| line.contains(&mark(*n))))
            .count();
        Self {
            header,
            placeholder,
            lines,
        }
    }

    /// The most of each of `self` and `other` that was shown.
    fn most(self, other: Self) -> Self {
        Self {
            header: self.header || other.header,
            placeholder: self.placeholder || other.placeholder,
            lines: self.lines.max(other.lines),
        }
    }

    /// As a run says it.
    fn said(self) -> String {
        let yes = |shown| if shown { "yes" } else { "no" };
        format!(
            "header {}, placeholder {}, brief lines {}/{LINES}",
            yes(self.header),
            yes(self.placeholder),
            self.lines
        )
    }
}

/// What the looks at a window found: through the turn, the most each view showed,
/// and at rest after it.
#[derive(Debug, Default)]
struct Seen {
    turn_look: Shows,
    turn_wide: Shows,
    rest: Option<(Shows, Shows)>,
    looks: usize,
    /// What of the screen at rest the engine's rule took otherwise than the brief
    /// was: none where it held.
    wrong: Option<String>,
}

impl Seen {
    fn line(&self) -> String {
        let rest = self.rest.map_or_else(
            || "never looked at at rest".to_owned(),
            |(look, wide)| format!("at rest: look {}; wide {}", look.said(), wide.said()),
        );
        format!(
            "{rest}; in the turn ({} looks): look {}; wide {}",
            self.looks,
            self.turn_look.said(),
            self.turn_wide.said()
        )
    }
}

/// The lines of the window `pane`'s screen the pane host gives for a tail of
/// `lines`: none where it does not answer.
fn tail(members: &Members, pane: &Value, lines: usize) -> Vec<String> {
    members
        .rig()
        .snapshot(pane, lines)
        .and_then(|answer| answer["tail"].as_array().cloned())
        .map(|shown| shown.iter().map(text).collect())
        .unwrap_or_default()
}

/// The harnesses the words of the command line name, or all five where they name none.
fn harnesses(named: &[String]) -> Result<Vec<Harness>> {
    if named.is_empty() {
        return Ok(Harness::ALL.to_vec());
    }
    named
        .iter()
        .map(|word| {
            Harness::from_name(word).ok_or_else(|| {
                Error::Daemon(format!(
                    "no harness is called {word}: {}",
                    Harness::ALL.map(Harness::name).join(", ")
                ))
            })
        })
        .collect()
}

/// A home of the run's own for Codex, holding a copy of the machine's `auth.json`
/// and nothing else, and where that copy came from.
struct CodexHome {
    home: tempfile::TempDir,
    copied: Vec<u8>,
}

impl CodexHome {
    fn make() -> Result<Self> {
        let machine = live::requested("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| live::requested("HOME").map(|home| Path::new(&home).join(".codex")))
            .ok_or_else(|| Error::Daemon("no Codex home to copy a login from".to_owned()))?;
        let auth = machine.join("auth.json");
        let copied = std::fs::read(&auth).map_err(|source| Error::File {
            action: "read",
            path: auth.clone(),
            source,
        })?;
        let home = tempfile::Builder::new()
            .prefix("cf-codex-home-")
            .tempdir()
            .map_err(|source| Error::File {
                action: "make a folder in",
                path: std::env::temp_dir(),
                source,
            })?;
        let into = home.path().join("auth.json");
        write_private(&into, &copied).map_err(|source| Error::File {
            action: "write",
            path: into,
            source,
        })?;
        Ok(Self { home, copied })
    }

    fn path(&self) -> String {
        self.home.path().to_string_lossy().into_owned()
    }

    /// Whether the run's copy of the login is no longer the copy made: Codex
    /// refreshed its token in the run's home.
    fn changed(&self) -> bool {
        std::fs::read(self.home.path().join("auth.json")).map_or(true, |now| now != self.copied)
    }
}

/// `bytes` written to `path` readable by its owner alone (mode 600 on Unix): a
/// copy of a login.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(bytes)
}

/// One worker's run: the brief given, its window kept open and looked at through
/// its turn and at rest after it.
fn run(harness: Harness, log: &Log) -> Result<Check> {
    let name = harness.name();
    let log = log.scoped(name);
    let codex = (harness == Harness::Codex)
        .then(CodexHome::make)
        .transpose()?;
    let mut plan = Plan::new(&format!("paste-screen-{name}"), members::stand_in_chief())
        .with(members::member(harness));
    if let Some(codex) = &codex {
        // Where the run's copy of the login is, should a killed run leave it behind.
        log.say(format!("Codex's home of the run's own: {}", codex.path()));
        plan = plan.with_var("CODEX_HOME", codex.path());
    }
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), &log)?;
    let seen = look(&mut members, harness, &log);
    let closed = members.close();
    let changed = codex.as_ref().is_some_and(CodexHome::changed);
    if changed {
        log.say("THE RUN'S COPY OF CODEX'S auth.json CHANGED: Codex refreshed its token in the run's home");
    }
    let (done, seen) = seen?;
    closed?;
    let mut detail = seen.line();
    if let Some(wrong) = &seen.wrong {
        detail.push_str(&format!("; {wrong}"));
    }
    if changed {
        detail.push_str("; Codex's auth.json copy changed in the run");
    }
    Ok(Check::new(
        name,
        "what the screen shows of a long message once sent",
        done && seen.rest.is_some() && seen.wrong.is_none(),
        detail,
    ))
}

/// The brief given to the worker of `harness`, and its window looked at: whether its
/// task was done, and what the looks found.
fn look(members: &mut Members, harness: Harness, log: &Log) -> Result<(bool, Seen)> {
    let agent = agents::id(harness);
    members.add(&agent, &[])?;
    let number = members.give(&agent, &brief())?;
    log.say(format!(
        "T-{number} is given: a brief of {LINES} numbered lines"
    ));
    let pane = find(log, "the worker's window to open", OPENS, LOOK, || {
        let lane = members.lane(&agent)?;
        Ok(lane["pane"].is_object().then_some(lane))
    })?
    .ok_or_else(|| Error::Daemon("the worker's window never opened".to_owned()))?;
    let handle = text(&pane["participant"]["handle"]);
    let kept = members.page("session.open", json!({ "handle": handle }))?;
    if kept["ok"] != true {
        return Err(Error::Daemon(format!(
            "the worker's window was not kept open: {kept}"
        )));
    }
    let session = text(&pane["participant"]["session"]);
    log.say(format!(
        "the worker's window ({handle}, session {session}) is kept open"
    ));
    let pane = pane["pane"].clone();
    let mut seen = Seen::default();
    let ended = find(log, "the worker's task to end", WORK, LOOK, || {
        seen.turn_look = seen
            .turn_look
            .most(Shows::of(&tail(members, &pane, LOOK_LINES)));
        seen.turn_wide = seen
            .turn_wide
            .most(Shows::of(&tail(members, &pane, WIDE_LINES)));
        seen.looks += 1;
        let thread = members.task(number)?;
        Ok(is_over(&thread).then(|| text(&thread["state"])))
    })?;
    std::thread::sleep(SETTLES);
    let at_rest = (
        Shows::of(&tail(members, &pane, LOOK_LINES)),
        Shows::of(&tail(members, &pane, WIDE_LINES)),
    );
    if members.rig().snapshot(&pane, 1).is_some() {
        seen.rest = Some(at_rest);
        let lines = tail(members, &pane, LOOK_LINES);
        let pasted: Vec<Value> = members.task(number)?["messages"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        seen.wrong = misread(&lines, &ask::ours(&lines, &pasted)?);
    }
    let state = ended.unwrap_or_else(|| "not over".to_owned());
    log.say(format!("T-{number} is {state}; {}", seen.line()));
    Ok((state == "done", seen))
}

/// Whether the brief's line `line` is: one of its numbered lines, or the sum it
/// asks for.
fn of_the_brief(line: &str) -> bool {
    line.contains("PSL-") || line.contains("Reply with only the sum of")
}

/// What the engine's rule (`ours`, a line to each of `lines`) took otherwise than
/// the screen was: a line of the brief taken for the window's, or the first line
/// with a word after the brief taken for ConsensFlow's. Said by line numbers, never
/// by the line. None where it held.
fn misread(lines: &[String], ours: &[bool]) -> Option<String> {
    let brief: Vec<usize> = (0..lines.len())
        .filter(|at| of_the_brief(&lines[*at]))
        .collect();
    if let Some(at) = brief
        .iter()
        .find(|at| !ours.get(**at).copied().unwrap_or(false))
    {
        return Some(format!(
            "line {} of the look holds the brief and was read as the window's",
            at + 1
        ));
    }
    let last = *brief.last()?;
    let next = (last + 1..lines.len()).find(|at| lines[*at].chars().any(char::is_alphanumeric))?;
    ours.get(next).copied().unwrap_or(false).then(|| {
        format!(
            "line {} of the look follows the brief and was read as ConsensFlow's",
            next + 1
        )
    })
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short task of each harness named; cargo xtask live paste-screen (npm run live:paste-screen)"]
fn what_each_harness_shows_of_a_long_message_once_it_is_sent() -> Result {
    let log = Log::begin("live paste-screen")?;
    let mut checks = Vec::new();
    for harness in harnesses(&live::list("harness"))? {
        let who = Who::Harness(harness);
        let before = who.version();
        let check = run(harness, &log).unwrap_or_else(|failed| {
            Check::failed(
                harness.name(),
                "what the screen shows of a long message once sent",
                failed.to_string(),
            )
        });
        log.say(format!(
            "{} {}: {}",
            harness.name(),
            live::between(before, who.version()),
            if check.ok { "ok" } else { "FAILED" }
        ));
        checks.push(check);
    }
    log.report("what each screen showed", &report::table(&checks));
    report::verdict(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| (*text).to_owned()).collect()
    }

    #[test]
    fn the_brief_has_forty_numbered_lines_and_asks_for_a_sum_it_does_not_hold() {
        let brief = brief();
        assert_eq!(brief.lines().count(), LINES + 1);
        assert!(brief.starts_with("PSL-01: line 1 of the brief"));
        assert!(brief.contains("PSL-40: line 40"));
        assert!(!brief.contains(&(SUM.0 + SUM.1).to_string()));
    }

    #[test]
    fn a_screen_says_its_header_its_placeholder_and_how_many_lines_of_the_brief_it_holds() {
        let shown = Shows::of(&lines(&[
            "> [ConsensFlow m-3 · T-1 · task from @chief]",
            "  PSL-01: line 1",
            "  PSL-02: line 2",
            "3470",
        ]));
        assert_eq!(
            shown,
            Shows {
                header: true,
                placeholder: false,
                lines: 2
            }
        );
        let folded = Shows::of(&lines(&["❭ [Pasted text #1 +41 lines]", "3470"]));
        assert_eq!(
            folded,
            Shows {
                header: false,
                placeholder: true,
                lines: 0
            }
        );
        // A line that only holds a header is no header line.
        assert!(!Shows::of(&lines(&["said: [ConsensFlow m-3 · note]"])).header);
        assert_eq!(
            folded.most(shown).said(),
            "header yes, placeholder yes, brief lines 2/40"
        );
    }

    #[test]
    fn a_screen_is_read_right_where_the_brief_is_consensflows_and_what_follows_the_windows() {
        let screen = lines(&[
            "PSL-39: line 39",
            "PSL-40: line 40",
            "Reply with only the sum of 2357 and 1113",
            "3470",
            "status bar",
        ]);
        assert_eq!(misread(&screen, &[true, true, true, false, false]), None);
        assert_eq!(
            misread(&screen, &[true, false, true, false, false]).as_deref(),
            Some("line 2 of the look holds the brief and was read as the window's")
        );
        assert_eq!(
            misread(&screen, &[true, true, true, true, false]).as_deref(),
            Some("line 4 of the look follows the brief and was read as ConsensFlow's")
        );
        // A screen that holds none of the brief has nothing to hold to the rule.
        assert_eq!(misread(&lines(&["3470"]), &[false]), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_of_a_login_is_readable_by_its_owner_alone() {
        use std::os::unix::fs::PermissionsExt;
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("auth.json");
        write_private(&file, b"{}").unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn with_no_harness_named_all_five_run_and_those_named_run_in_their_order() {
        assert_eq!(harnesses(&[]).unwrap(), Harness::ALL.to_vec());
        assert_eq!(
            harnesses(&["pi".to_owned(), "claude".to_owned()]).unwrap(),
            [Harness::Pi, Harness::Claude]
        );
        assert!(harnesses(&["nobody".to_owned()]).is_err());
    }
}
