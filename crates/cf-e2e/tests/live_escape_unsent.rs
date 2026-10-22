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
//! Isolation. Claude Code runs on a throwaway `CLAUDE_CONFIG_DIR` that holds a copy
//! of the owner's login from the Keychain (`security find-generic-password`), never
//! printed, refused where the token ends within three hours, compared with the copy
//! afterwards (a refresh is said, as a failure) and deleted on every way out; the
//! owner's `~/.claude/settings.json` and `~/.claude.json` must hash the same after
//! the run, and `~/.claude/history.jsonl` hold as many lines. Claude's windows are
//! shut out of MCP servers and the browser (`--strict-mcp-config --no-chrome`).
//! Codex runs on a throwaway `CODEX_HOME` holding a copy of `auth.json` only,
//! compared the same way, with every MCP server switched off.
//!
//! ```text
//! cargo xtask live escape-unsent                      every harness, Claude last
//! cargo xtask live escape-unsent devin codex          the ones named, in that order
//! ```
//!
//! It runs on macOS (and Linux, but for Claude Code): its copies of a login are
//! files of mode 600, and Claude's login is read from the Keychain.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Open, Window};
use cf_e2e::live::{self, agents, ask, codex, Harness, Installed, Log};
use cf_e2e::process::Run;
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

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

/// The lines of the screen as drawn the run reads: the whole of a 40-row window.
const BOTTOM: usize = 40;

/// The least time the Claude login copied must have before it ends: the run, and
/// the margin the owner's own Claude needs to have refreshed it first.
const LOGIN_LEFT: Duration = Duration::from_secs(3 * 60 * 60);

const ESCAPE: &[u8] = b"\x1b";
const ENTER: &[u8] = b"\r";
const CTRL_U: &[u8] = b"\x15";

/// A step of keys, and what it is called in the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Typed,
    Escape,
    EscapeTwice,
    CtrlU,
    Dialog,
    DialogEscape,
}

impl Step {
    const ALL: [Step; 6] = [
        Step::Typed,
        Step::Escape,
        Step::EscapeTwice,
        Step::CtrlU,
        Step::Dialog,
        Step::DialogEscape,
    ];

    const fn name(self) -> &'static str {
        match self {
            Step::Typed => "typed",
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
            Step::Typed | Step::Escape | Step::EscapeTwice | Step::CtrlU => WORDS,
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
    lines: Vec<String>,
}

impl Read {
    /// The step's outcome, as ids: `escape: words-shown held holds`.
    fn said(&self) -> String {
        format!(
            "{}: {} {} {}",
            self.step.name(),
            if self.shown {
                "words-shown"
            } else {
                "words-gone"
            },
            if self.held { "held" } else { "clear" },
            self.reader.as_deref().unwrap_or("unread"),
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
    fn rig(&self) -> &Rig {
        self.rig
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn named(&self) -> Value {
        json!({ "id": self.id(), "generation": 1 })
    }

    /// Types `keys` as the human does, one write.
    fn types(&self, keys: &[u8]) -> Result {
        self.rig().types(&self.named(), keys)
    }

    /// Types `text` one character at a time.
    fn types_text(&self, text: &str) -> Result {
        for character in text.chars() {
            self.types(character.to_string().as_bytes())?;
            sleep(KEY_GAP);
        }
        Ok(())
    }

    /// The pane host's snapshot, with the screen's last lines as drawn.
    fn snapshot(&self) -> Result<Value> {
        let mut request = self.named();
        request["bottom"] = json!(BOTTOM);
        self.rig().host("pane.snapshot", request)
    }

    /// Until the screen holds still for [`STILL`], or [`SETTLE`] passed.
    fn settle(&self) {
        let started = Instant::now();
        let (mut last, mut since) = (self.rig().screen(self.id()), Instant::now());
        while started.elapsed() < SETTLE {
            sleep(LOOK);
            let now = self.rig().screen(self.id());
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

/// What the adapter of `harness` reads of the input box in `lines`.
fn reader(harness: Harness, lines: &[String]) -> Result<Option<String>> {
    let read =
        ask::question(&json!({ "op": "input_box", "kind": harness.kind(), "screen": lines }))?;
    Ok(read.as_str().map(str::to_owned))
}

/// Reads what `step` left in `pane`, a window of `harness`.
fn read(pane: &Pane, harness: Harness, step: Step) -> Result<Read> {
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
        reader: reader(harness, &lines)?,
        lines,
    })
}

/// Presses `step`'s keys into `pane`, a window of `harness`, and reads what they
/// left.
fn step(pane: &Pane, harness: Harness, step: Step, before: Option<&Read>) -> Result<Read> {
    match step {
        Step::Typed => pane.types_text(WORDS)?,
        Step::Escape => pane.types(ESCAPE)?,
        Step::EscapeTwice => {
            pane.types(ESCAPE)?;
            sleep(DOUBLE_PRESS);
            pane.types(ESCAPE)?;
        }
        Step::CtrlU => pane.types(CTRL_U)?,
        Step::Dialog => {
            pane.types_text(picker(harness))?;
            sleep(KEYED);
            pane.types(ENTER)?;
            pane.settle();
            pane.types_text(FILTER)?;
        }
        Step::DialogEscape => {
            pane.types(ESCAPE)?;
            // A picker whose first Escape clears its filter takes a second to close.
            if read(pane, harness, step)?.shown && before.is_some_and(|read| read.shown) {
                pane.types(ESCAPE)?;
            }
        }
    }
    read(pane, harness, step)
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
    let (a, b) = (3172, 1046);
    let body = window.given(&format!("What is {a} + {b}? Answer with the number alone."))?;
    let sent = window.send_read_empty(&body, &(a + b).to_string(), left.typed)?;
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
                "the paste went in and nothing answered it in {} s",
                sent.seconds
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

/// A throwaway config folder holding a copy of a login: removed with the value,
/// however the run ends. `file` is the login's name in it.
struct Throwaway {
    folder: TempDir,
    file: &'static str,
    copy: Vec<u8>,
}

impl Throwaway {
    /// A folder holding `login` as `file`, readable by its owner alone.
    fn holding(file: &'static str, login: Vec<u8>) -> Result<Self> {
        let folder = TempDir::new()
            .map_err(|failed| Error::Daemon(format!("a throwaway folder: {failed}")))?;
        let path = folder.path().join(file);
        fs::write(&path, &login)
            .map_err(|failed| Error::Daemon(format!("the login's copy: {failed}")))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|failed| Error::Daemon(format!("the login's copy: {failed}")))?;
        Ok(Self {
            folder,
            file,
            copy: login,
        })
    }

    fn path(&self) -> &Path {
        self.folder.path()
    }

    /// Whether the login is as it was copied: a harness that refreshed it has left
    /// the owner's own copy behind.
    fn check(&self, who: &str) -> Check {
        let now = fs::read(self.folder.path().join(self.file)).unwrap_or_default();
        Check::new(
            who,
            "the login copied is as it was",
            now == self.copy,
            if now == self.copy {
                "unchanged".to_owned()
            } else {
                "REFRESHED during the run: the owner's own login may now be stale".to_owned()
            },
        )
    }
}

/// The owner's Claude login, from the Keychain, as a config folder's
/// `.credentials.json` takes it; refused where its token ends within [`LOGIN_LEFT`].
fn claude_login() -> Result<Vec<u8>> {
    let ran = Run::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .limit(Duration::from_secs(120))
        .run()?;
    if ran.code != Some(0) {
        return Err(Error::Daemon(format!(
            "the Keychain gave no Claude login (exit {:?})",
            ran.code
        )));
    }
    let login = ran.stdout.trim().as_bytes().to_vec();
    let read: Value = serde_json::from_slice(&login)
        .map_err(|_| Error::Daemon("the Claude login in the Keychain is no JSON".to_owned()))?;
    let ends = read["claudeAiOauth"]["expiresAt"]
        .as_i64()
        .unwrap_or_default();
    let left = ends - jiff::Timestamp::now().as_millisecond();
    let least = i64::try_from(LOGIN_LEFT.as_millis()).unwrap_or(i64::MAX);
    if left < least {
        return Err(Error::Daemon(format!(
            "the Claude login's token ends in {} min, under {} min: let the owner's Claude refresh it first",
            left / 60_000,
            least / 60_000
        )));
    }
    Ok(login)
}

/// What the owner's Claude files are: `settings.json` and `.claude.json` by their
/// hashes, `history.jsonl` by its lines.
fn owners_claude(home: &Path) -> Vec<(String, String)> {
    let hash = |file: PathBuf| {
        fs::read(&file).map_or_else(
            |_| "none".to_owned(),
            |bytes| format!("{:x}", Sha256::digest(bytes)),
        )
    };
    let lines = fs::read(home.join(".claude/history.jsonl")).map_or(0, |bytes| {
        bytes.iter().filter(|byte| **byte == b'\n').count()
    });
    vec![
        (
            "~/.claude/settings.json".to_owned(),
            hash(home.join(".claude/settings.json")),
        ),
        ("~/.claude.json".to_owned(), hash(home.join(".claude.json"))),
        (
            "~/.claude/history.jsonl lines".to_owned(),
            lines.to_string(),
        ),
    ]
}

/// Whether the owner's Claude files are as `before`.
fn owners_unchanged(home: &Path, before: &[(String, String)]) -> Check {
    let after = owners_claude(home);
    let changed: Vec<&str> = before
        .iter()
        .zip(&after)
        .filter(|(was, is)| was != is)
        .map(|(was, _)| was.0.as_str())
        .collect();
    Check::new(
        "claude",
        "the owner's Claude files",
        changed.is_empty(),
        if changed.is_empty() {
            "unchanged".to_owned()
        } else {
            format!("changed: {}", changed.join(", "))
        },
    )
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
        log.block(&format!("{name} {}", read.said()), &read.lines.join("\n"));
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
    let owners_before = claude.then(|| owners_claude(&home));
    let claude_config = claude
        .then(|| Throwaway::holding(".credentials.json", claude_login()?))
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
        checks.push(owners_unchanged(&home, before));
    }
    report::conclude(&log, "what each window came to", &checks)
}
