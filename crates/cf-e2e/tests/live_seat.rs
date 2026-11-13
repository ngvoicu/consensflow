//! Live: each harness sits in the worker's seat of one rig and takes one short task (T-1136, the
//! live matrix's prerequisite LMX-06). It is opt-in (`cargo xtask live seat`): it spends the
//! machine's quota on one short task of each harness named (all five by default), and needs each
//! installed and logged in, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! The rig is one: the chief is the stand-in, the staff's roster holds every harness named, and so
//! the rig makes a home of the run's own for every one of them at once, with the variables each
//! sets for the whole rig (Devin on Windows moves `USERPROFILE`, `APPDATA` and `LOCALAPPDATA`), and
//! a harness's variables that took another's away would show here. A harness is added to the staff,
//! given its task, and taken out of the staff when its task is over, so each task lands on the
//! harness meant whatever its tier.
//!
//! The task asks for a command the shell of every harness runs, Node's (Claude Code and Pi run
//! commands in Git Bash on Windows; Codex, Devin and Muse in PowerShell), and for a reply that holds
//! the seat's own word and the command's output (`42`). What each seat is held to:
//! - the task is done and its result, with the word and the output, reached the chief;
//! - **its record was found**: the harness's own record holds the brief once, whole, as a user's
//!   message, the first of the conversation, as the page's drawer reads it (`task.transcript`), and
//!   something after it that ran the command;
//! - the owner's files are as they were (the rig's close fails the run otherwise).
//!
//! Besides, each seat's time (from the task given to its result at the chief), and Muse's allowance
//! before and after (`muse usage --json` on a copy of the login in a folder of the run's own, never
//! on the owner's config). Claude Code's, Codex's, Pi's and Devin's cannot be read by the kit yet
//! (LMX-19): the run says so.
//!
//! ```text
//! cargo xtask live seat                        every harness, one after the other
//! cargo xtask live seat muse devin             the ones named
//! cargo xtask windows --machine zeewin2 -- cargo xtask live seat
//! ```

use std::path::Path;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{self, delivered_result, find, is_over, Account, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, agents, between, Harness, Log, Who};
use cf_e2e::{muse, Error, Result};
use serde_json::json;

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the table says each seat was checked for.
const CHECKED: &str =
    "a window opened in the rig, one task taken, its record found, the result returned";

/// The command every harness's shell runs, and what it prints: no quote, no space, no shell's own
/// syntax.
const COMMAND: &str = "node -p 40+2";
const OUTPUT: &str = "42";

/// How long a task has to be done, its result to reach the chief, and the window to close.
const WORK: Duration = Duration::from_secs(300);
const DELIVERS: Duration = Duration::from_secs(60);
const CLOSES: Duration = Duration::from_secs(60);

/// How often the board is looked at.
const LOOK: Duration = Duration::from_secs(1);

/// About how long one seat takes, which the logins of the run must last, with their margin, for
/// each seat named ([`Log::lasting`]): the seats took 7 to 28 s each (zeewin2 and a Mac, T-1136), so
/// two minutes is generous, and the limits above end a seat that takes longer. (A login that has 29
/// minutes left cannot be renewed by using it, so a plan of six minutes a seat refused five seats.)
const PER_SEAT: Duration = Duration::from_secs(2 * 60);

/// The harnesses the words of the command line name, or every one.
fn seats(named: &[String]) -> Result<Vec<Harness>> {
    if named.is_empty() {
        return Ok(Harness::ALL.to_vec());
    }
    named
        .iter()
        .map(|word| match Who::named(word, "seat")? {
            Who::Harness(harness) => Ok(harness),
            Who::StandIn => Err(Error::Daemon(
                "the stand-in is no harness: the rig's regression covers it".to_owned(),
            )),
        })
        .collect()
}

/// A word that is in one message and nowhere else on the machine.
fn word(harness: Harness) -> String {
    format!(
        "SEAT{}{}",
        harness.name().to_ascii_uppercase(),
        std::process::id()
    )
}

/// The task: run the command, and reply with the word and the command's output.
fn brief(word: &str) -> String {
    format!(
        "Run this shell command exactly as written, and nothing else: {COMMAND}\nThen reply with exactly the word {word}, a space, and the output of the command, and nothing else."
    )
}

/// What a reply holds when the seat did it: the word and the output.
fn replied(body: &str, word: &str) -> bool {
    body.contains(word) && body.contains(OUTPUT)
}

/// The text of a record's item without the wrapper Claude Code puts a paste in.
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

/// The window's record of task `number`, as the page's drawer reads it: each item's role and text.
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

/// The id and the body of the message of task `number` whose text holds `word`.
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

/// Whether the record shows the command ran: an item before the last, not a user's and not a
/// whole context (a few hundred characters at most), that holds the command (Claude Code's and Pi's
/// call, Codex's) or its output on a token of its own (Devin's result: `Output from command in
/// shell …: 42 Exit code: 0`). The last item is the reply, which holds the output as the task asked.
fn ran_the_command(items: &[(String, String)]) -> bool {
    let Some((_, before)) = items.split_last() else {
        return false;
    };
    before
        .iter()
        .filter(|(role, text)| role != "user" && text.len() < 2_000)
        .any(|(_, text)| {
            text.contains("40+2")
                || text
                    .split(|character: char| !character.is_alphanumeric())
                    .any(|token| token == OUTPUT)
        })
}

/// Muse's allowance now, on a copy of its login in a folder of the run's own: `window 4%, week
/// 51%`, or why it cannot be read. The owner's config is read for the copy and never run.
fn muse_allowance() -> String {
    let read = || -> Result<String> {
        let install = muse::installed().map_err(Error::Daemon)?;
        let home = muse::real_home().map_err(Error::Daemon)?;
        let folder = cf_e2e::door::made("live-seat-").map_err(|source| Error::File {
            action: "make a folder in",
            path: std::env::temp_dir(),
            source,
        })?;
        muse::seed_live_home(folder.path(), &home)?;
        let said = muse::usage(&install, &home, &folder.path().join(".config"))?;
        Ok(match muse::used_percent(&said) {
            Some((window, week)) => format!("window {window}%, week {week}%"),
            None => format!("no allowance ({})", said["reason"]),
        })
    };
    read().unwrap_or_else(|why| format!("unreadable ({why})"))
}

/// What one seat showed.
#[derive(Debug, Default)]
struct Seen {
    state: String,
    looks: usize,
    /// The last lines of the worker's window at the last look it had one.
    screen: String,
    /// Its result, holding the word and the output, was delivered to the chief.
    delivered: bool,
    /// The task given to the result at the chief.
    took: Duration,
}

/// Task `number` of `agent` looked at every second until it is over, then its result waited for at
/// the chief.
fn watch(members: &Members, agent: &str, number: i64, word: &str, log: &Log) -> Result<Seen> {
    let begun = Instant::now();
    let mut seen = Seen::default();
    let mut said = String::new();
    find(log, &format!("T-{number} to end"), WORK, LOOK, || {
        let thread = members.task(number)?;
        seen.state = live::text(&thread["state"]);
        seen.looks += 1;
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
                            .is_some_and(|body| replied(body, word))
                })
                .map(|_| ()))
        })?
        .is_some();
    }
    seen.took = begun.elapsed();
    Ok(seen)
}

/// One seat held to what it should have shown.
fn judge(account: &mut Account, members: &Members, number: i64, seen: &Seen, word: &str) -> Result {
    account.say(format!(
        "T-{number} is {} after {} looks, {} s from the task given",
        seen.state,
        seen.looks,
        seen.took.as_secs()
    ));
    account.check("the task is done", seen.state == "done", &seen.state);
    if seen.state != "done" {
        account.say(format!("the worker's window last showed: {}", seen.screen));
    }
    account.check(
        "the result reached the chief with the word and the command's output",
        seen.delivered,
        delivered_result(&members.inbox(Some(members.chief()))?)
            .map_or("none".to_owned(), |body| live::cut(&body, 80)),
    );
    let stored = message_with(members, number, word)?;
    let marker = stored
        .as_ref()
        .map(|(id, _)| format!("[ConsensFlow m-{id} "));
    let items = record(members, number)?;
    account.say(format!(
        "the record's items: {}",
        items
            .iter()
            .map(|(role, text)| format!("{role}({})", text.len()))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    for (role, text) in items
        .iter()
        .filter(|(role, text)| role != "user" && text.len() < 400)
    {
        account.say(format!("  {role}: {}", live::cut(text, 90)));
    }
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
        "its record was found, and holds the brief once",
        given.len() == 1,
        format!("{} of {} user messages", given.len(), users.len()),
    );
    let whole = given.first().is_some_and(|text| {
        text.contains(COMMAND) && text.contains(word) && text.contains("[ConsensFlow m-")
    });
    account.check(
        "the brief is whole in it, as the harness took it",
        whole,
        given.first().map_or(0, |text| text.len()),
    );
    let opens = users.first().is_some_and(|text| {
        marker
            .as_deref()
            .is_some_and(|marker| text.starts_with(marker))
    });
    account.check(
        "it is the first user message of the conversation",
        opens,
        users
            .first()
            .map_or("none".to_owned(), |text| live::cut(text, 60)),
    );
    account.check(
        "the record holds the command the seat ran",
        ran_the_command(&items),
        format!("{} items after the brief", items.len().saturating_sub(1)),
    );
    Ok(())
}

/// Every seat named, one after the other in the one rig: what each came to.
fn run(harnesses: &[Harness], log: &Log) -> Result<Vec<(Harness, Account, Duration)>> {
    let mut plan = Plan::new("seat", members::stand_in_chief()).in_a_new_repository();
    for harness in harnesses {
        plan = plan.with(members::member(*harness));
    }
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    let mut ran = Vec::new();
    for harness in harnesses {
        let log = log.scoped(harness.name());
        let account = seat(&mut members, *harness, &log);
        let (account, took) = account.unwrap_or_else(|failure| {
            let mut account = Account::new();
            account.fail(&failure);
            (account, Duration::ZERO)
        });
        ran.push((*harness, account, took));
    }
    let closed = members.close();
    if let Some((_, account, _)) = ran.last_mut() {
        account.closed(closed);
    }
    Ok(ran)
}

/// One harness in the worker's seat: added to the staff, given its task, watched, judged, and
/// taken out of the staff when its window is closed.
fn seat(members: &mut Members, harness: Harness, log: &Log) -> Result<(Account, Duration)> {
    let mut account = Account::new();
    let agent = agents::id(harness);
    let allowance = (harness == Harness::Muse).then(muse_allowance);
    if let Some(before) = &allowance {
        account.say(format!("Muse's allowance before: {before}"));
    } else {
        account.say(format!(
            "{}'s allowance before: no reader yet (LMX-19)",
            harness.name()
        ));
    }
    members.add(&agent, &[])?;
    let word = word(harness);
    let number = members.give(&agent, &brief(&word))?;
    log.say(format!("T-{number} is given to {agent}"));
    let seen = watch(members, &agent, number, &word, log)?;
    judge(&mut account, members, number, &seen, &word)?;
    find(log, "the worker's window to close", CLOSES, LOOK, || {
        Ok(members.window_of(&agent)?.is_none().then_some(()))
    })?;
    members.page("member.remove", json!({ "agent": agent }))?;
    if allowance.is_some() {
        account.say(format!("Muse's allowance after: {}", muse_allowance()));
    } else {
        account.say(format!(
            "{}'s allowance after: no reader yet (LMX-19)",
            harness.name()
        ));
    }
    Ok((account, seen.took))
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short task of each harness named; cargo xtask live seat"]
fn every_harness_sits_in_a_seat_of_one_rig_takes_a_short_task_and_returns_its_result() -> Result {
    let named = live::list("harness");
    let harnesses = seats(&named)?;
    let seats_asked = u32::try_from(harnesses.len()).unwrap_or(5);
    let log = Log::begin("live seat")?.lasting(PER_SEAT * seats_asked);
    let versions: Vec<(Harness, String)> = harnesses
        .iter()
        .map(|harness| (*harness, Who::Harness(*harness).version()))
        .collect();
    let ran = run(&harnesses, &log)?;
    let mut checks = Vec::new();
    for (harness, account, took) in ran {
        let before = versions
            .iter()
            .find(|(each, _)| *each == harness)
            .map(|(_, version)| version.clone())
            .unwrap_or_default();
        let version = between(before, Who::Harness(harness).version());
        log.block(
            &format!(
                "== {} {version}: {} in {} s",
                harness.name(),
                if account.held() { "ok" } else { "FAILED" },
                took.as_secs()
            ),
            &account.text(),
        );
        checks.push(Check::new(harness.name(), CHECKED, account.held(), version));
    }
    log.report("what each seat came to", &report::table(&checks));
    report::verdict_of(&log, &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_harness_named_all_five_sit_and_those_named_sit_in_the_order_given() {
        assert_eq!(seats(&[]).unwrap().len(), 5);
        assert_eq!(
            seats(&["muse".to_owned(), "devin".to_owned()]).unwrap(),
            [Harness::Muse, Harness::Devin]
        );
        assert!(seats(&["fake".to_owned()]).is_err());
        assert!(seats(&["nobody".to_owned()]).is_err());
    }

    /// The brief names a command no shell reads differently: no quote, no space inside an
    /// argument, no operator.
    #[test]
    fn the_command_is_one_every_shell_of_the_five_runs_as_written() {
        for shell_syntax in ['"', '\'', '$', '`', ';', '|', '&', '>', '<', '\\'] {
            assert!(!COMMAND.contains(shell_syntax), "{shell_syntax}");
        }
        assert_eq!(COMMAND.split(' ').count(), 3);
        assert!(brief("SEATX").contains(COMMAND));
        assert!(brief("SEATX").contains("SEATX"));
    }

    #[test]
    fn a_reply_that_holds_the_word_and_the_output_is_a_seat_that_did_the_task() {
        assert!(replied("SEATPI1 42", "SEATPI1"));
        assert!(!replied("SEATPI1", "SEATPI1"));
        assert!(!replied("42", "SEATPI1"));
        assert!(!replied("SEATCODEX1 42", "SEATPI1"));
    }

    #[test]
    fn each_seats_word_is_its_own() {
        let words: Vec<String> = Harness::ALL.iter().map(|harness| word(*harness)).collect();
        for (at, one) in words.iter().enumerate() {
            assert!(!words[at + 1..].contains(one), "{one}");
        }
    }

    #[test]
    fn the_wrapper_claude_puts_a_paste_in_is_taken_off() {
        let wrapped =
            "<pasted_content id=\"1\">\n[ConsensFlow m-9 x] body\n</pasted_content id=\"1\">";
        assert_eq!(unwrapped(wrapped), "[ConsensFlow m-9 x] body");
        assert_eq!(unwrapped("  plain "), "plain");
    }

    fn items(shape: &[(&str, &str)]) -> Vec<(String, String)> {
        shape
            .iter()
            .map(|(role, text)| ((*role).to_owned(), (*text).to_owned()))
            .collect()
    }

    /// Devin's record shows the command by its output and its exit; the reply is the last item.
    #[test]
    fn a_record_shows_the_command_ran_by_the_command_or_by_its_output_before_the_reply() {
        let devin = items(&[
            ("user", "[ConsensFlow m-1 x] Run node -p 40+2"),
            ("custom", "You are powered by SWE-2 Medium."),
            (
                "tool",
                "Output from command in shell 3d3ed1: 42 Exit code: 0",
            ),
            ("assistant", "SEATDEVIN1 42"),
        ]);
        assert!(ran_the_command(&devin));
        let called = items(&[
            ("user", "brief"),
            ("tool_use", "Bash: node -p 40+2"),
            ("assistant", "SEATCLAUDE1 42"),
        ]);
        assert!(ran_the_command(&called));
        // A reply alone, or a user's message that holds the command, shows nothing ran.
        let none = items(&[("user", "Run node -p 40+2"), ("assistant", "SEATX 42")]);
        assert!(!ran_the_command(&none));
        assert!(!ran_the_command(&[]));
        // A number in a token of another is no output.
        let other = items(&[("tool", "took 142 ms"), ("assistant", "SEATX 42")]);
        assert!(!ran_the_command(&other));
    }
}
