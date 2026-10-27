//! Codex's and Devin's parts of `live parity` (T-548): a member has what the same harness
//! has in a terminal in the project's folder.
//!
//! - **Codex's own setup.** Codex runs a member as an app-server and a remote TUI under
//!   ConsensFlow's supervisor, with a broker between them, where a terminal runs its local
//!   TUI: what the human's `CODEX_HOME` gives it must reach the member all the same. The
//!   run's Codex home is its own, a copy of the machine's login and nothing else of the
//!   owner's ([`CodexHome`], which says if Codex refreshed the login), holding an MCP server
//!   of the run's (one tool, which answers a word), a skill and an `AGENTS.md`; the
//!   project's `AGENTS.md` says a word of its own. The worker calls the tool, names its
//!   skills and says the words its instructions give it: its answer must hold the tool's
//!   word, the skill and both words. (Probed by hand on Codex 0.164.0 against a terminal:
//!   the same MCP servers, skills, hooks, plugins, slash commands but `/worktree`, which
//!   Codex's own `--remote` lacks too.)
//! - **Devin's own setup.** Devin runs a member on a copy of the human's `config.json` made
//!   for the launch (`--config`), with ConsensFlow's hooks added: what Devin finds beside
//!   the config (its skills and MCP servers) must still be found. On macOS and Linux the
//!   run's config folder is its own (`XDG_CONFIG_HOME`), holding a copy of the machine's
//!   `config.json` (readable by its owner alone), an MCP server of the run's and a skill;
//!   on Windows, where Devin keeps its config with its login and sessions, the server is
//!   the project's (`.devin/mcp_config.json`). The project has a skill of its own and its
//!   `AGENTS.md` a word. The worker calls the tool, names its skills and says the word.
//! - **What the human types, and the interrupt after it.** While the worker's next task
//!   waits in a long command, marks no key of a layout types (— → € «) and a letter are
//!   typed into its box as the page writes them (as they were typed, `terminals.js`), and
//!   must all show there, as in a terminal; the box is cleared, and then the harness's
//!   interrupt is pressed as the engine presses it: the turn must stop well before the
//!   command would have ended. On Windows the page spelled such marks in ASCII until T-548,
//!   and for a round wrote them as win32-input-mode keys, after one of which the console
//!   host held every lone Escape and no interrupt reached the window (Calliope, T-607).
//!
//! The run's MCP server is a Node script: zeewin has no Python.

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::ask;
use cf_e2e::live::codex::CodexHome;
use cf_e2e::live::members::{self, find, is_over, last_result, LongCommand, Members, Plan};
use cf_e2e::live::{self, installed, last_lines, text, Harness, Log};
use cf_e2e::process::own_var;
use cf_e2e::{files, Error, Result};
use tempfile::TempDir;

use super::{failure, FAKE_AGENT, LOOK, WORK};

/// What the chief asks of the worker: the run's MCP tool called, its skills named, and the
/// words its instructions give.
const SETUP_TASK: &str = "Do these three things and nothing else. 1. Call your MCP tool \
cf_parity_probe_tool once and write the word it returns. 2. List the names of every skill \
you can load, without loading any. 3. Write every parity word your instructions give you. \
One item per line.";

/// The run's MCP server: a stdio server with one tool, which answers [`TOOLS_WORD`].
const SERVER: &str = r#"const readline = require("readline");
const lines = readline.createInterface({ input: process.stdin });
lines.on("line", (line) => {
  let request;
  try {
    request = JSON.parse(line);
  } catch {
    return;
  }
  if (request === null || request.id === undefined || request.id === null) return;
  let result = {};
  if (request.method === "initialize") {
    const asked = (request.params || {}).protocolVersion || "2024-11-05";
    result = { protocolVersion: asked, capabilities: { tools: {} }, serverInfo: { name: "cf-parity", version: "1" } };
  } else if (request.method === "tools/list") {
    result = { tools: [{ name: "cf_parity_probe_tool", description: "ConsensFlow's live parity check: returns a word.", inputSchema: { type: "object", properties: {} } }] };
  } else if (request.method === "tools/call") {
    result = { content: [{ type: "text", text: "CF-PARITY-TOOL-RAN" }] };
  }
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: request.id, result }) + "\n");
});
"#;

/// What the run's tool answers.
const TOOLS_WORD: &str = "CF-PARITY-TOOL-RAN";

/// The skill of the human's own configuration (Codex's home, Devin's config folder).
const GLOBAL_SKILL: &str = "cf-parity-global";

/// The skill of the project's own folder (Devin's `.devin/skills`).
const PROJECT_SKILL: &str = "cf-parity-project";

/// The word of the human's own `AGENTS.md` (Codex's home), and of the project's.
const GLOBAL_WORD: &str = "CF-PARITY-GLOBAL";
const PROJECT_WORD: &str = "CF-PARITY-PROJECT";

/// What the project's folder tells every window (`AGENTS.md`, `CLAUDE.md`).
const PARITY_BRIEF: &str = "This folder is an automated ConsensFlow test. Do exactly what each \
message asks and nothing more. When a ConsensFlow message tells you to run a command or to use \
one of your tools, do it. The project's parity word is CF-PARITY-PROJECT.\n";

/// What the human types into the box of the worker's window, and the marks of it that must
/// show there: no key of a layout types the first four, and the last is a letter.
const TYPED: &str = "QQ \u{2014} \u{2192} \u{20ac} \u{ab} \u{e9} ZZ";
const MARKS: [&str; 5] = ["\u{2014}", "\u{2192}", "\u{20ac}", "\u{ab}", "\u{e9}"];

/// How long the typed marks have to show.
const SHOWN: Duration = Duration::from_secs(15);

/// The command the worker waits in while the human types: two minutes, by a mark of its own.
const WAITING: LongCommand = LongCommand::lasting("cf-parity-waits", 120);

/// How long the turn has to stop once the interrupt is pressed: well before the command
/// would have ended by itself.
const STOPS_WITHIN: Duration = Duration::from_secs(40);

/// The gap between two Escapes of an interrupt, the daemon's own.
const DOUBLE_PRESS: Duration = Duration::from_millis(150);

/// What the worker of a check is asked, what its answer must hold (each with where it
/// comes from), and the folders of the run's own that must last as long as it.
struct Case {
    plan: Plan,
    words: Vec<(&'static str, &'static str)>,
    codex_home: Option<CodexHome>,
    _folders: Vec<TempDir>,
}

/// Codex's and Devin's checks, for each of `harnesses` that is one of them, one after the
/// other.
pub fn run(log: &Log, harnesses: &[Harness]) -> Result {
    for harness in harnesses
        .iter()
        .copied()
        .filter(|harness| matches!(harness, Harness::Codex | Harness::Devin))
    {
        check(harness, &log.scoped(harness.name()))?;
    }
    Ok(())
}

/// A skill named `name`, as `SKILL.md` holds it.
fn skill(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: A skill for ConsensFlow's live parity check.\n---\n\
         Say that the parity check skill {name} was loaded.\n"
    )
}

/// A folder of the run's own whose name begins `prefix`.
fn folder(prefix: &str) -> Result<TempDir> {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .map_err(|source| Error::File {
            action: "make a folder in",
            path: std::env::temp_dir(),
            source,
        })
}

/// A folder of the run's own holding the MCP server's program, and the program.
fn server_folder() -> Result<(TempDir, PathBuf)> {
    let folder = folder("cf-parity-server-")?;
    let program = folder.path().join("server.cjs");
    files::write(&program, SERVER)?;
    Ok((folder, program))
}

/// `text` as a TOML or JSON string, quotes included (a Windows path's backslashes
/// escaped).
fn quoted(text: &Path) -> String {
    serde_json::Value::from(text.to_string_lossy().into_owned()).to_string()
}

/// `file`, made with the folders it is in, holding `text`, readable by its owner alone: a
/// copy of a config that may hold a token.
fn write_private(file: &Path, text: &str) -> Result {
    let failed = |action: &'static str| {
        move |source| Error::File {
            action,
            path: file.to_path_buf(),
            source,
        }
    };
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(failed("make the folder of"))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut made = options.open(file).map_err(failed("make"))?;
    std::io::Write::write_all(&mut made, text.as_bytes()).map_err(failed("write"))
}

/// Codex's check: a Codex home of the run's own, holding a copy of the machine's login,
/// the run's MCP server, a skill and an `AGENTS.md`.
fn codex_case(log: &Log) -> Result<Case> {
    let home = CodexHome::copying_login_of(&installed(Harness::Codex)?)?;
    let (server, program) = server_folder()?;
    home.write(
        "config.toml",
        &format!(
            "[mcp_servers.cf-parity]\ncommand = \"node\"\nargs = [{}]\n",
            quoted(&program)
        ),
    )?;
    home.write(
        &format!("skills/{GLOBAL_SKILL}/SKILL.md"),
        &skill(GLOBAL_SKILL),
    )?;
    home.write(
        "AGENTS.md",
        &format!("The human's global parity word is {GLOBAL_WORD}.\n"),
    )?;
    log.say(format!(
        "Codex runs on a home of the run's own, {}, holding a copy of the machine's login",
        home.path().display()
    ));
    let plan = Plan::new("parity-codex", members::stand_in_chief())
        .told(PARITY_BRIEF)
        .with(members::member(Harness::Codex))
        .with_var("CODEX_HOME", home.path().to_string_lossy());
    Ok(Case {
        plan,
        words: vec![
            (TOOLS_WORD, "the MCP server of its home"),
            (GLOBAL_SKILL, "the skill of its home"),
            (GLOBAL_WORD, "the AGENTS.md of its home"),
            (PROJECT_WORD, "the project's AGENTS.md"),
        ],
        codex_home: Some(home),
        _folders: vec![server],
    })
}

/// Devin's check: on macOS and Linux a config folder of the run's own (a copy of the
/// machine's `config.json`, the run's MCP server and a skill); on Windows the server is the
/// project's. The project has a skill of its own.
fn devin_case() -> Result<Case> {
    let (server, program) = server_folder()?;
    let servers = format!(
        "{{\"mcpServers\":{{\"cf-parity\":{{\"command\":\"node\",\"args\":[{}]}}}}}}",
        quoted(&program)
    );
    let mut plan = Plan::new("parity-devin", members::stand_in_chief())
        .told(PARITY_BRIEF)
        .with(members::member(Harness::Devin))
        .with_file(
            &format!(".devin/skills/{PROJECT_SKILL}/SKILL.md"),
            skill(PROJECT_SKILL),
        );
    let mut words = vec![
        (TOOLS_WORD, "the run's MCP server"),
        (PROJECT_SKILL, "the project's skill"),
        (PROJECT_WORD, "the project's AGENTS.md"),
    ];
    let mut folders = vec![server];
    if cfg!(windows) {
        plan = plan.with_file(".devin/mcp_config.json", servers);
    } else {
        let config = folder("cf-parity-devin-config-")?;
        let devin = config.path().join("devin");
        // The machine's own settings (its organisation among them), copied, never written.
        let machine = machine_config_folder()?.join("devin").join("config.json");
        let settings = std::fs::read_to_string(&machine).unwrap_or_else(|_| "{}".to_owned());
        write_private(&devin.join("config.json"), &settings)?;
        files::write(&devin.join("mcp_config.json"), &servers)?;
        files::write(
            &devin.join("skills").join(GLOBAL_SKILL).join("SKILL.md"),
            skill(GLOBAL_SKILL),
        )?;
        plan = plan.with_var("XDG_CONFIG_HOME", config.path().to_string_lossy());
        words.push((GLOBAL_SKILL, "the skill of its config folder"));
        folders.push(config);
    }
    Ok(Case {
        plan,
        words,
        codex_home: None,
        _folders: folders,
    })
}

/// The machine's config folder: `XDG_CONFIG_HOME`, else `.config` in the home.
fn machine_config_folder() -> Result<PathBuf> {
    if let Some(folder) = own_var("XDG_CONFIG_HOME").filter(|folder| !folder.is_empty()) {
        return Ok(PathBuf::from(folder));
    }
    own_var("HOME")
        .map(|home| PathBuf::from(home).join(".config"))
        .ok_or_else(|| Error::Daemon("this run has no home to find Devin's config in".to_owned()))
}

/// The worker's answer to [`SETUP_TASK`], once its task is over.
fn answer(members: &mut Members, harness: Harness, log: &Log) -> Result<String> {
    let agent = live::agents::id(harness);
    members.add(&agent, &[])?;
    let number = members.give(&agent, SETUP_TASK)?;
    log.say(format!("T-{number} is given"));
    let thread = find(log, "the worker's task to end", WORK, LOOK, || {
        let thread = members.task(number)?;
        Ok(is_over(&thread).then_some(thread))
    })?
    .ok_or_else(|| {
        Error::Daemon(format!(
            "T-{number} did not end within {} s",
            WORK.as_secs()
        ))
    })?;
    if text(&thread["state"]) != "done" {
        return Err(Error::Daemon(format!(
            "T-{number} is {}",
            text(&thread["state"])
        )));
    }
    last_result(&thread)
        .map(|result| text(&result["body"]))
        .ok_or_else(|| Error::Daemon(format!("T-{number} is done with no result")))
}

/// [`TYPED`] typed into the box of the `harness` worker's window while its task waits in
/// [`WAITING`], as the page writes it: every mark of it must show on the window's screen.
/// The box is cleared, the harness's interrupt is pressed as the engine presses it, and the
/// turn must stop within [`STOPS_WITHIN`]: the task ends, the command is gone from the
/// window, its lane, which read working, reads idle, or Devin's wire says the turn was
/// cancelled.
fn typed_then_interrupted(members: &Members, harness: Harness, log: &Log) -> Result {
    let agent = live::agents::id(harness);
    let brief = format!(
        "Run this shell command once and wait for it to end: {}. Then reply with the one \
         word WAITED.",
        WAITING.text()
    );
    let number = members.give(&agent, &brief)?;
    log.say(format!(
        "T-{number} is given: wait in a command while the human types"
    ));
    let (window, process) = find(
        log,
        "the command in the worker's window",
        WORK,
        LOOK,
        || {
            let Some(window) = members.window_of(&agent)? else {
                return Ok(None);
            };
            let Some(process) = members.window_process(&window) else {
                return Ok(None);
            };
            let running = !WAITING
                .in_window(&members::ThisMachine, process)?
                .is_empty();
            Ok(running.then_some((window, process)))
        },
    )?
    .ok_or_else(|| Error::Daemon(format!("T-{number}'s window ran no {}", WAITING.text())))?;
    let typed = typed_into(members, &window, log);
    members.press(&window.pane, &[0x15])?;
    // A turn that is over already, or whose command has gone, tells nothing of the
    // interrupt. Codex's lane reads working while the command runs; Devin's may read idle
    // (its wire is quiet), so its lane says nothing of the turn there.
    let running = || -> Result<bool> {
        Ok(!WAITING
            .in_window(&members::ThisMachine, process)?
            .is_empty())
    };
    let idle = || -> Result<bool> { Ok(members.lane(&agent)?["activity"]["state"] == "idle") };
    let busy_before = !idle()?;
    // Devin says a stopped turn on its wire: `cause:cancelled`. On Windows its command may
    // go on after the turn it was started by is cancelled (2026-10-03), and its lane reads
    // idle while it runs, so this is the sign of its interrupt there.
    let cancels = || cancelled_turns(&members.rig().home());
    let cancelled_before = cancels();
    if is_over(&members.task(number)?) || !running()? {
        WAITING.end_in(process)?;
        return Err(Error::Daemon(format!(
            "the {} window's turn was over before its interrupt",
            harness.name()
        )));
    }
    let interrupt = ask::interrupt(harness.kind())?;
    for press in 0..interrupt.presses {
        if press > 0 {
            sleep(DOUBLE_PRESS);
        }
        members.press(&window.pane, b"\x1b")?;
    }
    if let Some(after) = interrupt.close_after {
        sleep(after);
        members.press(&window.pane, b"\x1b")?;
    }
    let stopped = find(log, "the turn to stop", STOPS_WITHIN, LOOK, || {
        let over = is_over(&members.task(number)?);
        let ended = !running()?;
        let rested = busy_before && idle()?;
        let cancelled = cancels() > cancelled_before;
        Ok((over || ended || rested || cancelled).then_some((over, ended, rested || cancelled)))
    })?;
    let left = WAITING.end_in(process)?;
    log.say(format!("what was left of the command, ended: {left:?}"));
    typed?;
    if stopped.is_none() {
        log.block(
            "the worker's window",
            &last_lines(&members.rig().screen(window.id())),
        );
        if let Some(wire) = live::wire::tail(&members.rig().home()) {
            log.say(format!("Devin's wire: {wire}"));
        }
        return Err(Error::Daemon(format!(
            "the {} window's turn did not stop within {} s of its interrupt, after the human typed: \
             the interrupt did not reach it",
            harness.name(),
            STOPS_WITHIN.as_secs()
        )));
    }
    if let Some((over, ended, rested)) = stopped {
        log.say(format!(
            "ok: the interrupt after the typing stopped the turn (its task over: {over}, its \
             command ended: {ended}, its lane at rest after work or its turn cancelled: {rested})"
        ));
    }
    Ok(())
}

/// How many turns Devin's newest wire log in the run's `home` says were cancelled: none
/// where there is no Devin launch.
fn cancelled_turns(home: &Path) -> usize {
    live::wire::newest(home)
        .and_then(|file| std::fs::read_to_string(file).ok())
        .map_or(0, |text| {
            text.lines()
                .filter(|record| live::wire::kind_of(record) == "cause:cancelled")
                .count()
        })
}

/// [`TYPED`] typed into `window` as the page writes it (as it was typed), and whether every
/// mark of it shows on the window's screen.
fn typed_into(members: &Members, window: &members::MemberWindow, log: &Log) -> Result {
    members.press(&window.pane, TYPED.as_bytes())?;
    let shown = find(
        log,
        "the typed marks in the worker's box",
        SHOWN,
        LOOK,
        || {
            let screen = members.rig().screen(window.id());
            Ok(MARKS.iter().all(|mark| screen.contains(mark)).then_some(()))
        },
    )?;
    if shown.is_some() {
        log.say(format!(
            "ok: what the human typed shows whole: {}",
            MARKS.join(" ")
        ));
        return Ok(());
    }
    let screen = members.rig().screen(window.id());
    let missing: Vec<&str> = MARKS
        .iter()
        .copied()
        .filter(|mark| !screen.contains(mark))
        .collect();
    log.block("the worker's window", &last_lines(&screen));
    Err(Error::Daemon(format!(
        "the window does not show {} of what the human typed, which the same harness shows in a terminal",
        missing.join(" ")
    )))
}

/// One worker's check, from the project's opening to its close.
fn check(harness: Harness, log: &Log) -> Result {
    let case = match harness {
        Harness::Codex => codex_case(log)?,
        _ => devin_case()?,
    };
    let mut members = Members::open(&case.plan, Path::new(FAKE_AGENT), log)?;
    let said = answer(&mut members, harness, log);
    if said.is_err() {
        failure(&members, harness, log);
    }
    let typed = said
        .is_ok()
        .then(|| typed_then_interrupted(&members, harness, log));
    let closed = members.close();
    if let Some(home) = &case.codex_home {
        if home.rotated() {
            log.say(
                "!!! Codex refreshed the login it was given a copy of: the machine's own Codex login \
                 may be stale; run `codex login status` in a terminal",
            );
        }
    }
    let said = said?;
    closed?;
    log.block("the worker's answer", &said);
    typed.transpose()?;
    let missing: Vec<String> = case
        .words
        .iter()
        .filter(|(word, _)| !said.contains(word))
        .map(|(word, from)| format!("{word} ({from})"))
        .collect();
    if !missing.is_empty() {
        return Err(Error::Daemon(format!(
            "the {} worker did not name {}: it lacks what the same harness has in a terminal there",
            harness.name(),
            missing.join(", ")
        )));
    }
    let named: Vec<&str> = case.words.iter().map(|(word, _)| *word).collect();
    log.say(format!("ok: the worker names {}", named.join(", ")));
    Ok(())
}
