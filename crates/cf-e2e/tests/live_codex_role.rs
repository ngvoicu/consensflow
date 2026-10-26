//! Live: a Codex window follows a role that is on no command line. It is opt-in
//! (`cargo xtask live codex-role`): it spends the machine's quota on a Codex chief's
//! few one-line turns, and needs Codex installed and logged in, so it is ignored
//! unless asked for and is no part of `check` or of CI.
//!
//! ConsensFlow gave a Codex window its role as `-c developer_instructions=<JSON>` on
//! the command line of its supervisor, which put it on the command lines of Codex's
//! app-server and TUI too: anyone on the machine reads a command line (`ps` is setuid
//! root on macOS), and a pattern of a role's words matched the window (T-432). Since
//! T-698 the role is a file of the launch's own, which the supervisor reads and its
//! broker gives the TUI's main thread start as its `developerInstructions`. That
//! replaces what Codex's own configuration says for the thread (probed on Codex
//! 0.164.0), so the file holds Codex's own instructions before the role, as the
//! command line's setting did.
//!
//! This is the daemon's own path, on the rig of the live checks
//! ([`cf_e2e::live::members`]): a project whose chief is the machine's Codex on its
//! cheap model, in a Codex home of the run's own that holds a copy of the machine's
//! login and a `config.toml` whose developer instructions give a word of the human's
//! own ([`CodexHome`], which says if Codex refreshed the login). The chief's role is
//! an evaluation's card (`CONSENSFLOW_EVAL_CHIEF_CARD`), which gives it a code word.
//! The human asks it for its code words three times:
//!
//! 1. in the window the project opened (a thread that starts): it names the role's
//!    word and the human's own;
//! 2. in a window opened again on that thread (the project closed and resumed): the
//!    same words, and the thread holds the developer message it began with, once;
//! 3. after the card was changed to a new word, said to replace the first, and the
//!    project closed and resumed: the engine tells the thread the new role in a note
//!    (a resumed thread keeps its first instructions, `role_change`), and the chief
//!    names the new word.
//!
//! Each time, no process of the window (the supervisor, Codex's server, its TUI),
//! nor any other process on the machine, has a word of the role or of the human's
//! instructions on its command line: `ps` on the window's tree, and `pgrep -f`.
//!
//! ```text
//! cargo xtask live codex-role
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::codex::CodexHome;
use cf_e2e::live::members::{self, find, Members, Plan};
use cf_e2e::live::{installed, Harness, Log};
use cf_e2e::process::Run;
use cf_e2e::rollout;
use cf_e2e::{files, Error, Result};
use serde_json::{json, Value};

/// The stand-in for the windows the run plays itself: none here, but the rig asks.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// How long the chief has to answer, and to read idle once its window is opened again.
const ANSWER: Duration = Duration::from_secs(240);
const READY: Duration = Duration::from_secs(240);

/// The chief's handle, which names its lane.
const CHIEF: &str = "chief";

/// How often the board and the record are looked at.
const LOOK: Duration = Duration::from_millis(500);

/// How long a typed question is given to be taken before Enter is pressed again, and
/// how many times it is.
const TAKES: Duration = Duration::from_secs(5);
const ENTERS: usize = 3;

/// The words of one run: a word of the role, the word of the changed role, and the
/// human's own, each found nowhere else on the machine.
struct Words {
    role: String,
    changed: String,
    own: String,
}

impl Words {
    fn new() -> Self {
        let pid = std::process::id();
        Self {
            role: format!("ROLEALPHA{pid}"),
            changed: format!("ROLEBRAVO{pid}"),
            own: format!("OWNERWORD{pid}"),
        }
    }
}

/// The chief's card: the whole of its role, which gives it `word`.
fn card(word: &str, replaces: bool) -> String {
    let again = if replaces {
        " It replaces any code word you were given before."
    } else {
        ""
    };
    format!(
        "# A check of a Codex window's role\n\nYour role's code word is {word}.{again} When you are asked for your code words, answer in one line with every code word your instructions give you, separated by commas, and nothing else. Run no tools.\n"
    )
}

/// What `program` prints, run with `args`, and its exit code.
fn run(program: &str, args: &[&str]) -> Result<(Option<i32>, String)> {
    let ran = Run::new(Path::new(program))
        .args(args.iter().copied())
        .inheriting_env()
        .run()?;
    Ok((ran.code, ran.stdout.clone()))
}

/// The processes of the tree of `pid`, itself first, each with its command line as
/// `ps` reads it.
fn tree_of(pid: u32) -> Result<Vec<(u32, String)>> {
    let mut found = Vec::new();
    let mut next = vec![pid];
    while let Some(pid) = next.pop() {
        let (_, command) = run("ps", &["-ww", "-o", "command=", "-p", &pid.to_string()])?;
        if command.trim().is_empty() {
            continue;
        }
        found.push((pid, command.trim().to_owned()));
        let (_, children) = run("pgrep", &["-P", &pid.to_string()])?;
        next.extend(
            children
                .lines()
                .filter_map(|line| line.trim().parse::<u32>().ok()),
        );
        if found.len() > 64 {
            break;
        }
    }
    Ok(found)
}

/// That no command line holds any of `words`: the chief's window's tree as `ps` reads
/// it (which must hold Codex's server and TUI), and every process `pgrep -f` matches.
fn on_no_command_line(members: &Members, words: &[&str], log: &Log) -> Result {
    let window = members
        .window_of(CHIEF)?
        .ok_or_else(|| Error::Daemon("the chief has no window".to_owned()))?;
    let process = members
        .window_process(&window)
        .ok_or_else(|| Error::Daemon("the pane host named no process for the window".to_owned()))?;
    let tree = tree_of(process.pid)?;
    let commands: Vec<&str> = tree.iter().map(|(_, command)| command.as_str()).collect();
    if !commands
        .iter()
        .any(|command| command.contains("app-server"))
        || !commands.iter().any(|command| command.contains("--remote"))
    {
        return Err(Error::Daemon(format!(
            "the window's tree has not got Codex's server and TUI: {} processes",
            tree.len()
        )));
    }
    for word in words {
        if let Some((pid, _)) = tree.iter().find(|(_, command)| command.contains(word)) {
            return Err(Error::Daemon(format!(
                "pid {pid} of the window's tree has {word} on its command line"
            )));
        }
        let (code, found) = run("pgrep", &["-f", word])?;
        if code != Some(1) {
            return Err(Error::Daemon(format!(
                "a pattern of {word} matches processes: {}",
                found.split_whitespace().collect::<Vec<_>>().join(" ")
            )));
        }
    }
    log.say(format!(
        "ok: none of the {} processes of the window, nor any other, has the role on its command line",
        tree.len()
    ));
    Ok(())
}

/// The chief's thread's record: the one rollout of the run's Codex home.
fn record(home: &Path) -> Result<String> {
    match rollout::rollouts(home).as_slice() {
        [only] => rollout::read(only),
        none_or_more => Err(Error::Daemon(format!(
            "the run's Codex home has {} rollouts, not the chief's one",
            none_or_more.len()
        ))),
    }
}

/// What the agent said in `text` after the user's message that holds `nonce`.
fn answers_after(text: &str, nonce: &str) -> Vec<String> {
    let mut asked = false;
    let mut said = Vec::new();
    for record in rollout::records(text) {
        let payload = &record["payload"];
        if record["type"] != "response_item" || payload["type"] != "message" {
            continue;
        }
        let words: String = payload["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|part| part["text"].as_str())
            .collect();
        match payload["role"].as_str() {
            Some("user") if words.contains(nonce) => asked = true,
            Some("assistant") if asked => said.push(words),
            _ => {}
        }
    }
    said
}

/// The human asks the chief for its code words, as a paste and Enter into its window,
/// and the chief's answer is waited for in its thread's record.
fn ask(members: &Members, home: &Path, round: usize, log: &Log) -> Result<String> {
    wait_idle(members, log)?;
    let nonce = format!("Q{round}-{}", std::process::id());
    let question = format!("({nonce}) What are your code words?");
    log.say(format!("the human asks the chief: {question}"));
    let pane = members.lane(CHIEF)?["pane"].clone();
    members.paste(&pane, &question)?;
    for again in 0..=ENTERS {
        std::thread::sleep(TAKES);
        let taken = record(home)
            .map(|text| text.contains(&nonce))
            .unwrap_or(false);
        if taken || again == ENTERS {
            break;
        }
        members.press(&pane, &[13])?;
    }
    let said = find(log, "the chief's answer", ANSWER, LOOK, || {
        let text = record(home)?;
        let said = answers_after(&text, &nonce);
        let idle = members.lane(CHIEF)?["activity"]["state"] == "idle";
        Ok((idle && !said.is_empty()).then(|| said.join("\n")))
    })?
    .ok_or_else(|| Error::Daemon("the chief did not answer in time".to_owned()))?;
    log.block("the chief's answer", &said);
    Ok(said)
}

fn wait_idle(members: &Members, log: &Log) -> Result {
    find(log, "the chief to read idle", READY, LOOK, || {
        Ok((members.lane(CHIEF)?["activity"]["state"] == "idle").then_some(()))
    })?
    .ok_or_else(|| Error::Daemon("the chief never read idle".to_owned()))
}

/// The human closes the project and resumes it: the chief's window is opened again on
/// its thread. Its `pane.open`.
fn closed_and_resumed(members: &Members, log: &Log) -> Result<Value> {
    let before = members.lane(CHIEF)?["pane"].clone();
    let closed = members.page("project.close", json!({}))?;
    if closed["ok"] != true {
        return Err(Error::Daemon(format!("project.close: {closed}")));
    }
    members.resume()?;
    let frame = find(log, "the chief's window opened again", READY, LOOK, || {
        Ok(members
            .window_of(CHIEF)?
            .filter(|window| window.pane != before)
            .and_then(|window| window.open))
    })?
    .ok_or_else(|| Error::Daemon("the chief's window was not opened again".to_owned()))?;
    if !frame["argv"].to_string().contains("\"resume\"") {
        return Err(Error::Daemon(format!(
            "the window was not opened on its thread: {}",
            frame["argv"]
        )));
    }
    wait_idle(members, log)?;
    Ok(frame)
}

/// That `answer` names every one of `words`.
fn names(answer: &str, words: &[&str], when: &str) -> Result {
    let missing: Vec<&&str> = words
        .iter()
        .filter(|word| !answer.contains(**word))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(Error::Daemon(format!(
            "{when}: the chief's answer does not name {missing:?}"
        )))
    }
}

/// The developer messages of the chief's thread that hold `word`.
fn developer_holding(home: &Path, word: &str) -> Result<usize> {
    Ok(rollout::said_by(&record(home)?, "developer")
        .iter()
        .filter(|said| said.contains(word))
        .count())
}

/// The run itself, on `home` and with the card at `card_file`.
fn check(home: &CodexHome, card_file: &Path, words: &Words, log: &Log) -> Result {
    let plan = Plan::new("codex-role", members::member(Harness::Codex))
        .with_var("CODEX_HOME", home.path().to_string_lossy())
        .with_daemon_var("CONSENSFLOW_EVAL_CHIEF_CARD", card_file.to_string_lossy());
    let members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    let ran = (|| -> Result {
        let all = [words.role.as_str(), words.own.as_str()];

        log.say("1. a thread that starts");
        let said = ask(&members, home.path(), 1, log)?;
        names(&said, &all, "the window the project opened")?;
        on_no_command_line(&members, &all, log)?;
        let began = developer_holding(home.path(), &words.role)?;
        if began != 1 || developer_holding(home.path(), &words.own)? != 1 {
            return Err(Error::Daemon(format!(
                "the thread began with the role in {began} developer messages, not one that holds the human's own instructions too"
            )));
        }

        log.say("2. the same thread, in a window opened again");
        closed_and_resumed(&members, log)?;
        let said = ask(&members, home.path(), 2, log)?;
        names(&said, &all, "the window opened again")?;
        on_no_command_line(&members, &all, log)?;
        if developer_holding(home.path(), &words.role)? != 1 {
            return Err(Error::Daemon(
                "the thread opened again was given its role a second time".to_owned(),
            ));
        }

        log.say("3. the role changed, and the thread opened again");
        files::write(card_file, card(&words.changed, true))?;
        closed_and_resumed(&members, log)?;
        let said = ask(&members, home.path(), 3, log)?;
        names(&said, &[&words.changed], "the role changed")?;
        on_no_command_line(&members, &[&words.changed, &words.role, &words.own], log)?;
        if said.contains(&words.role) {
            log.say(format!(
                "seen: the chief names the first word too ({}), beside the new one",
                words.role
            ));
        }
        let told = rollout::said_by(&record(home.path())?, "user")
            .iter()
            .filter(|said| said.contains(&words.changed))
            .count();
        log.say(format!(
            "the thread holds the new role in {told} message of ConsensFlow's, and its first instructions in {} developer message",
            developer_holding(home.path(), &words.role)?
        ));
        Ok(())
    })();
    let closed = members.close();
    ran?;
    closed
}

#[test]
#[ignore = "opt-in: spends the machine's Codex quota on three one-line turns of a chief; cargo xtask live codex-role"]
fn a_codex_window_follows_a_role_no_command_line_holds_fresh_resumed_and_changed() -> Result {
    let log = Log::begin("live codex role")?;
    let words = Words::new();
    let home = CodexHome::copying_login_of(&installed(Harness::Codex)?)?;
    home.write(
        "config.toml",
        &format!(
            "developer_instructions = \"The human's own code word is {}.\"\n",
            words.own
        ),
    )?;
    log.say(format!(
        "Codex runs on a home of the run's own, {}, holding a copy of the machine's login",
        home.path().display()
    ));
    let cards = tempfile::tempdir().map_err(|source| Error::File {
        action: "make a folder in",
        path: std::env::temp_dir(),
        source,
    })?;
    let card_file: PathBuf = cards.path().join("chief.md");
    files::write(&card_file, card(&words.role, false))?;
    let ran = check(&home, &card_file, &words, &log);
    if home.rotated() {
        log.say(
            "!!! Codex refreshed the login it was given a copy of: the machine's own Codex login \
             may be stale; run `codex login status` in a terminal",
        );
    }
    ran?;
    log.say("ok: the Codex chief followed its role, fresh, resumed and changed, and no command line held it");
    Ok(())
}
