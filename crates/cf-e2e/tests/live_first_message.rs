//! Live: a worker's window is given its first message once it is up, on every harness,
//! and no command line holds it (T-432). It is opt-in (`cargo xtask live
//! first-message`): it spends the machine's quota on two short tasks of each harness
//! named, and needs each installed and logged in, so it is ignored unless asked for and
//! is no part of `check` or of CI.
//!
//! The field bug (2026-10-24, ConsensFlow 3.0.5): a worker sent SIGINT to every process
//! whose `ps` line matched "xtask plants", and two other workers' Claude windows closed.
//! Claude Code's, Pi's and Codex's windows were started with their task's brief as the
//! last word of their command line, which every process on the Mac reads (`/bin/ps` is
//! set-uid root) and every kill or wait by pattern matches. Their windows now open with
//! nothing, and are delivered the brief once up: Claude Code's pasted once it is past the
//! question of trusting its folder (its record wraps the paste, as it wraps every later
//! message), Pi's through its extension's inbox, Codex's through the broker of its
//! supervisor. Devin reads its own from a prompt file, and OpenCode is posted its own.
//!
//! The native daemon and the real pane host run in a home of the run's own; the chief is
//! the stand-in, and the check plays the chief with its token. Each harness is the one
//! worker of a project of its own, in a folder made a new git repository nobody trusted:
//! Claude Code asks whether to trust it, and ConsensFlow answers before anything is
//! pasted. Each worker is given
//!
//! 1. `fresh`: a brief of about 16,000 characters, filler around one ask, in a window
//!    opened afresh (on a Claude config made for the run, its notices and dialogs
//!    included, where the run is given one: `--claude-config`);
//! 2. `resumed`: once that task is done and its window closed, a follow-up (`cf task
//!    reopen`), which opens a window on the worker's conversation.
//!
//! While each task runs, every second: no word of the command lines the daemon opened the
//! worker's windows with holds the message's word, and `pgrep -f` on that word finds no
//! process on the machine. Once it is done: its result reached the chief and holds the
//! word, and the window's record has the message once, as a user's message (the brief the
//! first of the conversation), whole.
//!
//! ```text
//! cargo xtask live first-message                       all five, one after the other
//! cargo xtask live first-message claude                one harness
//! cargo xtask live first-message claude --claude-config <folder>
//!                                                      Claude on a config of the run's own
//! cargo xtask live first-message pi --pi-agent-dir <folder>
//!                                                      Pi on an agent folder of the run's own
//! ```
//!
//! Codex reads its login from `CODEX_HOME`, which the run's caller points at a home of
//! the run's own.

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::members::{
    self, delivered_result, find, is_over, Account, Machine, Members, Plan, ThisMachine,
};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, agents, between, Harness, Log, Who};
use cf_e2e::{Error, Result};
use serde_json::json;

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the table says each worker was checked for.
const CHECKED: &str = "first message given once up, on no command line, fresh and resumed";

/// How long a task has to be done, its result to reach the chief, and the worker's
/// window to close once it is.
const WORK: Duration = Duration::from_secs(300);
const DELIVERS: Duration = Duration::from_secs(60);
const CLOSES: Duration = Duration::from_secs(60);

/// How often the board, the command lines and the machine's processes are looked at.
const LOOK: Duration = Duration::from_secs(1);

/// About how long the fresh brief is, in characters: more than a hook's 10,000, which
/// Claude Code would have put in a file of its own.
const LONG: usize = 16_000;

/// The harnesses the words of the command line name, or all five.
fn workers(named: &[String]) -> Result<Vec<Harness>> {
    if named.is_empty() {
        return Ok(vec![
            Harness::Claude,
            Harness::Codex,
            Harness::Pi,
            Harness::OpenCode,
            Harness::Devin,
        ]);
    }
    named
        .iter()
        .map(|word| match Who::named(word, "worker")? {
            Who::Harness(harness) => Ok(harness),
            Who::StandIn => Err(Error::Daemon(
                "the stand-in is no harness: the rig's regression covers it".to_owned(),
            )),
        })
        .collect()
}

/// A word that is in one message and nowhere else on the machine.
fn word(harness: Harness, step: &str) -> String {
    format!(
        "FM{}{}{}",
        harness.name().to_ascii_uppercase(),
        step.to_ascii_uppercase(),
        std::process::id()
    )
}

/// The fresh brief: about [`LONG`] characters of filler, and the one ask at its end.
fn long_brief(word: &str) -> String {
    let mut brief = String::from(
        "This brief is long on purpose: the lines below are filler to read past, and the last line is the task.\n",
    );
    let mut line = 1;
    while brief.len() < LONG {
        brief.push_str(&format!(
            "Line {line:04}: filler of a long brief, to read past; it asks nothing.\n"
        ));
        line += 1;
    }
    brief.push_str(&format!(
        "Reply with exactly the word {word} and nothing else."
    ));
    brief
}

/// The follow-up of the resumed step.
fn follow_up(word: &str) -> String {
    format!("Reply with exactly the word {word} and nothing else.")
}

/// The text of a record's item without the wrapper Claude Code puts a paste in
/// (`<pasted_content id="…">` on a line of its own, and the same closed at the end).
fn unwrapped(text: &str) -> &str {
    let text = text.trim();
    let Some(rest) = text.strip_prefix("<pasted_content") else {
        return text;
    };
    let Some((_, inner)) = rest.split_once(">\n") else {
        return text;
    };
    inner
        .rfind("</pasted_content")
        .map_or(inner, |end| inner[..end].trim_end())
}

/// How long a message's body may be before ConsensFlow cuts what it pastes
/// (`delivery_text`'s `OPENING`, in UTF-16 units): the rest is read with `cf inbox read`.
const OPENING: usize = 15_000;

/// The body of the message `id` as ConsensFlow gives it to a window: whole, or its
/// opening and where to read all of it, as it was given on a command line too.
fn given_as(message: &str, id: i64) -> String {
    let length = message.encode_utf16().count();
    if length <= OPENING {
        return message.to_owned();
    }
    let mut units = 0;
    let opening: String = message
        .chars()
        .take_while(|character| {
            units += character.len_utf16();
            units <= OPENING
        })
        .collect();
    format!("{opening}\n… ({length} characters; read all of it with: cf inbox read m-{id})")
}

/// What one task of a worker showed.
#[derive(Debug, Default)]
struct Seen {
    state: String,
    /// The command-line words, of the windows the daemon opened, that held the word.
    on_command_lines: Vec<String>,
    /// The processes `pgrep -f` found by the word: their number and name.
    matched: Vec<String>,
    looks: usize,
    /// The last lines of the worker's window at the last look it had one.
    screen: String,
    /// Its result, holding the word, was delivered to the chief.
    delivered: bool,
}

/// The processes of the machine whose command line holds `word`, each as its number and
/// command line: none where none does. Read from the machine's process table as every
/// live check reads it (`ThisMachine`: `ps` on macOS and Linux, Windows' own table there),
/// not `pgrep`, which Windows has not: there the check said `(pgrep could not be run)` for
/// every word (integration B round 2, T-800).
fn matching(word: &str) -> Vec<String> {
    let Ok(processes) = ThisMachine.processes() else {
        return vec!["(the machine's processes could not be read)".to_owned()];
    };
    let own = std::process::id();
    processes
        .into_iter()
        .filter(|process| process.identity.pid != own && process.command.contains(word))
        .map(|process| {
            format!(
                "{} {}",
                process.identity.pid,
                live::cut(&process.command, 80)
            )
        })
        .collect()
}

/// Task `number` of `agent` looked at once a second until it is over: the command lines
/// of the worker's windows and the machine's processes held to `word` at each look; then,
/// a task done, its result waited for at the chief.
fn watch(members: &Members, agent: &str, number: i64, word: &str, log: &Log) -> Result<Seen> {
    let mut seen = Seen::default();
    let mut said = String::new();
    find(log, &format!("T-{number} to end"), WORK, LOOK, || {
        let thread = members.task(number)?;
        seen.state = live::text(&thread["state"]);
        seen.looks += 1;
        // How the worker's window is getting on, said as it changes.
        let lane = members.lane(agent)?;
        let activity = format!(
            "T-{number} is {}; the worker's window is {}{}",
            seen.state,
            lane["activity"]["state"].as_str().unwrap_or("not there"),
            lane["activity"]["reason"]
                .as_str()
                .map_or_else(String::new, |reason| format!(" ({reason})"))
        );
        if activity != said {
            log.say(&activity);
            said = activity;
        }
        if let Some(pane) = lane["pane"]["id"].as_str() {
            seen.screen = members.tail(pane);
        }
        for frame in members.windows_opened_for(agent) {
            for arg in frame["argv"].as_array().into_iter().flatten() {
                let arg = arg.as_str().unwrap_or_default();
                if arg.contains(word) && !seen.on_command_lines.iter().any(|held| held == arg) {
                    seen.on_command_lines.push(live::cut(arg, 80));
                }
            }
        }
        for process in matching(word) {
            if !seen.matched.contains(&process) {
                seen.matched.push(process);
            }
        }
        Ok(is_over(&thread).then_some(()))
    })?;
    if seen.state == "done" {
        seen.delivered = find(log, "the result", DELIVERS, LOOK, || {
            let inbox = members.inbox(Some(members.chief()))?;
            Ok(inbox
                .iter()
                .filter(|message| message["kind"] == "result")
                .find(|message| {
                    message["state"] == "delivered"
                        && message["body"]
                            .as_str()
                            .is_some_and(|body| body.contains(word))
                })
                .map(|_| ()))
        })?
        .is_some();
    }
    Ok(seen)
}

/// The window's record of task `number`, as the page's drawer reads it: each item's
/// role and text.
fn record(members: &Members, number: i64) -> Result<Vec<(String, String)>> {
    let read = members.page("task.transcript", json!({ "task": number }))?;
    Ok(read["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| (live::text(&item["role"]), live::text(&item["text"])))
                .collect()
        })
        .unwrap_or_default())
}

/// The id and the body of the message whose text holds `word`, among task `number`'s:
/// the body as the ledger keeps it (a tier's preamble, `Critical work: …`, included).
fn message_with(members: &Members, number: i64, word: &str) -> Result<Option<(i64, String)>> {
    let thread = members.task(number)?;
    Ok(thread["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|message| {
            message["body"]
                .as_str()
                .is_some_and(|body| body.contains(word))
        })
        .and_then(|message| Some((message["id"].as_i64()?, live::text(&message["body"])))))
}

/// One step held to what the owner should have seen: the task done, its result at the
/// chief with the word, the word on no command line and in no process, and the record
/// holding the message once, whole, as a user's message; `first` where it is the first
/// user message of the conversation.
fn judge(
    account: &mut Account,
    step: &str,
    members: &Members,
    number: i64,
    seen: &Seen,
    word: &str,
    first: bool,
) -> Result {
    account.say(format!(
        "{step}: T-{number} is {} after {} looks",
        seen.state, seen.looks
    ));
    account.check(
        &format!("{step}: the task is done"),
        seen.state == "done",
        &seen.state,
    );
    if seen.state != "done" {
        account.say(format!(
            "{step}: the worker's window last showed: {}",
            seen.screen
        ));
    }
    account.check(
        &format!("{step}: the result reached the chief with the word"),
        seen.delivered,
        delivered_result(&members.inbox(Some(members.chief()))?)
            .map_or("none".to_owned(), |body| live::cut(&body, 80)),
    );
    account.check(
        &format!("{step}: no command line held the message"),
        seen.on_command_lines.is_empty(),
        format!("{:?}", seen.on_command_lines),
    );
    account.check(
        &format!("{step}: no process matched a pattern of its words"),
        seen.matched.is_empty(),
        format!("{:?}", seen.matched),
    );
    let stored = message_with(members, number, word)?;
    let marker = stored
        .as_ref()
        .map(|(id, _)| format!("[ConsensFlow m-{id} "));
    let items = record(members, number)?;
    let users: Vec<&str> = items
        .iter()
        .filter(|(role, _)| role == "user")
        .map(|(_, text)| unwrapped(text))
        .collect();
    let given: Vec<&&str> = users
        .iter()
        .filter(|text| {
            marker
                .as_deref()
                .is_some_and(|marker| text.contains(marker))
        })
        .collect();
    account.check(
        &format!("{step}: the record has the message once"),
        given.len() == 1,
        format!("{} of {} user messages", given.len(), users.len()),
    );
    let expected = stored.as_ref().map(|(id, body)| given_as(body, *id));
    let whole = given.first().is_some_and(|text| {
        marker
            .as_deref()
            .is_some_and(|marker| text.contains(marker))
            && expected
                .as_deref()
                .is_some_and(|expected| text.contains(expected))
    });
    account.check(
        &format!("{step}: whole, its header and every line"),
        whole,
        given.first().map_or(0, |text| text.len()),
    );
    if first {
        let opens = users.first().is_some_and(|text| {
            marker
                .as_deref()
                .is_some_and(|marker| text.starts_with(marker))
        });
        account.check(
            &format!("{step}: the first user message of the conversation"),
            opens,
            users
                .first()
                .map_or("none".to_owned(), |text| live::cut(text, 60)),
        );
    }
    Ok(())
}

/// The worker of `harness`, given a long brief afresh and then a follow-up: what it
/// came to.
fn run(harness: Harness, log: &Log) -> Result<Account> {
    let log = log.scoped(harness.name());
    let plan = Plan::new(
        &format!("first-message-{}", harness.name()),
        members::stand_in_chief(),
    )
    .with(members::member(harness))
    .in_a_new_repository();
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), &log)?;
    let account = steps(&mut members, harness, &log);
    let closed = members.close();
    let mut account = account?;
    account.closed(closed);
    Ok(account)
}

fn steps(members: &mut Members, harness: Harness, log: &Log) -> Result<Account> {
    let mut account = Account::new();
    let agent = agents::id(harness);
    members.add(&agent, &[])?;

    let fresh = word(harness, "fresh");
    let brief = long_brief(&fresh);
    let number = members.give(&agent, &brief)?;
    log.say(format!(
        "T-{number} is given: a brief of {} characters",
        brief.len()
    ));
    let seen = watch(members, &agent, number, &fresh, log)?;
    judge(&mut account, "fresh", members, number, &seen, &fresh, true)?;
    // Claude Code asks whether to trust a new repository before it starts, and the brief
    // is pasted only once ConsensFlow has answered (it says so to the human once).
    let answered = members
        .inbox(None)?
        .iter()
        .filter_map(|message| message["body"].as_str())
        .any(|body| body.contains("asked whether to trust") && body.contains("answered yes"));
    if harness == Harness::Claude {
        account.check(
            "fresh: the window asked whether to trust the new repository, and was answered first",
            answered,
            answered,
        );
    } else {
        account.say(format!(
            "fresh: the window asked whether to trust the folder: {answered}"
        ));
    }

    // The task done, its window closes: the follow-up opens one on its conversation.
    find(log, "the worker's window to close", CLOSES, LOOK, || {
        Ok(members.window_of(&agent)?.is_none().then_some(()))
    })?;
    let opened = members.windows_opened_for(&agent).len();
    let resumed = word(harness, "resumed");
    let again = follow_up(&resumed);
    let reopened = members.as_chief(["task", "reopen", &format!("T-{number}"), &again])?;
    account.check(
        "resumed: the task reopens",
        reopened.code == Some(0),
        &reopened,
    );
    let seen = watch(members, &agent, number, &resumed, log)?;
    account.say(format!(
        "resumed: {} windows opened for the worker, {opened} before the follow-up",
        members.windows_opened_for(&agent).len()
    ));
    judge(
        &mut account,
        "resumed",
        members,
        number,
        &seen,
        &resumed,
        false,
    )?;
    Ok(account)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on two short tasks of each harness named; cargo xtask live first-message"]
fn every_harness_is_given_its_first_message_once_up_and_no_command_line_holds_it() -> Result {
    let log = Log::begin("live first-message")?;
    let mut checks = Vec::new();
    for harness in workers(&live::list("harness"))? {
        let who = Who::Harness(harness);
        let before = who.version();
        let account = run(harness, &log).unwrap_or_else(|failure| {
            let mut account = Account::new();
            account.fail(&failure);
            account
        });
        let version = between(before, who.version());
        log.block(
            &format!(
                "== {} {version}: {}",
                harness.name(),
                if account.held() { "ok" } else { "FAILED" }
            ),
            &account.text(),
        );
        checks.push(Check::new(harness.name(), CHECKED, account.held(), version));
    }
    log.report("what each worker came to", &report::table(&checks));
    report::verdict(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_harness_named_all_five_run_and_those_named_run_in_the_order_given() {
        assert_eq!(workers(&[]).unwrap().len(), 5);
        assert_eq!(
            workers(&["pi".to_owned(), "claude".to_owned()]).unwrap(),
            [Harness::Pi, Harness::Claude]
        );
        assert!(workers(&["fake".to_owned()]).is_err());
        assert!(workers(&["nobody".to_owned()]).is_err());
    }

    #[test]
    fn the_fresh_brief_is_longer_than_a_hook_may_carry_and_ends_in_its_one_ask() {
        let brief = long_brief("WORD");
        assert!(
            brief.len() >= LONG && brief.len() < LONG + 200,
            "{}",
            brief.len()
        );
        assert!(brief.ends_with("Reply with exactly the word WORD and nothing else."));
        assert_eq!(brief.matches("WORD").count(), 1);
    }

    #[test]
    fn a_paste_claude_code_wrapped_is_read_as_it_was_given_and_any_other_text_as_it_is() {
        let given = "[ConsensFlow m-7 · T-1 · task from @chief]\nDo it";
        let wrapped =
            format!("\n\n<pasted_content id=\"b118\">\n{given}\n</pasted_content id=\"b118\">\n");
        assert_eq!(unwrapped(&wrapped), given);
        assert_eq!(unwrapped(given), given);
        assert_eq!(unwrapped("  plain  "), "plain");
    }

    #[test]
    fn a_long_message_is_given_as_its_opening_and_where_to_read_all_of_it() {
        assert_eq!(given_as("short", 4), "short");
        let long = "x".repeat(OPENING + 5);
        let given = given_as(&long, 4);
        assert!(given.starts_with(&"x".repeat(OPENING)));
        assert!(given.ends_with(&format!(
            "\n… ({} characters; read all of it with: cf inbox read m-4)",
            OPENING + 5
        )));
        assert!(!given.contains(&"x".repeat(OPENING + 1)));
    }

    #[test]
    fn each_word_names_its_harness_and_step() {
        let fresh = word(Harness::Pi, "fresh");
        assert!(fresh.starts_with("FMPIFRESH"), "{fresh}");
        assert_ne!(fresh, word(Harness::Pi, "resumed"));
    }

    #[test]
    fn a_word_no_process_holds_matches_none() {
        assert!(matching("FMNOPROCESSHOLDSTHIS0").is_empty());
    }
}
