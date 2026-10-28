//! Live: what Escape leaves in a harness's input box, and whether keys the human
//! typed and then left with Escape still hold deliveries to the window. It is opt-in
//! (`cargo xtask live escape-unsent`): it opens a real window of each harness named,
//! and spends one cheap turn on Claude Code and on Devin, so it is ignored unless
//! asked for and is no part of `check` or of CI.
//!
//! The pane host counts the human's keys for what they leave unsent in a window's
//! input box (a character adds one, Backspace takes one off, Enter, Ctrl+C and
//! Ctrl+U clear it), and holds every paste into the window while the count is up:
//! Escape changes nothing. Keys typed into a harness's dialog or picker and left
//! there with Escape held every delivery to the window, its box empty (T-288, the
//! shape suspected of results queued for two days on the owner's other Mac). The
//! adapters that paste (Claude Code's and Devin's) now read the box on the screen
//! (`harness-ask input_box`), and a paste whose adapter read it empty, with no key
//! of the human's since, goes in over the count (`readEmpty`). Codex, Pi and
//! OpenCode take their messages through their own channels, which no count holds:
//! their windows are looked at for what Escape does, and nothing is sent.
//!
//! Each window, in a folder of its own, on its harness's cheap model, has keys typed
//! into it as the human types them at the app's page (`pane.typed`), one character
//! at a time, and after each step the run reads the pane host's count and the
//! screen's last lines as drawn, which the run's log shows and the report gives as
//! outcomes: whether the words typed are on the screen, whether the count holds, and
//! what the adapter reads of the box (`empty`, `holds`, or none).
//!
//! - `typed`         words at the prompt
//! - `escape`        Escape once
//! - `escape-twice`  Escape twice, 150 ms apart (the daemon's gap)
//! - `ctrl-u`        Ctrl+U, to leave the box as empty as keys can
//! - `dialog`        the harness's model picker (`/models` in OpenCode, `/model`
//!   elsewhere), opened with Enter, and a filter typed into it
//! - `dialog-escape` Escape out of it, and once more where the filter is still on
//!   the screen
//!
//! The run fails where the adapter reads a box empty while the words typed are on
//! the screen in it, which would paste a delivery into the human's text; and, for
//! Claude Code and Devin, where after `dialog-escape` the count holds and the box
//! does not read empty, or a paste told so is refused or never answered. No choice
//! is ever taken in a picker (no Enter there): `/model` with a choice saves the
//! owner's settings.
//!
//! Isolation. Claude Code runs on a throwaway `CLAUDE_CONFIG_DIR` (mode 700) that
//! holds a copy of the owner's login, the `claudeAiOauth` of the Keychain item of the
//! user's account on a Mac (`security find-generic-password -a "$USER"`; the item with no
//! account is a stale one) and of `~/.claude/.credentials.json` elsewhere, never printed, refused where its token ends within two
//! hours, and a `.claude.json` that says onboarding is done (else Claude opens on its
//! theme dialog). The copy is compared afterwards (a refresh is said, as a failure),
//! and the folder deleted on every way out. The owner's `~/.claude/settings.json` must
//! hash the same after the run, and `history.jsonl`, `projects/` and `.claude.json`'s
//! projects gain nothing that names the run's folder (`.claude.json` changes by
//! itself, and is not hashed). Claude's windows are shut out of MCP servers and the
//! browser (`--strict-mcp-config --no-chrome`).
//! Codex runs on a throwaway `CODEX_HOME` holding a copy of `auth.json` only,
//! compared the same way, with every MCP server switched off.
//!
//! ```text
//! cargo xtask live escape-unsent                      every harness, Claude last
//! cargo xtask live escape-unsent devin codex          the ones named, in that order
//! ```
//!
//! Every harness runs in it on Windows too (`windows-from-tree.sh … cargo xtask live
//! escape-unsent claude`): there Claude Code's login is read from
//! `%USERPROFILE%\.claude\.credentials.json`, and the throwaway folder is the user's own in
//! the temporary folder, as Windows gives it, with no mode of its own.

#[path = "live_isolation/claude.rs"]
mod isolation;

use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Open, Window};
use cf_e2e::live::{self, agents, ask, codex, Harness, Installed, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};
use isolation::{claude_login, owners_claude, owners_unchanged, Throwaway};
use regex::Regex;
use serde_json::{json, Value};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows:
/// no window of this test runs it, but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The folder of the run's windows, below the live folders.
const FOLDER: &str = "escape-unsent";

/// The harnesses, in the order a run with none named takes them: Claude Code last.
const ORDER: [Harness; 5] = [
    Harness::Devin,
    Harness::Codex,
    Harness::Pi,
    Harness::OpenCode,
    Harness::Claude,
];

/// What is typed at the prompt, and into the picker's filter: words no screen of
/// a harness shows by itself.
const WORDS: &str = "probe words";
const FILTER: &str = "zqv";

/// The gap between two characters typed, between a command's keys and its Enter,
/// and between the two presses of a double Escape.
const KEY_GAP: Duration = Duration::from_millis(40);
const KEYED: Duration = Duration::from_millis(600);
const DOUBLE_PRESS: Duration = Duration::from_millis(150);

/// How long a window has to draw its prompt, how long its screen must hold still
/// after keys to count as drawn, and the most that is waited for that.
const DRAWS: Duration = Duration::from_secs(120);
const STILL: Duration = Duration::from_millis(1500);
const SETTLE: Duration = Duration::from_secs(15);
const LOOK: Duration = Duration::from_millis(100);

/// How many of the last bytes a window printed the log shows after each step.
const RAW_TAIL: usize = 800;

/// The most a key is waited for to be drawn, as the run measures it.
const DRAW_WAIT: Duration = Duration::from_secs(5);

/// The lines of the screen as drawn the run reads: the whole of a 40-row window.
const BOTTOM: usize = 40;

/// The word the delivery asks for: one no screen shows by itself, with no digits a
/// model may write another way (`4,218`).
const ANSWERED: &str = "PICKERLEFTOK";

const ESCAPE: &[u8] = b"\x1b";
/// Home: the cursor to the start of what is typed, which stays.
const HOME: &[u8] = b"\x1b[H";
/// End: the cursor back to the end of what is typed.
const END: &[u8] = b"\x1b[F";
const ENTER: &[u8] = b"\r";
const CTRL_U: &[u8] = b"\x15";

/// A step of keys, and what it is called in the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Typed,
    Home,
    Escape,
    EscapeTwice,
    CtrlU,
    Dialog,
    DialogEscape,
}

impl Step {
    const ALL: [Step; 7] = [
        Step::Typed,
        Step::Home,
        Step::Escape,
        Step::EscapeTwice,
        Step::CtrlU,
        Step::Dialog,
        Step::DialogEscape,
    ];

    const fn name(self) -> &'static str {
        match self {
            Step::Typed => "typed",
            Step::Home => "home",
            Step::Escape => "escape",
            Step::EscapeTwice => "escape-twice",
            Step::CtrlU => "ctrl-u",
            Step::Dialog => "dialog",
            Step::DialogEscape => "dialog-escape",
        }
    }

    /// The words the step's keys leave where they may still be on the screen.
    const fn words(self) -> &'static str {
        match self {
            Step::Typed | Step::Home | Step::Escape | Step::EscapeTwice | Step::CtrlU => WORDS,
            Step::Dialog | Step::DialogEscape => FILTER,
        }
    }
}

/// The command that opens `harness`'s model picker.
const fn picker(harness: Harness) -> &'static str {
    match harness {
        Harness::OpenCode => "/models",
        Harness::Claude | Harness::Codex | Harness::Pi | Harness::Devin => "/model",
    }
}

/// Whether ConsensFlow pastes into `harness`'s window, so that the count holds its
/// deliveries: Claude Code and Devin. The others take theirs through their own
/// channels.
const fn pasted(harness: Harness) -> bool {
    matches!(harness, Harness::Claude | Harness::Devin)
}

/// What one step left.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Read {
    step: Step,
    /// Whether the words the step typed are on the screen.
    shown: bool,
    /// Whether the pane host's count holds a paste.
    held: bool,
    /// How many writes of the human's keys the pane host has taken.
    typed: u64,
    /// What the adapter reads of the input box: `empty`, `holds`, or none.
    reader: Option<String>,
    /// The longest a key of the step took to be drawn, in milliseconds: none where
    /// one was not drawn within [`DRAW_WAIT`].
    drew_ms: Option<u64>,
    lines: Vec<String>,
    /// Where the pane host says the cursor is ([`Pane::snapshot`]'s `cursor`).
    cursor: Value,
}

impl Read {
    /// The step's outcome, as ids: `escape: words-shown held holds`.
    fn said(&self) -> String {
        format!(
            "{}: {} {} {} {}",
            self.step.name(),
            if self.shown {
                "words-shown"
            } else {
                "words-gone"
            },
            if self.held { "held" } else { "clear" },
            self.reader.as_deref().unwrap_or("unread"),
            self.drew_ms
                .map_or_else(|| "undrawn".to_owned(), |ms| format!("drawn-in-{ms}ms")),
        )
    }
}

/// A window of the run: one the live kit opens as the app does (`window`), or
/// Codex's TUI opened bare (ConsensFlow opens Codex on its broker, which the kit
/// does not).
struct Pane<'r> {
    rig: &'r Rig,
    id: String,
    window: Option<Window<'r>>,
}

impl Pane<'_> {
    fn id(&self) -> &str {
        &self.id
    }

    fn named(&self) -> Value {
        json!({ "id": self.id(), "generation": 1 })
    }

    /// Types `keys` as the human does, one write.
    fn types(&self, keys: &[u8]) -> Result {
        self.rig.types(&self.named(), keys)
    }

    /// Types `text` one character at a time: the longest one took to be drawn.
    fn types_text(&self, text: &str) -> Result<Option<u64>> {
        let mut longest = Some(0);
        for character in text.chars() {
            self.types(character.to_string().as_bytes())?;
            longest = longest.zip(self.drawn()?).map(|(was, now)| was.max(now));
            sleep(KEY_GAP);
        }
        Ok(longest)
    }

    /// How long after now the window prints something, its last key typed before
    /// now: in milliseconds, none where it prints nothing within [`DRAW_WAIT`].
    fn drawn(&self) -> Result<Option<u64>> {
        let since = Instant::now();
        while since.elapsed() < DRAW_WAIT {
            let snapshot = self.snapshot()?;
            let printed = snapshot["outputQuietMs"].as_u64();
            let typed = snapshot["typedQuietMs"].as_u64();
            if printed
                .zip(typed)
                .is_some_and(|(printed, typed)| printed < typed)
            {
                return Ok(Some(
                    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX),
                ));
            }
            sleep(Duration::from_millis(10));
        }
        Ok(None)
    }

    /// The pane host's snapshot, with the screen's last lines as drawn.
    fn snapshot(&self) -> Result<Value> {
        let mut request = self.named();
        request["bottom"] = json!(BOTTOM);
        self.rig.host("pane.snapshot", request)
    }

    /// Until the screen holds still for [`STILL`], or [`SETTLE`] passed.
    fn settle(&self) {
        let started = Instant::now();
        let (mut last, mut since) = (self.rig.screen(self.id()), Instant::now());
        while started.elapsed() < SETTLE {
            sleep(LOOK);
            let now = self.rig.screen(self.id());
            if now == last {
                if since.elapsed() >= STILL {
                    return;
                }
            } else {
                (last, since) = (now, Instant::now());
            }
        }
    }

    fn kill(&self) {
        match &self.window {
            Some(window) => window.kill(),
            None => {
                let _ = self.rig.host("pane.kill", self.named());
            }
        }
    }
}

/// What the adapter of `harness` reads of the input box in `lines`, the cursor where
/// `cursor` says.
fn reader(harness: Harness, lines: &[String], cursor: &Value) -> Result<Option<String>> {
    let read = ask::question(
        &json!({ "op": "input_box", "kind": harness.kind(), "screen": lines, "cursor": cursor }),
    )?;
    Ok(read.as_str().map(str::to_owned))
}

/// Reads what `step` left in `pane`, a window of `harness`.
fn read(pane: &Pane, harness: Harness, step: Step, drew_ms: Option<u64>) -> Result<Read> {
    pane.settle();
    let snapshot = pane.snapshot()?;
    let lines: Vec<String> = snapshot["bottom"]
        .as_array()
        .map(|lines| lines.iter().map(live::text).collect())
        .unwrap_or_default();
    Ok(Read {
        step,
        shown: lines.iter().any(|line| line.contains(step.words())),
        held: snapshot["unsent"] == true,
        typed: snapshot["typed"].as_u64().unwrap_or_default(),
        reader: reader(harness, &lines, &snapshot["cursor"])?,
        drew_ms,
        lines,
        cursor: snapshot["cursor"].clone(),
    })
}

/// Presses `step`'s keys into `pane`, a window of `harness`, and reads what they
/// left.
fn step(pane: &Pane, harness: Harness, step: Step, before: Option<&Read>) -> Result<Read> {
    let drew = match step {
        Step::Typed => pane.types_text(WORDS)?,
        Step::Home => {
            pane.types(HOME)?;
            pane.drawn()?
        }
        Step::Escape => {
            pane.types(ESCAPE)?;
            pane.drawn()?
        }
        Step::EscapeTwice => {
            pane.types(ESCAPE)?;
            sleep(DOUBLE_PRESS);
            pane.types(ESCAPE)?;
            pane.drawn()?
        }
        // End first: Ctrl+U clears what is before the cursor, which Home moved.
        Step::CtrlU => {
            pane.types(END)?;
            pane.types(CTRL_U)?;
            pane.drawn()?
        }
        Step::Dialog => {
            let command = pane.types_text(picker(harness))?;
            sleep(KEYED);
            pane.types(ENTER)?;
            let opened = pane.drawn()?;
            pane.settle();
            let filter = pane.types_text(FILTER)?;
            command
                .zip(opened)
                .zip(filter)
                .map(|((command, opened), filter)| command.max(opened).max(filter))
        }
        Step::DialogEscape => {
            pane.types(ESCAPE)?;
            let mut drew = pane.drawn()?;
            // A picker whose first Escape clears its filter takes a second to close.
            if read(pane, harness, step, drew)?.shown && before.is_some_and(|read| read.shown) {
                pane.types(ESCAPE)?;
                drew = drew
                    .zip(pane.drawn()?)
                    .map(|(first, second)| first.max(second));
            }
            drew
        }
    };
    read(pane, harness, step, drew)
}

/// The run's checks of one window's steps `reads`.
fn checks_of(harness: Harness, reads: &[Read]) -> Vec<Check> {
    let name = harness.name();
    let said: Vec<String> = reads.iter().map(Read::said).collect();
    let mut checks = vec![Check::passed(name, "what each step left", said.join("; "))];
    let over_words: Vec<&str> = reads
        .iter()
        .filter(|read| read.shown && read.reader.as_deref() == Some("empty"))
        .map(|read| read.step.name())
        .collect();
    checks.push(Check::new(
        name,
        "the box never reads empty over words typed",
        over_words.is_empty(),
        if over_words.is_empty() {
            "it did not".to_owned()
        } else {
            format!(
                "it read empty with the words on the screen at {}",
                over_words.join(", ")
            )
        },
    ));
    checks
}

/// After the picker was left with Escape (`left`): the count holds, the box reads
/// empty, and a paste told so goes in and is answered.
fn delivered(window: &Window, left: &Read) -> Result<Check> {
    let name = window.harness().name();
    const WHAT: &str = "a delivery after the picker was left with Escape";
    if !left.held {
        return Ok(Check::failed(
            name,
            WHAT,
            "the count did not hold: the shape did not form",
        ));
    }
    if left.reader.as_deref() != Some("empty") {
        let read = left.reader.as_deref().unwrap_or("no box");
        return Ok(Check::failed(
            name,
            WHAT,
            format!("the box read {read}: the delivery is held"),
        ));
    }
    let body = window.given(&format!(
        "Reply with exactly the word {ANSWERED} and nothing else."
    ))?;
    let sent = window.send_read_empty(&body, ANSWERED, left.typed)?;
    Ok(if sent.written["ok"] != true {
        Check::failed(
            name,
            WHAT,
            format!("the paste was refused: {}", sent.written),
        )
    } else if !sent.shown {
        Check::failed(
            name,
            WHAT,
            format!(
                "the paste went in and nothing answered it in {} s; its record holds {} items, the last {:?}; its screen ends: {}",
                sent.seconds,
                window.recorded().len(),
                window.recorded().last().map(|item| (item.role.clone(), live::cut(&item.text, 80))),
                window.tail()
            ),
        )
    } else {
        Check::passed(
            name,
            WHAT,
            format!("pasted over the count and answered in {} s", sent.seconds),
        )
    })
}

/// Codex's TUI, opened bare in a folder of its own on its cheap model, its MCP
/// servers off, on the throwaway `codex_home`; its question about trusting the
/// folder answered with Enter (a yes kept in the throwaway folder).
fn open_codex<'r>(
    rig: &'r Rig,
    install: &Installed,
    codex_home: &Path,
    log: &Log,
) -> Result<Pane<'r>> {
    let home = PathBuf::from(rig.var("HOME").unwrap_or_default());
    let workspace = live::machine::live_folder_in(&home, &format!("{FOLDER}/codex"))?;
    let agent = agents::agent(Harness::Codex);
    let mut argv = vec![install.command().to_string_lossy().into_owned()];
    argv.extend(codex::isolation(&codex::mcp_servers(install.command())?)?);
    argv.extend(["-m".to_owned(), live::text(&agent["model"])]);
    if let Some(effort) = agent["effort"].as_str() {
        argv.extend([
            "-c".to_owned(),
            format!("model_reasoning_effort=\"{effort}\""),
        ]);
    }
    argv.push("--dangerously-bypass-approvals-and-sandbox".to_owned());
    let env = rig.environment().to_vec();
    let argv = ask::program(&argv, &env)?;
    let id = "escape-codex".to_owned();
    let opened = rig.host(
        "pane.open",
        json!({
            "id": id, "generation": 1, "cwd": workspace, "argv": argv,
            "env": { "CODEX_HOME": codex_home }, "dropEnv": [],
            "size": { "rows": 40, "cols": 120 },
        }),
    )?;
    if opened["ok"] != true {
        return Err(Error::Daemon(format!("codex did not open: {opened}")));
    }
    log.launched(&agent);
    let pane = Pane {
        rig,
        id,
        window: None,
    };
    pane.settle();
    let trust = Regex::new(r"(?i)trust this folder|trust and continue")
        .map_err(|failed| Error::Daemon(failed.to_string()))?;
    if trust.is_match(&rig.screen(pane.id())) {
        rig.host(
            "pane.input",
            json!({ "id": pane.id(), "generation": 1, "bytes": ENTER }),
        )?;
        sleep(Duration::from_secs(2));
        pane.settle();
    }
    Ok(pane)
}

/// The steps of one window of `install`'s, and what they came to.
fn probe(
    rig: &Rig,
    install: &Installed,
    codex_home: Option<&Path>,
    log: &Log,
) -> Result<Vec<Check>> {
    let harness = install.harness();
    let name = harness.name();
    let pane = match codex_home {
        Some(codex_home) => open_codex(rig, install, codex_home, log)?,
        None => {
            let window = Open::new(
                rig,
                install,
                &format!("{FOLDER}/{name}"),
                &format!("escape-{name}"),
            )
            .open_telling(log, name)?;
            if !window.still(DRAWS) {
                return Ok(vec![Check::failed(
                    name,
                    "opened",
                    format!("did not draw: {}", window.tail()),
                )]);
            }
            Pane {
                rig,
                id: window.id().to_owned(),
                window: Some(window),
            }
        }
    };
    let mut reads: Vec<Read> = Vec::new();
    for each in Step::ALL {
        let read = step(&pane, harness, each, reads.last())?;
        // The cursor, and the last bytes the window printed as they came: what tells
        // faint text from typed text, which the screen's text cannot (the log stays
        // on the machine).
        let printed = rig.printed(pane.id());
        let last = String::from_utf8_lossy(&printed[printed.len().saturating_sub(RAW_TAIL)..]);
        log.block(
            &format!("{name} {}", read.said()),
            &format!(
                "{}\ncursor: {}\nbytes: {last:?}",
                read.lines.join("\n"),
                read.cursor
            ),
        );
        reads.push(read);
    }
    let mut checks = checks_of(harness, &reads);
    if let (Some(window), true, Some(left)) = (&pane.window, pasted(harness), reads.last()) {
        checks.push(delivered(window, left)?);
    }
    pane.kill();
    Ok(checks)
}

#[test]
#[ignore = "opt-in: opens a real window of each harness named and spends a cheap turn on Claude Code and Devin; cargo xtask live escape-unsent"]
fn keys_left_in_a_dialog_with_escape_hold_no_delivery_to_a_window_whose_box_is_empty() -> Result {
    let log = Log::begin("live escape-unsent")?;
    ask::prepare()?;
    let named: Vec<Harness> = live::list("harness")
        .iter()
        .map(|word| {
            Harness::from_name(word)
                .ok_or_else(|| Error::Daemon(format!("no harness is called {word}")))
        })
        .collect::<Result<_>>()?;
    let harnesses = if named.is_empty() {
        ORDER.to_vec()
    } else {
        named
    };
    let installs: Vec<Installed> = harnesses
        .iter()
        .map(|harness| log.found(*harness))
        .collect::<Result<_>>()?;
    let home = live::machine::home()?;
    let claude = harnesses.contains(&Harness::Claude);
    let owners_before = claude.then(|| owners_claude(&home, FOLDER));
    let claude_config = claude
        .then(|| {
            Throwaway::holding(".credentials.json", claude_login()?)?
                .with(".claude.json", r#"{"hasCompletedOnboarding":true}"#)
        })
        .transpose()?;
    let codex_home = harnesses
        .contains(&Harness::Codex)
        .then(|| {
            let auth = fs::read(home.join(".codex/auth.json"))
                .map_err(|failed| Error::Daemon(format!("Codex's auth.json: {failed}")))?;
            Throwaway::holding("auth.json", auth)
        })
        .transpose()?;
    let config = match &claude_config {
        Some(folder) => live::rig_with_claude_config(&installs, FAKE_AGENT, folder.path()),
        None => live::rig(&installs, FAKE_AGENT),
    };
    let rig = Rig::start(config)?;
    let mut checks = Vec::new();
    for install in &installs {
        let codex = (install.harness() == Harness::Codex)
            .then(|| codex_home.as_ref().map(Throwaway::path))
            .flatten();
        match probe(&rig, install, codex, &log) {
            Ok(found) => checks.extend(found),
            Err(failed) => checks.push(Check::failed(
                install.harness().name(),
                "probed",
                failed.to_string(),
            )),
        }
    }
    rig.close()?;
    if let Some(folder) = &claude_config {
        checks.push(folder.check("claude"));
    }
    if let Some(folder) = &codex_home {
        checks.push(folder.check("codex"));
    }
    if let Some(before) = &owners_before {
        checks.push(owners_unchanged(&home, FOLDER, before));
    }
    report::conclude(&log, "what each window came to", &checks)
}
