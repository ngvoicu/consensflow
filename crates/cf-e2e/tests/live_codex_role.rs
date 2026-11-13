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
//! own (the kit's home, [`cf_e2e::live::isolation`], which fails the run if Codex
//! refreshed the login or changed the owner's setup). The chief's role is
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
//! instructions on its command line (the window's tree, and every process of the
//! machine), nor the window's board token (every process). The machine's processes are
//! read as the live checks read them on any machine ([`ThisMachine`]): `ps` on macOS and
//! Linux, Windows' process table on Windows, which has no `ps` (T-1079). Codex's server had the token
//! there (`-c shell_environment_policy.set.CONSENSFLOW_TOKEN=…`, found in T-764's
//! review): the broker gives ConsensFlow's variables to each thread in its `config`
//! now, and the run's `config.toml` holds a user policy of `inherit = "core"`, under
//! which only that gives the thread's commands the token. The chief runs a command
//! that says whether they have it (never its value), in the window the project
//! opened and in the one opened again.
//!
//! ```text
//! cargo xtask live codex-role
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::members::{
    self, descending_from, find, Identity, Machine, Members, Plan, Running, ThisMachine,
};
use cf_e2e::live::{Harness, Log};
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

/// The command the human has the chief run to find whether its commands have the board's
/// token: it says `TOKEN_IS_SET` or `TOKEN_IS_UNSET`, and never the value. Node's, which
/// every machine's shell runs: a Codex window's shell on Windows is PowerShell, where
/// `echo TOKEN_IS_$(test -n …)` is no command (T-1079).
const TOKEN_PROBE: &str =
    "node -e \"console.log('TOKEN_IS_' + (process.env.CONSENSFLOW_TOKEN ? 'SET' : 'UNSET'))\"";

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
        "# A check of a Codex window's role\n\nYour role's code word is {word}.{again} When you are asked for your code words, answer in one line with every code word your instructions give you, separated by commas, and nothing else. Run no tools unless a message asks you to run a command: then run it and reply with its output alone.\n"
    )
}

/// The processes of the tree of the window's own process `own` among `all` (the machine's),
/// itself first: what descends from it, each by the parent that started it. None where the
/// window's process has gone.
fn tree_of(all: &[Running], own: Identity) -> Vec<Running> {
    let Some(root) = all.iter().find(|process| process.identity == own) else {
        return Vec::new();
    };
    std::iter::once(root.clone())
        .chain(descending_from(all, own))
        .collect()
}

/// That no command line among the machine's processes (`all`) holds any of `words`, nor
/// `token`: the window's tree (of the process `own`, which must hold Codex's server and TUI),
/// and every other process. How many processes the window's tree holds.
fn on_no_command_line_of(
    all: &[Running],
    own: Identity,
    words: &[&str],
    token: &str,
) -> Result<usize> {
    let tree = tree_of(all, own);
    let has = |part: &str| tree.iter().any(|process| process.command.contains(part));
    if !has("app-server") || !has("--remote") {
        return Err(Error::Daemon(format!(
            "the window's tree has not got Codex's server and TUI: {} processes",
            tree.len()
        )));
    }
    for word in words {
        if let Some(process) = tree.iter().find(|process| process.command.contains(word)) {
            return Err(Error::Daemon(format!(
                "pid {} of the window's tree has {word} on its command line",
                process.identity.pid
            )));
        }
        let holding: Vec<String> = all
            .iter()
            .filter(|process| process.command.contains(word))
            .map(|process| process.identity.pid.to_string())
            .collect();
        if !holding.is_empty() {
            return Err(Error::Daemon(format!(
                "a pattern of {word} matches processes: {}",
                holding.join(" ")
            )));
        }
    }
    // Nor the board's token, which the window's environment holds.
    if all.iter().any(|process| process.command.contains(token)) {
        return Err(Error::Daemon(
            "a process has the window's board token on its command line".to_owned(),
        ));
    }
    Ok(tree.len())
}

/// That no command line holds any of `words`, nor the window's board token: the chief's
/// window's tree and every process of the machine, as [`ThisMachine`] lists them.
fn on_no_command_line(members: &Members, words: &[&str], log: &Log) -> Result {
    let window = members
        .window_of(CHIEF)?
        .ok_or_else(|| Error::Daemon("the chief has no window".to_owned()))?;
    let own = members
        .window_process(&window)
        .ok_or_else(|| Error::Daemon("the pane host named no process for the window".to_owned()))?;
    let token = window
        .open
        .as_ref()
        .and_then(|open| open["env"]["CONSENSFLOW_TOKEN"].as_str())
        .filter(|token| !token.is_empty())
        .ok_or_else(|| Error::Daemon("the window was opened with no board token".to_owned()))?
        .to_owned();
    let held = on_no_command_line_of(&ThisMachine.processes()?, own, words, &token)?;
    log.say(format!(
        "ok: none of the {held} processes of the window, nor any other, has the role or the board's token on its command line"
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

/// The human asks the chief for its code words ([`asks`]).
fn ask(members: &Members, home: &Path, round: usize, log: &Log) -> Result<String> {
    asks(
        members,
        home,
        &format!("Q{round}"),
        "What are your code words?",
        log,
    )
}

/// What the human has the chief run: whether its commands have the board's token,
/// which a user policy of `inherit = "core"` keeps only where the thread was given
/// it (the broker gives it in the thread's `config`). The value is never printed.
fn token_reaches_commands(members: &Members, home: &Path, round: usize, log: &Log) -> Result {
    let said = asks(
        members,
        home,
        &format!("C{round}"),
        &format!("Run this shell command and reply with its output alone: {TOKEN_PROBE}"),
        log,
    )?;
    if said.contains("TOKEN_IS_SET") {
        log.say("ok: the thread's commands have the board's token");
        Ok(())
    } else {
        Err(Error::Daemon(
            "the thread's commands have no board token (TOKEN_IS_SET not said)".to_owned(),
        ))
    }
}

/// The human says `words` to the chief, marked `mark`, as a paste and Enter into its
/// window, and the chief's answer is waited for in its thread's record.
fn asks(members: &Members, home: &Path, mark: &str, words: &str, log: &Log) -> Result<String> {
    wait_idle(members, log)?;
    let nonce = format!("{mark}-{}", std::process::id());
    let question = format!("({nonce}) {words}");
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

/// The run itself, Codex's home of the run's own holding `config` as its `config.toml`,
/// with the card at `card_file`.
fn check(config: &str, card_file: &Path, words: &Words, log: &Log) -> Result {
    let plan = Plan::new("codex-role", members::member(Harness::Codex))
        .with_home_file(Harness::Codex, "CODEX_HOME", "config.toml", config)
        .with_daemon_var("CONSENSFLOW_EVAL_CHIEF_CARD", card_file.to_string_lossy());
    let members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    let codex_home = members.codex_home();
    let home = codex_home.as_path();
    let ran = (|| -> Result {
        let all = [words.role.as_str(), words.own.as_str()];

        log.say("1. a thread that starts");
        let said = ask(&members, home, 1, log)?;
        names(&said, &all, "the window the project opened")?;
        on_no_command_line(&members, &all, log)?;
        token_reaches_commands(&members, home, 1, log)?;
        let began = developer_holding(home, &words.role)?;
        if began != 1 || developer_holding(home, &words.own)? != 1 {
            return Err(Error::Daemon(format!(
                "the thread began with the role in {began} developer messages, not one that holds the human's own instructions too"
            )));
        }

        log.say("2. the same thread, in a window opened again");
        closed_and_resumed(&members, log)?;
        let said = ask(&members, home, 2, log)?;
        names(&said, &all, "the window opened again")?;
        on_no_command_line(&members, &all, log)?;
        token_reaches_commands(&members, home, 2, log)?;
        if developer_holding(home, &words.role)? != 1 {
            return Err(Error::Daemon(
                "the thread opened again was given its role a second time".to_owned(),
            ));
        }

        log.say("3. the role changed, and the thread opened again");
        files::write(card_file, card(&words.changed, true))?;
        closed_and_resumed(&members, log)?;
        let said = ask(&members, home, 3, log)?;
        names(&said, &[&words.changed], "the role changed")?;
        on_no_command_line(&members, &[&words.changed, &words.role, &words.own], log)?;
        if said.contains(&words.role) {
            log.say(format!(
                "seen: the chief names the first word too ({}), beside the new one",
                words.role
            ));
        }
        let told = rollout::said_by(&record(home)?, "user")
            .iter()
            .filter(|said| said.contains(&words.changed))
            .count();
        log.say(format!(
            "the thread holds the new role in {told} message of ConsensFlow's, and its first instructions in {} developer message",
            developer_holding(home, &words.role)?
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
    let config = format!(
        "developer_instructions = \"The human's own code word is {}.\"\n\n[shell_environment_policy]\ninherit = \"core\"\n",
        words.own
    );
    log.say("Codex runs on a home of the run's own, holding a copy of the machine's login");
    let cards = cf_e2e::door::made("live-codex-role-").map_err(|source| Error::File {
        action: "make a folder in",
        path: std::env::temp_dir(),
        source,
    })?;
    let card_file: PathBuf = cards.path().join("chief.md");
    files::write(&card_file, card(&words.role, false))?;
    check(&config, &card_file, &words, &log)?;
    log.say("ok: the Codex chief followed its role, fresh, resumed and changed, and no command line held it");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(pid: u32, ppid: u32, command: &str) -> Running {
        Running {
            identity: Identity {
                pid,
                started: 1_000 + u64::from(pid),
            },
            ppid,
            command: command.to_owned(),
            exe: None,
        }
    }

    /// The machine as zeewin2's process table lists it (Windows has no `ps`): the window's
    /// process, Codex's server and TUI under it, and processes that are none of the window's.
    fn windows_machine() -> (Vec<Running>, Identity) {
        let all = vec![
            running(4, 0, ""),
            running(100, 4, r#""C:\Windows\System32\conhost.exe" 0x4"#),
            running(
                2000,
                100,
                r#""C:\Program Files\nodejs\node.exe" C:\codex\bin\codex.js"#,
            ),
            running(
                2100,
                2000,
                r#""C:\codex\codex.exe" app-server --listen ws://127.0.0.1:50123"#,
            ),
            running(
                2200,
                2000,
                r#""C:\codex\codex.exe" --remote ws://127.0.0.1:50123 --no-alt-screen"#,
            ),
            running(3000, 4, r#""C:\Windows\System32\notepad.exe" C:\notes.txt"#),
        ];
        let own = all[2].identity;
        (all, own)
    }

    #[test]
    fn the_window_s_tree_is_what_descends_from_its_process_in_a_windows_table_as_in_ps() {
        let (all, own) = windows_machine();
        let pids: Vec<u32> = tree_of(&all, own)
            .iter()
            .map(|process| process.identity.pid)
            .collect();
        assert_eq!(pids, [2000, 2100, 2200]);
        // A window whose process has gone has no tree, whatever has its number since.
        let gone = Identity {
            pid: 2000,
            started: 7,
        };
        assert!(tree_of(&all, gone).is_empty());
    }

    #[test]
    fn nothing_is_found_where_no_command_line_holds_a_word_or_the_token() {
        let (all, own) = windows_machine();
        assert_eq!(
            on_no_command_line_of(&all, own, &["ROLEALPHA7", "OWNERWORD7"], "tok-123").unwrap(),
            3
        );
    }

    #[test]
    fn a_word_on_the_command_line_of_the_windows_tree_or_of_any_other_process_is_found() {
        let (mut all, own) = windows_machine();
        all[4]
            .command
            .push_str(" -c developer_instructions=ROLEALPHA7");
        let said = on_no_command_line_of(&all, own, &["ROLEALPHA7"], "tok").unwrap_err();
        assert!(
            said.to_string()
                .contains("pid 2200 of the window's tree has ROLEALPHA7"),
            "{said}"
        );
        // Another process of the machine: not in the tree, and found all the same.
        let (mut all, own) = windows_machine();
        all[5].command.push_str(" ROLEALPHA7");
        let said = on_no_command_line_of(&all, own, &["ROLEALPHA7"], "tok").unwrap_err();
        assert!(
            said.to_string()
                .contains("a pattern of ROLEALPHA7 matches processes: 3000"),
            "{said}"
        );
    }

    #[test]
    fn the_board_token_on_any_command_line_of_the_machine_is_found() {
        let (mut all, own) = windows_machine();
        all[5].command.push_str(" CONSENSFLOW_TOKEN=tok-123");
        let said = on_no_command_line_of(&all, own, &["ROLEALPHA7"], "tok-123").unwrap_err();
        assert!(
            said.to_string().contains("board token on its command line"),
            "{said}"
        );
    }

    #[test]
    fn a_tree_without_codex_s_server_and_tui_is_not_the_window_s() {
        let (all, own) = windows_machine();
        let only_the_server: Vec<Running> = all
            .into_iter()
            .filter(|process| process.identity.pid != 2200)
            .collect();
        let said =
            on_no_command_line_of(&only_the_server, own, &["ROLEALPHA7"], "tok").unwrap_err();
        assert!(
            said.to_string()
                .contains("has not got Codex's server and TUI: 2 processes"),
            "{said}"
        );
    }

    #[test]
    fn the_command_that_asks_for_the_token_is_one_the_shell_of_every_machine_runs() {
        // Windows' shell for a Codex window is PowerShell, where `$(test -n …)` runs no
        // command (T-1079): Node's one-liner is the same on every machine.
        assert!(TOKEN_PROBE.starts_with("node -e "), "{TOKEN_PROBE}");
        for unix_only in ["$(", "test -n", "&&", "||", "echo"] {
            assert!(
                !TOKEN_PROBE.contains(unix_only),
                "{unix_only}: {TOKEN_PROBE}"
            );
        }
        assert!(TOKEN_PROBE.contains("CONSENSFLOW_TOKEN"));
        // The answer is said, and the command's own words are no answer.
        assert!(!TOKEN_PROBE.contains("TOKEN_IS_SET"));
        assert!(!TOKEN_PROBE.contains("TOKEN_IS_UNSET"));
    }
}
