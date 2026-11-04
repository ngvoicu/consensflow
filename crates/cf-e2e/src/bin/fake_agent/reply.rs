//! What the window answers a message with. Every message is one turn.
//!
//! - `Reply with exactly: X` answers X.
//! - A line `SAY <words>` answers the words as they stand, with their spaces, in one
//!   line: a worker whose last words are any words a case needs (T-1119: a question,
//!   a wait), where `Reply with exactly` answers one word.
//! - A line `SAVED <file>[,<file>...] <words>` is an image designer's turn: it saves a
//!   picture under each name in the window's folder and answers the absolute path of
//!   each, a line apiece, and then the words, as the designer's role asks its result
//!   to begin (`skill/core/staff.md`).
//! - A line `LONG <characters>` is a worker whose result is a report of that many characters,
//!   lines of a few dozen words as a worker's are, with no picture: a message that Claude
//!   Code is given in pieces when the machine is loaded (T-763's live case).
//! - A line `DISPATCH --tier standard <task>` (or `DISPATCH --review --tier
//!   standard <task>`, and `--test` the same) runs `cf task add` with those
//!   words and the task (`\n` in it becomes a line break), with this window's
//!   own token, the way a chief hands out work. Each such line of a message is
//!   run, in order, so that one turn puts several tasks on the board at once, as
//!   a chief does that plans (the long run's stand-in chief).
//! - A line `CF <words>` runs any other `cf` with those words as they stand
//!   (`CF task cancel T-3`), and `CF <words> :: <text>` with the text, spaces
//!   and all, as one last word (`CF tell T-1 :: Stop now`): the chief's own
//!   verbs, as it would type them. Each such line of a message is run, in order
//!   (a chief that accepts three tasks in one turn: the matrix's stand-in chief),
//!   `CF_TEST_CF_GAP_MS` milliseconds apart where that is set (a chief slow between
//!   two commands, as a loaded machine makes it).
//! - A line `WRITE <file> :: <text>` is a worker that changes a file: it writes the
//!   text (`\n` in it a line break, and one at its end) to the file, a path below the
//!   window's folder, and answers `wrote <file>` (the matrix's stand-in worker, whose
//!   change the scenario's checker reads).
//! - A result delivered to the chief's window started with `CF_TEST_ACCEPT_RESULTS`
//!   set is accepted, `cf task accept` of the task its envelope (its first line, the
//!   result's header) names, and nothing else in it is done: a chief that takes its
//!   staff's results, as the long run's dry run needs to reach its end (every task
//!   accepted). A result header its words quote, a note's or a result's, is none.
//!   Without it a result is answered as any message is.
//! - A line `SLEEP <seconds> <words>` is a turn that works that long before it
//!   does what the words say: a window a chief can still tell, or a chief
//!   nothing is delivered to. An Escape stops it, as it stops Claude's: the turn
//!   ends there, and the window writes what Claude writes of a turn it was
//!   interrupted in (`take_turn`).
//! - A line `PRINT <bytes> <words>` is a turn that prints that many bytes of text to
//!   its screen, as a harness does that streams its model's reasoning, then does what
//!   the words say. A window whose terminal is not read on stops in the middle of it,
//!   as a harness's write to its terminal does.
//! - A line `HANG <seconds> <words>` is a turn hung that long, as a harness hung
//!   in its model request is (Pi on zeewin's long run, 2026-10-19): an Escape does
//!   nothing to it, and only the end of the window ends it before its time. Then it
//!   does what the words say.
//! - A line `HOOKS <seconds> <words>` is a first turn still in the hooks of its
//!   prompt for that long (the words that go in later, with this one carried in
//!   them, do not put the window back there): Claude's status reads idle, and
//!   the turn has written nothing but the message it was given. An Escape there
//!   stops it and leaves no record of it at all, as it does Claude's: only the
//!   key says it was stopped (`take_turn`).
//! - A line `BACKGROUND <seconds> <words>` starts a job (`background`) that runs
//!   that long and answers the words at once, as an agent does that leaves a job
//!   in the background and ends its turn. With `--wake` before the seconds the
//!   window begins a turn of its own when the job ends, as a harness does that
//!   its job's notification wakes (`WOKEN <words>`, which answers `woken:
//!   <words>`); with `--deaf` the job ignores the requests to end; with `--detach`
//!   the shell that runs it detaches it (`nohup … &`) and ends, which leaves the job
//!   with no parent, as the agents of four harnesses left theirs, and with no chain of
//!   processes to a shell the harness started. On Unix that shell is in a session of its
//!   own (Pi's, Claude's), so that the job is in a session no window has and the host cannot
//!   see it; with `--detach-here` the shell is in the window's session instead, and the job is
//!   there after it (T-1145: the host names it as work the window's close ends). On Windows the
//!   two are the same: a PowerShell's `Start-Process`, and the pane's job holds the job.
//! - A line `JOB [--deaf] [--detach|--detach-here] <seconds> <rest>` starts such a job and
//!   goes on with `rest` as the line it was given, as an agent does whose tool runs a command
//!   while its turn is at work (`JOB 120 SLEEP 60 words` leaves a job running for
//!   the whole of a minute's turn, which then answers `words`). With `--detach` the job
//!   is detached from its shell as in `BACKGROUND --detach`, and the turn goes on once the
//!   shell has ended: a job left with no parent while the turn works (`JOB --detach-here`
//!   keeps the shell in the window's session, as the shells of a harness that leads no
//!   session of its own do: what the window's close ends).
//! - A line `AGENTS <count> <seconds> <words>` starts that many background agents
//!   (`agents`: no program, as Claude Code's run in its own process) that end
//!   together after that long, and answers the words at once, as an agent does
//!   that leaves some running and ends its turn: the turn's `turn_duration` says
//!   how many are left, the window's status stays `busy` for them, and when they
//!   end the window begins a turn of its own (`WOKEN <words>`, which answers
//!   `woken: <words>`), as Claude Code does. `AGENTS --then <count> <seconds> <rest>`
//!   starts them so and goes on with `rest` as the line it was given, as a turn does
//!   that starts agents and works on (T-604's, 2026-10-25): an Escape or a refusal
//!   then ends a turn whose agents still run, and whose record counts none of them.
//! - A line `PACED <n> <seconds> <words>` is a turn that works through `n` steps, each a message
//!   that calls a tool, with `seconds` between one and the next, as the record of a turn at
//!   work grows (an Escape in one of them stops it there), and then answers the words. Unlike
//!   `STEPS`, which writes its steps at once, it can follow `REPLY … THEN`: a window that
//!   answers the chief's tell and carries on with its work, as the field's did (T-1139).
//! - A line `STEPS <n> <words>` is a turn that writes `n` lines of progress, each a
//!   message that calls a tool, before its answer, and `CLAUDE-JOB [--silent] <seconds>
//!   <words> :: <report>` a turn that starts a job in the background as Claude Code does,
//!   whose end wakes the window with a turn that answers the report (`steps` says how).
//! - A line `MODEL-BUSY` is a turn its provider refuses for a moment, its model overloaded:
//!   Claude's 529 record, as fake Codex's `MODEL-BUSY` is its `server_overloaded`; a line
//!   `MODEL-GONE` one it refuses for good, Claude's 400 record. Only the message that says it
//!   is refused: what comes after it is answered.
//! - A task saying `QUOTA-OUT` is refused with a 429, Claude's way, by the
//!   window whose participant `CF_TEST_QUOTA_OUT` names, for as long as the
//!   file `CF_TEST_QUOTA_FILE` names is there when it is set (taking it away is
//!   the human logging the harness into another account).
//! - Every message is refused so, whatever it says, by a window of a participant
//!   `CF_TEST_QUOTA_SPENT` names (a comma between names), for as long as that
//!   file is there: an account whose quota is spent refuses each turn of every
//!   window logged into it, the chief's too. A turn that works first (`SLEEP`) is
//!   refused when it asks again, after its work.
//! - A line `ASK <questions JSON>` asks through Claude's question tool: the
//!   PreToolUse hook of the settings file this window was launched with runs on
//!   a synthetic AskUserQuestion event, and the turn answers with what the hook
//!   handed back.
//! - A window started with `CF_TEST_SCRIPTED_RESULTS` set (a folder of an eval run's, the
//!   evals' stand-in reviewer or worker) answers each task brief with the next result of the
//!   folder, after writing the files of that result in its own folder, and nothing else of what
//!   is written here (`scripted`).
//! - A question whose text says `REPLY <words>` is answered with `cf answer`.
//! - `REPLY <words> THEN <line>` answers it so and goes on, in the same turn, with `line`
//!   as the line it was given, and ends with what that answers: a window that answers
//!   the chief's tell and carries on to its end (T-442).
//! - Anything else is acknowledged.

use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use cf_e2e::pattern;
use cf_e2e::process::{own_var, Run};
use regex::Regex;
use serde_json::{json, Value};

use crate::agents::Agents;
use crate::background::{until_up, Detach, Jobs, Server, THINKING};
use crate::scripted;
use crate::steps::{Step, Tasks};
use crate::stop::Stop;

/// How long the shell that detaches a job may take to end: a PowerShell's
/// `Start-Process` takes a second or two, a `/bin/sh` a moment.
const DETACHED_SHELL: Duration = Duration::from_secs(30);

/// How the flags of a `JOB` or a `BACKGROUND` line say the agent's tool detaches the command:
/// `--detach-here` leaves the shell in the window's session, `--detach` in one of its own
/// (on Windows both are a PowerShell's `Start-Process`), none does not detach it.
fn detaching(flags: &str) -> Detach {
    if flags.contains("--detach-here") {
        Detach::Here
    } else if flags.contains("--detach") {
        Detach::Apart
    } else {
        Detach::No
    }
}

/// The header of a result delivered to a window, with the task it is the result of.
const RESULT: &str = r"^\[ConsensFlow m-\d+ · (T-\d+) · result from @[^\]]+\]$";

/// The participant the chief's window is started as, before a switch of the chief and
/// after it.
const CHIEF: &str = "chief";

/// What a window knows of itself that decides how it answers.
pub struct Window {
    /// The participant the daemon named this window for (`worker-amber-pine`).
    pub participant: String,
    /// The settings file the window was launched with, whose hooks a question
    /// goes through.
    pub settings: Option<String>,
    /// The conversation, for the hook's event.
    pub session: String,
    /// The jobs the window starts.
    pub jobs: Arc<Jobs>,
    /// When its harness starts its server: the window's first message is what
    /// one that is late waits for.
    pub server: Server,
    /// The background agents the window starts.
    pub agents: Agents,
    /// The jobs it starts in the background that its harness wakes it for.
    pub tasks: Tasks,
    /// Where a turn the window begins by itself is queued: the message that
    /// wakes it when the job it left running ends.
    pub wake: Sender<String>,
    /// The Escape that stops the turn at work.
    pub stop: Arc<Stop>,
    /// The window's first message came with its launch, as an argument of the program, and
    /// was no paste: Claude Code makes no attachment of the paths of images in it.
    pub seeded: bool,
    /// What a command cancelled put back in the input box, until the terminal's reader
    /// takes it there.
    pub put_back: Arc<Mutex<Option<String>>>,
}

/// What a turn answers.
#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    Text(String),
    /// The provider refuses it: Claude writes the refusal as a record.
    Refused,
    /// The provider refuses it with an error of its API, which Claude writes as a record: a
    /// 529 for a model overloaded for a moment (`MODEL-BUSY`), a 400 for one it refuses for
    /// good (`MODEL-GONE`).
    ApiError(u16),
    /// An Escape stopped the turn before it answered.
    Stopped,
    /// The model it runs on is one its account does not have (`CF_TEST_GONE_MODEL`): the
    /// turn fails at once, as Claude Code 2.1.312's does.
    Gone,
    /// The turn works through these steps, a pause after each (an Escape in one of them
    /// stops it there), and then answers the text: a turn whose record grows while it
    /// is at work (`PACED`).
    Worked(Vec<Step>, Duration, String),
}

/// `cf` as an agent runs it: the one first on the window's `PATH`, whichever
/// ConsensFlow installed there. What it printed and said, trimmed.
pub fn run_cf(words: &[String]) -> String {
    let path = own_var("PATH").unwrap_or_default();
    let extensions = own_var("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned());
    let cf = first_on_path("cf", &path, &extensions).unwrap_or_else(|| "cf".into());
    match Run::new(cf).args(words).inheriting_env().unlimited().run() {
        Ok(ran) => ran.output().trim().to_owned(),
        Err(failed) => failed.to_string(),
    }
}

/// The first `name` in the folders of `path`, as a shell finds a command: on Windows with each
/// of the extensions `extensions` names (`PATHEXT`). A program started by its bare name on
/// Windows is looked for first in the folder of the program that starts it, which no shell
/// does: this stand-in's is cargo's `target\debug`, where another tree's build of `cf` may be
/// (zeewin2, T-1186: a3-1's `cf`, with T-1110's fix, ran for a tree without it).
fn first_on_path(name: &str, path: &str, extensions: &str) -> Option<std::path::PathBuf> {
    let extensions: Vec<&str> = if cfg!(windows) {
        extensions
            .split(';')
            .filter(|extension| !extension.is_empty())
            .collect()
    } else {
        vec![""]
    };
    std::env::split_paths(path).find_map(|folder| {
        extensions
            .iter()
            .map(|extension| folder.join(format!("{name}{extension}")))
            .find(|file| file.is_file())
    })
}

/// Claude's question tool: the settings' PreToolUse hook answers, or the
/// window's dialog would. The hook is run in the platform's shell, as Claude
/// runs one: `/bin/sh` here, `cmd.exe` on Windows.
fn ask_through_hook(window: &Window, questions: &Value) -> Result<String, String> {
    let file = window
        .settings
        .as_deref()
        .ok_or("the window was launched with no settings file")?;
    let text = std::fs::read_to_string(file).map_err(|failed| format!("{file}: {failed}"))?;
    let settings: Value =
        serde_json::from_str(&text).map_err(|failed| format!("{file}: {failed}"))?;
    let command = settings["hooks"]["PreToolUse"]
        .as_array()
        .and_then(|hooks| {
            hooks
                .iter()
                .find(|hook| hook["matcher"] == "AskUserQuestion")
        })
        .and_then(|hook| hook["hooks"][0]["command"].as_str())
        .ok_or_else(|| format!("{file} holds no PreToolUse hook for AskUserQuestion"))?;
    let event = json!({
        "session_id": window.session,
        "cwd": std::env::current_dir().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_default(),
        "permission_mode": "bypassPermissions",
        "hook_event_name": "PreToolUse",
        "tool_name": "AskUserQuestion",
        "tool_input": { "questions": questions },
    });
    let run = shell(command)
        .inheriting_env()
        .unlimited()
        .input(event.to_string());
    let ran = run.run().map_err(|failed| failed.to_string())?;
    let answers = serde_json::from_str::<Value>(&ran.stdout)
        .ok()
        .map(|output| output["hookSpecificOutput"]["updatedInput"]["answers"].clone())
        .filter(Value::is_object);
    if let Some(answers) = answers {
        let said: Vec<String> = questions
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(
                |question| match &answers[question["question"].as_str().unwrap_or_default()] {
                    Value::String(answer) => answer.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                },
            )
            .collect();
        return Ok(format!("answered: {}", said.join(" | ")));
    }
    let errors = ran.stderr.trim();
    let code = ran
        .code
        .map_or_else(|| "null".to_owned(), |code| code.to_string());
    let said = format!(
        "unanswered (exit {code}): {}",
        if errors.is_empty() {
            "no output"
        } else {
            errors
        }
    );
    Ok(said.chars().take(600).collect())
}

/// A command line run by the platform's shell.
#[cfg(unix)]
fn shell(command: &str) -> Run {
    Run::new("/bin/sh").args(["-c", command])
}

/// A hook's command line run as Claude Code runs one on Windows: by Git Bash's `bash -c`
/// (2.1.316, read from its binary: "Defaults to bash (powershell on Windows without Git
/// Bash)", T-1049), which takes the window's own `cf` as its hook names it, a quoted
/// whole path (`'C:/…/cf.exe' hook claude`). Where no Git Bash is found, as Node's
/// `shell: true` ran one: `cmd.exe /d /s /c "<command>"`, the quotes around it and its own
/// quotes written as they are.
#[cfg(windows)]
fn shell(command: &str) -> Run {
    if let Some(bash) = git_bash() {
        return Run::new(bash).args(["-c", command]);
    }
    let interpreter = own_var("ComSpec").unwrap_or_else(|| "cmd.exe".to_owned());
    Run::new(interpreter)
        .args(["/d", "/s", "/c"])
        .raw_arg(format!("\"{command}\""))
}

/// Git Bash, where Claude Code looks for it: the file `CLAUDE_CODE_GIT_BASH_PATH` names,
/// then the `bin\bash.exe` of the Git whose `git.exe` is on the PATH (`…\Git\cmd` or
/// `…\Git\bin`), then Git's own install folder.
#[cfg(windows)]
fn git_bash() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Some(named) = own_var("CLAUDE_CODE_GIT_BASH_PATH").map(PathBuf::from) {
        if named.is_file() {
            return Some(named);
        }
    }
    let path = own_var("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .filter(|folder| folder.join("git.exe").is_file())
        .filter_map(|folder| folder.parent().map(|git| git.join("bin").join("bash.exe")))
        .chain([PathBuf::from(r"C:\Program Files\Git\bin\bash.exe")])
        .find(|bash| bash.is_file())
}

/// Prints lines of text to the window's screen until they make `bytes`.
fn print_lines(bytes: usize) -> std::io::Result<()> {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let (mut printed, mut line) = (0, 0);
    while printed < bytes {
        line += 1;
        let text = format!("reasoning, line {line:08}, as a harness streams its model's words\r\n");
        out.write_all(text.as_bytes())?;
        printed += text.len();
    }
    out.flush()
}

/// The answer to `text`.
pub fn reply_to(text: &str, window: &Window) -> Result<Reply, String> {
    static PACED: OnceLock<Regex> = OnceLock::new();
    static SLEEP: OnceLock<Regex> = OnceLock::new();
    static HANG: OnceLock<Regex> = OnceLock::new();
    static PRINT: OnceLock<Regex> = OnceLock::new();
    static JOB: OnceLock<Regex> = OnceLock::new();
    static BACKGROUND: OnceLock<Regex> = OnceLock::new();
    static AGENTS: OnceLock<Regex> = OnceLock::new();
    static AGENTS_THEN: OnceLock<Regex> = OnceLock::new();
    static WOKEN: OnceLock<Regex> = OnceLock::new();
    static ASK: OnceLock<Regex> = OnceLock::new();
    static ASKED: OnceLock<Regex> = OnceLock::new();
    static REPLY: OnceLock<Regex> = OnceLock::new();
    static DISPATCH: OnceLock<Regex> = OnceLock::new();
    static VERB: OnceLock<Regex> = OnceLock::new();
    static REFUSED: OnceLock<Regex> = OnceLock::new();
    static SAVED: OnceLock<Regex> = OnceLock::new();
    static WRITE: OnceLock<Regex> = OnceLock::new();
    static LONG: OnceLock<Regex> = OnceLock::new();
    static EXACT: OnceLock<Regex> = OnceLock::new();
    static SAY: OnceLock<Regex> = OnceLock::new();
    static RESULTS: OnceLock<Regex> = OnceLock::new();

    if spent(window) {
        return Ok(Reply::Refused);
    }
    if let Some(folder) = own_var(scripted::FOLDER) {
        let here = std::env::current_dir().map_err(|failed| failed.to_string())?;
        if let Some(result) = scripted::answer(Path::new(&folder), text, &here)? {
            return Ok(Reply::Text(result));
        }
    }
    let accepts = accepting(
        own_var("CF_TEST_ACCEPT_RESULTS").as_deref(),
        &window.participant,
    );
    let accepted: Vec<String> = acceptances(text, pattern::once(&RESULTS, RESULT), accepts)
        .iter()
        .map(|words| run_cf(words))
        .collect();
    if !accepted.is_empty() {
        return Ok(Reply::Text(format!("accepted: {}", accepted.join(" | "))));
    }
    if let Some(paced) = pattern::once(&PACED, r"(?m)^PACED (\d+) (\d+) (.+)$").captures(text) {
        let (steps, seconds): (u64, u64) = match (paced[1].parse(), paced[2].parse()) {
            (Ok(steps), Ok(seconds)) => (steps, seconds),
            _ => return Err(format!("PACED {} {}", &paced[1], &paced[2])),
        };
        return Ok(Reply::Worked(
            (1..=steps).map(|at| Step::progress(at, steps)).collect(),
            Duration::from_secs(seconds),
            paced[3].to_owned(),
        ));
    }
    if let Some(slow) = pattern::once(&SLEEP, r"(?m)^SLEEP (\d+) (.+)$").captures(text) {
        let seconds: u64 = slow[1].parse().map_err(|_| format!("SLEEP {}", &slow[1]))?;
        if window.stop.work(Duration::from_secs(seconds)) {
            return Ok(Reply::Stopped);
        }
        return reply_to(&slow[2], window);
    }
    if let Some(print) = pattern::once(&PRINT, r"(?m)^PRINT (\d+) (.+)$").captures(text) {
        let bytes: usize = print[1]
            .parse()
            .map_err(|_| format!("PRINT {}", &print[1]))?;
        print_lines(bytes).map_err(|failed| format!("PRINT {bytes}: {failed}"))?;
        return reply_to(&print[2], window);
    }
    if let Some(hung) = pattern::once(&HANG, r"(?m)^HANG (\d+) (.+)$").captures(text) {
        let seconds: u64 = hung[1].parse().map_err(|_| format!("HANG {}", &hung[1]))?;
        thread::sleep(Duration::from_secs(seconds));
        return reply_to(&hung[2], window);
    }
    if let Some(job) = pattern::once(
        &JOB,
        r"(?m)^JOB ((?:--(?:deaf|detach-here|detach) )*)(\d+) (.+)$",
    )
    .captures(text)
    {
        let seconds: u64 = job[2].parse().map_err(|_| format!("JOB {}", &job[2]))?;
        let (deaf, detach) = (job[1].contains("--deaf"), detaching(&job[1]));
        // An agent thinks before it runs a tool (see `BACKGROUND`).
        thread::sleep(THINKING);
        let (shell_ended, ended) = std::sync::mpsc::channel();
        let file = window
            .jobs
            .start_detaching("job", seconds, deaf, detach, move || {
                let _ = shell_ended.send(());
            })?;
        until_up(&file)?;
        // A tool that detached a job returns once its shell has ended, and the turn goes
        // on from there (see `BACKGROUND`): the job has no parent from then on.
        if detach != Detach::No && ended.recv_timeout(DETACHED_SHELL).is_err() {
            return Err(format!(
                "JOB --detach: the shell that detached the job did not end in {} s",
                DETACHED_SHELL.as_secs()
            ));
        }
        return reply_to(&job[3], window);
    }
    if let Some(job) = pattern::once(
        &BACKGROUND,
        r"(?m)^BACKGROUND ((?:--(?:wake|deaf|detach-here|detach) )*)(\d+) (.+)$",
    )
    .captures(text)
    {
        let seconds: u64 = job[2]
            .parse()
            .map_err(|_| format!("BACKGROUND {}", &job[2]))?;
        let words = job[3].to_owned();
        let (wakes, deaf, detach) = (
            job[1].contains("--wake"),
            job[1].contains("--deaf"),
            detaching(&job[1]),
        );
        let detached = detach != Detach::No;
        if wakes && detached {
            return Err(
                "BACKGROUND --wake --detach: a job detached from its shell is no child to wait for"
                    .to_owned(),
            );
        }
        // An agent thinks before it runs a tool: the daemon marks where a launch's
        // turn begins when its message shows in the record, up to a pass after the
        // agent began.
        thread::sleep(THINKING);
        let (wake, woken) = (window.wake.clone(), format!("WOKEN {words}"));
        // A command the agent's tool runs is run by a shell in a group of its own, as a
        // harness runs it, which waits for it or, as the agents of the harnesses do
        // when they leave a job, detaches it and ends. A tool that detached a job
        // returns once its shell has ended, and only then does the agent answer: an
        // answer while the shell still ran left the job under a running shell, the
        // turn's work by the chain (a PowerShell's `Start-Process` takes a second or
        // two: Windows CI and zeewin, 2026-10-14).
        let (shell_ended, ended) = std::sync::mpsc::channel();
        let file = window
            .jobs
            .start_detaching("job", seconds, deaf, detach, move || {
                if wakes {
                    let _ = wake.send(woken);
                }
                let _ = shell_ended.send(());
            })?;
        until_up(&file)?;
        if detached && ended.recv_timeout(DETACHED_SHELL).is_err() {
            return Err(format!(
                "BACKGROUND --detach: the shell that detached the job did not end in {} s",
                DETACHED_SHELL.as_secs()
            ));
        }
        return Ok(Reply::Text(words));
    }
    if let Some(agents) = pattern::once(&AGENTS, r"(?m)^AGENTS (\d+) (\d+) (.+)$").captures(text) {
        let count: u64 = agents[1]
            .parse()
            .map_err(|_| format!("AGENTS {}", &agents[1]))?;
        let seconds: u64 = agents[2]
            .parse()
            .map_err(|_| format!("AGENTS {} {}", &agents[1], &agents[2]))?;
        let words = agents[3].to_owned();
        let (wake, woken) = (window.wake.clone(), format!("WOKEN {words}"));
        window.agents.start(count, seconds, move || {
            let _ = wake.send(woken);
        });
        return Ok(Reply::Text(words));
    }
    if let Some(agents) =
        pattern::once(&AGENTS_THEN, r"(?m)^AGENTS --then (\d+) (\d+) (.+)$").captures(text)
    {
        let count: u64 = agents[1]
            .parse()
            .map_err(|_| format!("AGENTS --then {}", &agents[1]))?;
        let seconds: u64 = agents[2]
            .parse()
            .map_err(|_| format!("AGENTS --then {} {}", &agents[1], &agents[2]))?;
        let (wake, woken) = (window.wake.clone(), "WOKEN the agents ended".to_owned());
        window.agents.start(count, seconds, move || {
            let _ = wake.send(woken);
        });
        return reply_to(&agents[3], window);
    }
    if let Some(woken) = pattern::once(&WOKEN, r"(?m)^WOKEN (.+)$").captures(text) {
        return Ok(Reply::Text(format!("woken: {}", &woken[1])));
    }
    if let Some(ask) = pattern::once(&ASK, r"(?m)^ASK (.+)$").captures(text) {
        let questions: Value =
            serde_json::from_str(&ask[1]).map_err(|failed| format!("ASK {}: {failed}", &ask[1]))?;
        return ask_through_hook(window, &questions).map(Reply::Text);
    }
    let asked =
        pattern::once(&ASKED, r"(?m)^\[ConsensFlow m-(\d+)[^\]]*question from @").captures(text);
    let reply = pattern::once(&REPLY, r"(?m)REPLY (.+)$").captures(text);
    if let (Some(asked), Some(reply)) = (asked, reply) {
        let (answer, goes_on) = match reply[1].split_once(" THEN ") {
            Some((answer, line)) => (answer, Some(line)),
            None => (&reply[1], None),
        };
        let words = [
            "answer".to_owned(),
            format!("m-{}", &asked[1]),
            answer.to_owned(),
        ];
        let replied = run_cf(&words);
        return match goes_on {
            Some(line) => reply_to(line, window),
            None => Ok(Reply::Text(format!("replied: {replied}"))),
        };
    }
    let dispatched: Vec<String> = dispatches(
        text,
        pattern::once(
            &DISPATCH,
            r"(?m)^DISPATCH ((?:(?:--(?:advice|review|test|design|self)|--\S+ \S+) )+)(.+)$",
        ),
    )
    .iter()
    .map(|words| run_cf(words))
    .collect();
    if !dispatched.is_empty() {
        return Ok(Reply::Text(format!(
            "dispatched: {}",
            dispatched.join(" | ")
        )));
    }
    let gap = own_var("CF_TEST_CF_GAP_MS")
        .and_then(|ms| ms.parse::<u64>().ok())
        .map(Duration::from_millis);
    let verbs: Vec<String> = verbs(text, pattern::once(&VERB, r"(?m)^CF (.+)$"))
        .iter()
        .enumerate()
        .map(|(index, words)| {
            if let Some(gap) = gap.filter(|_| index > 0) {
                thread::sleep(gap);
            }
            run_cf(words)
        })
        .collect();
    if !verbs.is_empty() {
        return Ok(Reply::Text(format!("ran cf: {}", verbs.join(" | "))));
    }
    if let Some(write) = pattern::once(&WRITE, r"(?m)^WRITE (\S+) :: (.*)$").captures(text) {
        let folder = std::env::current_dir().map_err(|failed| failed.to_string())?;
        return written_in(&folder, &write[1], &write[2]).map(Reply::Text);
    }
    // A model refused for a moment (a 529, Anthropic's overloaded), as fake Codex's
    // `MODEL-BUSY` is its server_overloaded: the live matrix's field report 4 (T-1112).
    if text.lines().any(|line| line.trim() == "MODEL-BUSY") {
        return Ok(Reply::ApiError(529));
    }
    // A model its provider refuses for good (a 400): a turn that fails however it is waited
    // for, the live matrix's field report 5 (T-1113).
    if text.lines().any(|line| line.trim() == "MODEL-GONE") {
        return Ok(Reply::ApiError(400));
    }
    if refused(text, window, pattern::once(&REFUSED, "QUOTA-OUT")) {
        return Ok(Reply::Refused);
    }
    if let Some(long) = pattern::once(&LONG, r"(?m)^LONG (\d+)$").captures(text) {
        let length = long[1]
            .parse::<usize>()
            .map_err(|failed| failed.to_string())?;
        return Ok(Reply::Text(a_report(length)));
    }
    if let Some(saved) = pattern::once(&SAVED, r"(?m)^SAVED (\S+) (.+)$").captures(text) {
        let folder = std::env::current_dir().map_err(|failed| failed.to_string())?;
        return saved_in(&folder, &saved[1], &saved[2]).map(Reply::Text);
    }
    if let Some(said) = pattern::once(&SAY, r"(?m)^SAY (.+)$").captures(text) {
        return Ok(Reply::Text(said[1].trim_end().to_owned()));
    }
    if let Some(exact) = pattern::once(&EXACT, r"Reply with exactly: (\S+)").captures(text) {
        return Ok(Reply::Text(exact[1].to_owned()));
    }
    Ok(Reply::Text(format!(
        "noted: {}",
        text.split('\n').next().unwrap_or_default()
    )))
}

/// A worker's report of at least `length` characters: numbered lines of words, whole lines, the
/// last one ending the report.
fn a_report(length: usize) -> String {
    let mut lines = Vec::new();
    let mut written = 0;
    for section in 1.. {
        let line = format!("{section}. Section {section} \u{2014} headings, links and the footer all match the page that was asked for.");
        written += line.chars().count() + 1;
        lines.push(line);
        if written >= length {
            break;
        }
    }
    lines.join("\n")
}

/// An image designer's answer: a picture saved under each of `names` (a comma
/// between) in `folder`, and then `words`, the paths first as its role asks.
fn saved_in(folder: &Path, names: &str, words: &str) -> Result<String, String> {
    let mut lines = Vec::new();
    for name in names.split(',') {
        let path = folder.join(name);
        std::fs::write(&path, cf_e2e::png::SMALL)
            .map_err(|failed| format!("SAVED {name}: {failed}"))?;
        lines.push(path.display().to_string());
    }
    lines.push(words.to_owned());
    Ok(lines.join("\n"))
}

/// A worker's change: `text` (each `\n` in it a line break, and a line break at its end)
/// written to `name`, a path below `folder`, which is answered as `wrote <name>`. A name
/// that leaves the folder (absolute, or with `..`) is refused: the stand-in writes only
/// in the project it works in.
fn written_in(folder: &Path, name: &str, text: &str) -> Result<String, String> {
    let relative = Path::new(name);
    let inside = relative
        .components()
        .all(|part| matches!(part, std::path::Component::Normal(_)));
    if !inside {
        return Err(format!(
            "WRITE {name}: not a path below the window's folder"
        ));
    }
    let path = folder.join(relative);
    std::fs::write(&path, format!("{}\n", text.replace("\\n", "\n")))
        .map_err(|failed| format!("WRITE {name}: {failed}"))?;
    Ok(format!("wrote {name}"))
}

/// The words of `cf` each `CF` line of `text` asks for, in the order of the lines
/// (`line` is the pattern of one): its words as they stand, and what follows a ` :: `
/// as one last word, spaces and all.
fn verbs(text: &str, line: &Regex) -> Vec<Vec<String>> {
    let each = line.captures_iter(text);
    each.map(|verb| {
        let mut parts = verb[1].split(" :: ");
        let mut words: Vec<String> = parts
            .next()
            .unwrap_or_default()
            .split(' ')
            .map(str::to_owned)
            .collect();
        let said: Vec<&str> = parts.collect();
        if !said.is_empty() {
            words.push(said.join(" :: "));
        }
        words
    })
    .collect()
}

/// The words of `cf task add` each `DISPATCH` line of `text` asks for, in the
/// order of the lines (`line` is the pattern of one): its flags, and its task with
/// each `\n` a line break.
fn dispatches(text: &str, line: &Regex) -> Vec<Vec<String>> {
    line.captures_iter(text)
        .map(|dispatch| {
            let mut words = vec!["task".to_owned(), "add".to_owned()];
            words.extend(dispatch[1].trim().split(' ').map(str::to_owned));
            words.push(dispatch[2].replace("\\n", "\n"));
            words
        })
        .collect()
}

/// The words of `cf task accept` of the result the delivery `text` is: where its own
/// envelope, its first line, is a result's header (`header` is its pattern), the task it
/// names; none where it is not, whatever its words quote, and none for a window that
/// is not `accepting`.
fn acceptances(text: &str, header: &Regex, accepting: bool) -> Vec<Vec<String>> {
    if !accepting {
        return Vec::new();
    }
    text.lines()
        .next()
        .and_then(|envelope| header.captures(envelope))
        .map(|result| vec!["task".to_owned(), "accept".to_owned(), result[1].to_owned()])
        .into_iter()
        .collect()
}

/// Whether the window of `participant` accepts the results delivered to it: the chief's,
/// started with `CF_TEST_ACCEPT_RESULTS` set to something (`switched_on`).
fn accepting(switched_on: Option<&str>, participant: &str) -> bool {
    switched_on.is_some_and(|on| !on.is_empty()) && participant == CHIEF
}

/// Whether the provider refuses this window the message `text`: it says
/// `QUOTA-OUT`, and the window is a session of the member `CF_TEST_QUOTA_OUT`
/// names, and the account is the refused one (`CF_TEST_QUOTA_FILE`, when it is
/// set, is still there).
fn refused(text: &str, window: &Window, quota_out: &Regex) -> bool {
    let refusing = own_var("CF_TEST_QUOTA_OUT").unwrap_or_default();
    let refused = own_var("CF_TEST_QUOTA_FILE").is_none_or(|account| Path::new(&account).exists());
    quota_out.is_match(text)
        && !refusing.is_empty()
        && refused
        && is_session_of(&window.participant, &refusing)
}

/// Whether the provider refuses this window whatever it is given: the window is
/// a session of a participant `CF_TEST_QUOTA_SPENT` names, and the account it
/// shares with them is spent (`CF_TEST_QUOTA_FILE` is there).
fn spent(window: &Window) -> bool {
    let sharing = own_var("CF_TEST_QUOTA_SPENT").unwrap_or_default();
    let spent = own_var("CF_TEST_QUOTA_FILE").is_some_and(|account| Path::new(&account).exists());
    spent
        && sharing
            .split(',')
            .any(|name| !name.is_empty() && is_session_of(&window.participant, name))
}

/// Whether `participant` is the member `member`, or a session of it
/// (`member-amber-pine`).
pub fn is_session_of(participant: &str, member: &str) -> bool {
    participant == member || participant.starts_with(&format!("{member}-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> Window {
        Window {
            participant: "chief".into(),
            settings: None,
            session: "sess".into(),
            jobs: Arc::new(Jobs::new(Path::new("/nowhere/consensflow"), 1)),
            server: Server::Never,
            agents: Agents::default(),
            tasks: Tasks::default(),
            wake: std::sync::mpsc::channel().0,
            stop: Arc::new(Stop::default()),
            seeded: false,
            put_back: Arc::default(),
        }
    }

    fn text(reply: Result<Reply, String>) -> String {
        match reply {
            Ok(Reply::Text(text)) => text,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_message_with_a_line_model_busy_is_refused_for_a_moment_and_the_next_is_answered() {
        let busy = reply_to(
            "[ConsensFlow m-4 \u{b7} T-1 \u{b7} task from @chief]\nMODEL-BUSY",
            &window(),
        );
        assert_eq!(busy, Ok(Reply::ApiError(529)));
        let gone = reply_to(
            "[ConsensFlow m-4 \u{b7} T-1 \u{b7} task from @chief]\nMODEL-GONE",
            &window(),
        );
        assert_eq!(gone, Ok(Reply::ApiError(400)));
        let next = reply_to("Resumed: Go on where you stopped.", &window());
        assert!(matches!(next, Ok(Reply::Text(_))), "{next:?}");
        // Words that only name it are no refusal.
        assert!(matches!(
            reply_to("Say MODEL-BUSY to me", &window()),
            Ok(Reply::Text(_))
        ));
    }

    #[test]
    fn a_message_that_asks_for_a_word_is_answered_with_it() {
        let reply = reply_to("Reply with exactly: WORKER_OK", &window());
        assert_eq!(text(reply), "WORKER_OK");
        // Anywhere in the message, as a task delivered to a window has its header first.
        let reply = reply_to(
            "[ConsensFlow m-4 · T-1 · task from @chief]\nReply with exactly: ONE_DONE",
            &window(),
        );
        assert_eq!(text(reply), "ONE_DONE");
    }

    #[test]
    fn a_line_that_says_words_is_answered_with_them_whole() {
        let reply = reply_to(
            "[ConsensFlow m-4 · T-1 · task from @chief]\nSAY Should I continue, or stop here?",
            &window(),
        );
        assert_eq!(text(reply), "Should I continue, or stop here?");
    }

    #[test]
    fn a_designer_saves_its_pictures_and_answers_their_paths_before_its_words() {
        let folder = tempfile::tempdir().unwrap();
        let answer = saved_in(folder.path(), "desk.png,hero.png", "Warm editorial scene.").unwrap();
        let (desk, hero) = (
            folder.path().join("desk.png"),
            folder.path().join("hero.png"),
        );
        assert_eq!(
            answer,
            format!(
                "{}\n{}\nWarm editorial scene.",
                desk.display(),
                hero.display()
            )
        );
        for picture in [desk, hero] {
            let bytes = std::fs::read(picture).unwrap();
            assert!(cf_e2e::png::inspect(&bytes).is_ok());
        }
        assert!(saved_in(&folder.path().join("none"), "a.png", "x")
            .unwrap_err()
            .starts_with("SAVED a.png: "));
    }

    #[test]
    fn a_worker_writes_its_change_below_its_folder_and_answers_the_file() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            written_in(folder.path(), "greet.cjs", "a();\\nb();").unwrap(),
            "wrote greet.cjs"
        );
        assert_eq!(
            std::fs::read_to_string(folder.path().join("greet.cjs")).unwrap(),
            "a();\nb();\n"
        );
        for outside in ["../x.cjs", "/tmp/x.cjs", "a/../../x.cjs"] {
            let refused = written_in(folder.path(), outside, "x").unwrap_err();
            assert!(
                refused.starts_with(&format!("WRITE {outside}: ")),
                "{refused}"
            );
        }
        assert!(!folder.path().join("../x.cjs").exists());
    }

    #[test]
    fn each_cf_line_of_a_message_is_run_in_order_with_its_last_word_whole() {
        let line = Regex::new(r"(?m)^CF (.+)$").unwrap();
        assert_eq!(
            verbs(
                "CF task accept T-1\nnoise\nCF tell T-2 :: Stop now :: please",
                &line
            ),
            [
                vec!["task", "accept", "T-1"],
                vec!["tell", "T-2", "Stop now :: please"],
            ]
        );
        assert!(verbs("no verb here", &line).is_empty());
    }

    #[test]
    fn a_long_worker_answers_a_report_of_the_length_it_was_asked_for_in_whole_lines() {
        let report = text(reply_to("LONG 12000", &window()));
        let length = report.chars().count();
        assert!(
            (12_000..12_000 + 100).contains(&length),
            "{length} characters"
        );
        assert!(report
            .lines()
            .all(|line| line.starts_with(|c: char| c.is_ascii_digit())));
        assert!(report.lines().count() > 100);
        assert!(!report.contains("[Image"), "no picture, so no attachment");
        assert!(!report.contains('/'), "no path");
        // Anywhere in the message, as a task delivered to a window has its header first.
        let reply = reply_to(
            "[ConsensFlow m-4 \u{b7} T-1 \u{b7} task from @chief]\nLONG 100",
            &window(),
        );
        assert!(text(reply).starts_with("1. Section 1"));
    }

    #[test]
    fn any_other_message_is_acknowledged_by_its_first_line() {
        assert_eq!(text(reply_to("hello\nsecond", &window())), "noted: hello");
        assert_eq!(text(reply_to("", &window())), "noted: ");
    }

    #[test]
    fn a_window_is_a_session_of_a_member_by_its_name_and_the_two_words_after_it() {
        assert!(is_session_of("worker", "worker"));
        assert!(is_session_of("worker-amber-pine", "worker"));
        assert!(!is_session_of("worker2-amber-pine", "worker"));
        assert!(!is_session_of("chief", "worker"));
        assert!(!is_session_of("worker", "worker-amber"));
    }

    #[test]
    fn a_paced_turn_works_through_its_steps_a_pause_after_each_and_answers_the_words_after_them() {
        let reply = reply_to("PACED 3 2 Reply with exactly: REPORT", &window()).unwrap();
        let Reply::Worked(steps, gap, answer) = reply else {
            panic!("a paced turn: {reply:?}");
        };
        assert_eq!(gap, Duration::from_secs(2));
        assert_eq!(
            steps
                .iter()
                .map(|step| step.said.as_str())
                .collect::<Vec<_>>(),
            ["Step 1 of 3", "Step 2 of 3", "Step 3 of 3"]
        );
        assert_eq!(answer, "Reply with exactly: REPORT");
    }

    /// `cf` is the first on the window's `PATH`, as a shell finds it, and none elsewhere is.
    #[test]
    fn cf_is_the_first_on_the_path_and_nothing_else_is_looked_at() {
        let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let file = |folder: &tempfile::TempDir| {
            folder
                .path()
                .join(if cfg!(windows) { "cf.EXE" } else { "cf" })
        };
        std::fs::write(file(&second), "").unwrap();
        let path = std::env::join_paths([first.path(), second.path()]).unwrap();
        let path = path.to_str().unwrap();
        let found = first_on_path("cf", path, ".COM;.EXE;.BAT;.CMD").unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            file(&second).canonicalize().unwrap()
        );
        std::fs::write(file(&first), "").unwrap();
        let found = first_on_path("cf", path, ".COM;.EXE;.BAT;.CMD").unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            file(&first).canonicalize().unwrap()
        );
        assert_eq!(first_on_path("cf", "", ".EXE"), None);
    }

    #[test]
    fn a_paced_turn_follows_the_answer_to_a_tell_in_the_same_turn() {
        // `REPLY … THEN PACED …`: the window answers the chief's tell with `cf answer` and
        // goes on working (T-1139), the answer to the tell failing here as no `cf` is run.
        let reply = reply_to(
            "[ConsensFlow m-3 \u{b7} T-1 \u{b7} question from @chief]\nStop. REPLY ok THEN PACED 2 1 Reply with exactly: END",
            &window(),
        )
        .unwrap();
        assert!(
            matches!(reply, Reply::Worked(steps, _, answer) if steps.len() == 2 && answer.ends_with("END"))
        );
    }

    #[test]
    fn a_message_that_says_sleep_works_that_long_before_it_does_what_the_rest_says() {
        let started = std::time::Instant::now();
        let reply = reply_to("SLEEP 1 Reply with exactly: LATE", &window());
        assert_eq!(text(reply), "LATE");
        assert!(started.elapsed() >= Duration::from_secs(1));
    }

    #[test]
    fn a_turn_that_works_is_stopped_by_an_escape_before_it_answers_and_the_words_after_it_are_never_said(
    ) {
        let window = window();
        window.stop.begin();
        let stop = Arc::clone(&window.stop);
        let presser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            stop.press();
        });
        let started = std::time::Instant::now();
        let reply = reply_to("SLEEP 30 Reply with exactly: LATE", &window);
        assert_eq!(reply, Ok(Reply::Stopped));
        assert!(started.elapsed() < Duration::from_secs(20));
        presser.join().unwrap();
    }

    #[test]
    fn a_hung_turn_goes_on_through_an_escape_and_does_what_the_rest_says_after_its_time() {
        let window = window();
        window.stop.begin();
        let stop = Arc::clone(&window.stop);
        let presser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            stop.press();
        });
        let started = std::time::Instant::now();
        let reply = reply_to("HANG 1 Reply with exactly: LATE", &window);
        assert_eq!(text(reply), "LATE");
        assert!(started.elapsed() >= Duration::from_secs(1));
        presser.join().unwrap();
    }

    #[test]
    fn the_turn_a_jobs_end_wakes_says_what_woke_it() {
        let reply = reply_to("WOKEN started the job", &window());
        assert_eq!(text(reply), "woken: started the job");
    }

    #[test]
    fn an_agents_line_leaves_that_many_running_answers_its_words_and_wakes_a_turn_when_they_end() {
        let (wake, woken) = std::sync::mpsc::channel();
        let window = Window { wake, ..window() };
        let reply = reply_to("AGENTS 2 0 started the research", &window);
        assert_eq!(text(reply), "started the research");
        assert!(window.agents.used());
        assert_eq!(
            woken.recv_timeout(Duration::from_secs(5)).unwrap(),
            "WOKEN started the research"
        );
        assert_eq!(window.agents.running(), 0);

        // Some that run on are what the turn that follows leaves running.
        let window = self::window();
        let reply = reply_to("AGENTS 3 600 waiting on three", &window);
        assert_eq!(text(reply), "waiting on three");
        assert_eq!(window.agents.running(), 3);
    }

    #[test]
    fn an_agents_then_line_starts_them_and_goes_on_with_the_rest_of_the_line() {
        let window = window();
        let reply = reply_to("AGENTS --then 2 600 Reply with exactly: SWEEPING", &window);
        assert_eq!(text(reply), "SWEEPING", "the rest is the turn's");
        assert_eq!(window.agents.running(), 2, "and they run on");
    }

    #[test]
    fn an_agents_line_that_names_no_count_or_seconds_is_only_acknowledged() {
        for line in ["AGENTS started the research", "AGENTS 2 soon words"] {
            let window = window();
            assert_eq!(text(reply_to(line, &window)), format!("noted: {line}"));
            assert!(!window.agents.used(), "{line}");
        }
    }

    #[test]
    fn a_background_line_that_is_no_job_is_only_acknowledged() {
        // No seconds, or none a number: nothing is started, and nothing waited for.
        for line in [
            "BACKGROUND started the job",
            "BACKGROUND --wake soon words",
            "JOB soon SLEEP 3 words",
            "JOB 5",
        ] {
            let started = std::time::Instant::now();
            assert_eq!(text(reply_to(line, &window())), format!("noted: {line}"));
            assert!(started.elapsed() < Duration::from_secs(1), "{line}");
        }
    }

    #[test]
    fn a_job_that_is_detached_from_its_shell_is_no_child_to_wake_the_window_when_it_ends() {
        let started = std::time::Instant::now();
        let failed = reply_to("BACKGROUND --wake --detach 5 words", &window()).unwrap_err();
        assert!(failed.starts_with("BACKGROUND --wake --detach"), "{failed}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "it started nothing"
        );
    }

    #[test]
    fn a_question_is_asked_through_the_hook_of_the_settings_and_not_without_them() {
        let failed = reply_to(r#"ASK [{"question":"Which?"}]"#, &window()).unwrap_err();
        assert_eq!(failed, "the window was launched with no settings file");
        let failed = reply_to("ASK not json", &window()).unwrap_err();
        assert!(failed.starts_with("ASK not json: "), "{failed}");
    }

    #[cfg(unix)]
    #[test]
    fn the_hook_is_given_the_event_and_what_it_hands_back_is_the_answer() {
        let folder = tempfile::tempdir().unwrap();
        let settings = folder.path().join("settings.json");
        // A hook that answers the question it is asked with its own words.
        std::fs::write(
            &settings,
            json!({ "hooks": { "PreToolUse": [
                { "matcher": "Other", "hooks": [{ "command": "exit 9" }] },
                { "matcher": "AskUserQuestion", "hooks": [{ "command":
                    "cat > /dev/null; printf '%s' '{\"hookSpecificOutput\":{\"updatedInput\":{\"answers\":{\"Which colour?\":\"blue\",\"And?\":\"two\"}}}}'" }] },
            ] } })
            .to_string(),
        )
        .unwrap();
        let window = Window {
            settings: Some(settings.to_string_lossy().into_owned()),
            ..window()
        };
        let questions = json!([{ "question": "Which colour?" }, { "question": "And?" }, { "question": "Unasked" }]);
        assert_eq!(
            ask_through_hook(&window, &questions).unwrap(),
            "answered: blue | two | "
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_hook_that_answers_nothing_is_unanswered_with_its_exit_and_what_it_said() {
        let folder = tempfile::tempdir().unwrap();
        let settings = folder.path().join("settings.json");
        std::fs::write(
            &settings,
            json!({ "hooks": { "PreToolUse": [
                { "matcher": "AskUserQuestion", "hooks": [{ "command": "cat > /dev/null; echo no way >&2; exit 2" }] },
            ] } })
            .to_string(),
        )
        .unwrap();
        let window = Window {
            settings: Some(settings.to_string_lossy().into_owned()),
            ..window()
        };
        assert_eq!(
            ask_through_hook(&window, &json!([])).unwrap(),
            "unanswered (exit 2): no way"
        );
    }

    #[test]
    fn each_dispatch_line_of_a_message_is_a_task_added_in_the_order_of_the_lines() {
        let line = Regex::new(
            r"(?m)^DISPATCH ((?:(?:--(?:advice|review|test|design|self)|--\S+ \S+) )+)(.+)$",
        )
        .unwrap();
        let text = "The plan.\nDISPATCH --tier complex JOB 60 SLEEP 30 Build\\nit\nnot a line: DISPATCH --tier light no\nDISPATCH --review --tier light Look at it";
        let words: Vec<Vec<String>> = dispatches(text, &line);
        assert_eq!(
            words,
            [
                vec![
                    "task",
                    "add",
                    "--tier",
                    "complex",
                    "JOB 60 SLEEP 30 Build\nit"
                ],
                vec!["task", "add", "--review", "--tier", "light", "Look at it"],
            ]
        );
        assert!(dispatches("DISPATCH nothing", &line).is_empty());
    }

    #[test]
    fn a_chief_accepts_the_result_its_delivery_opens_with_and_no_header_its_words_hold() {
        let header = Regex::new(RESULT).unwrap();
        let result = "[ConsensFlow m-18 · T-2 · result from @long-claude-2-amber-lagoon]\n\
                      Reply with exactly: DEV-SERVER-UP";
        assert_eq!(
            acceptances(result, &header, true),
            [["task", "accept", "T-2"]]
        );
        // A result header on a line of a delivery's words is a quote, whatever the
        // delivery: a note's (Astra's and Sol's), or a result's that quotes another.
        let note = "[ConsensFlow m-201 · T-7 · note from @human]\nFor the record:\n\
                    [ConsensFlow m-200 · T-9 · result from @long-worker]\nDone.";
        assert!(acceptances(note, &header, true).is_empty());
        let quoting = format!(
            "{result}\n[ConsensFlow m-19 · T-4 · result from @long-devin-2-lively-meadow]\n\
             DEVIN-MOVED-IT"
        );
        assert_eq!(
            acceptances(&quoting, &header, true),
            [["task", "accept", "T-2"]]
        );
        // A task, a note and a question are no results; a header that does not open the
        // delivery is none of its envelope; and a window not started to accept them
        // takes none.
        for other in [
            "[ConsensFlow m-22 · T-1 · task from @human]\nResumed: Go on where you stopped.",
            "[ConsensFlow m-39 · T-16 · note from ConsensFlow]\nGo on where you stopped.",
            "[ConsensFlow m-7 · T-3 · question from @worker]\nWhich one?",
            "a line that quotes [ConsensFlow m-18 · T-2 · result from @x]",
            "noted:\n[ConsensFlow m-18 · T-2 · result from @x]",
        ] {
            assert!(acceptances(other, &header, true).is_empty(), "{other}");
        }
        assert!(acceptances(result, &header, false).is_empty());
    }

    #[test]
    fn only_the_chiefs_window_started_to_accept_results_accepts_them() {
        assert!(accepting(Some("1"), "chief"));
        // A worker is given no result; one handed a result anyway takes none.
        assert!(!accepting(Some("1"), "long-claude-1-amber-pine"));
        assert!(!accepting(Some(""), "chief"));
        assert!(!accepting(None, "chief"));
    }
}
