//! Live: a Claude chief whose `/compact` the human cancelled with Escape, through the
//! product. It is opt-in (`cargo xtask live compact-cancelled`): it spends the machine's
//! quota on a Claude Code chief, and needs it installed and logged in, so it is ignored
//! unless asked for and is no part of `check` or of CI.
//!
//! The finding it answers (the live suite `commands-at-rest`, 2026-10-20, Claude Code
//! 2.1.304, and again on 2.1.310, T-483): `/compact` typed into an idle Claude, and Escape a
//! moment after it began compacting. Claude writes of that the bare `/compact` and, at the
//! Escape, the words it writes before a command (`<local-command-caveat>`), a sibling of the
//! bare command, and nothing more; and it puts the `/compact` back in its input box.
//! ConsensFlow read the chief as working until the human typed again, and a reading at rest
//! alone would have pasted the next message behind the `/compact`, to be sent with it as the
//! compaction's instructions. Claude's reader now ends the turn at those words, and the
//! adapter clears the box before the next paste (`cf_harness::claude`'s `restored`).
//!
//! A real Claude Code is the chief of a project on the native daemon, as it ships, and its
//! one member is the stand-in (no model). The human asks the chief something, to have an
//! exchange to compact, then types `/compact` into its window as the human types at the
//! app's page (`pane.typed`, which the pane host counts) and Escape a second later. The run
//! needs the cancel's own records (the bare command and the words, with no boundary) before
//! it judges anything: a compaction that finished before the Escape is no cancel.
//!
//! - `alone`  then the chief must read idle on the board, and a task given to the member
//!   then must have its result handed to the chief alone: a message of Claude's record that
//!   begins with its header, answered, and nothing compacted.
//! - `typed`  the human types words of their own after what the box holds, and sends
//!   nothing: the result must wait (the box holds the human's words), and nothing reach
//!   Claude's record; then the human sends what the box holds, which compacts with their
//!   words as its instructions, and the result goes in alone after it.
//!
//! ```text
//! cargo xtask live compact-cancelled                    both cases
//! cargo xtask live compact-cancelled --case alone
//! ```
//!
//! The run fails when a case's cancel never came, the chief never read idle after it, the
//! result went in behind the box's text or not at all, or (`typed`) went in while the box
//! held the human's words.

use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::claude::{self, Entry};
use cf_e2e::live::members::{
    find, member_as, named, session_of, stand_in_worker, Account, Members, Plan, STAND_IN_WORKER,
};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, ask, Harness, Log};
use cf_e2e::Result;
use serde_json::Value;

/// The stand-in for the member's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The human's keys: `/compact`, the Enter that sends it, the Escape that cancels it, and
/// the words `typed` adds after what the box holds.
const COMPACT: &[u8] = b"/compact";
const ENTER: &[u8] = b"\r";
const ESCAPE: &[u8] = &[27];
const THEIR_WORDS: &str = " keep the plan";

/// The gap between keys as a person types them, and between the Enter and the Escape.
const KEYED: Duration = Duration::from_millis(600);
const CANCEL_AFTER: Duration = Duration::from_secs(1);

/// How long the chief has to answer, to write its cancel, to read idle, and a result to
/// be handed to it; how long `typed` watches that nothing is pasted into its box.
const ANSWER: Duration = Duration::from_secs(180);
const CANCELLED: Duration = Duration::from_secs(30);
const IDLE: Duration = Duration::from_secs(60);
const DELIVERED: Duration = Duration::from_secs(300);
const HELD: Duration = Duration::from_secs(45);

/// How often the board and the record are looked at.
const LOOK: Duration = Duration::from_millis(250);

/// A case of the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    Alone,
    Typed,
}

impl Case {
    const ALL: [Case; 2] = [Case::Alone, Case::Typed];

    const fn name(self) -> &'static str {
        match self {
            Case::Alone => "alone",
            Case::Typed => "typed",
        }
    }
}

/// What the check opens: a project whose chief is a real Claude Code, on its cheap model,
/// and whose one member is the stand-in agent, which has no model.
fn plan() -> Plan {
    Plan::new("compact-cancelled", member_as(Harness::Claude, "chief")).with(stand_in_worker())
}

/// The conversation the chief's window shows, as its launch names it.
fn chief_session(members: &Members) -> Option<String> {
    let pane = format!("p{}-chief", members.project_id());
    members
        .rig()
        .open_frames()
        .into_iter()
        .rev()
        .find(|frame| frame["id"] == pane.as_str())
        .and_then(|frame| session_of(&frame))
}

/// The records of the chief's conversation from the `mark`th on.
fn written_since(members: &Members, session: &str, mark: usize) -> Vec<Entry> {
    claude::record(&members.claude_config(), session)
        .into_iter()
        .skip(mark)
        .collect()
}

/// The user's record of `/compact` as it was typed, with `words` after it.
fn bare_compact(entry: &Entry, words: &str) -> bool {
    entry.kind == "user" && entry.text == format!("/compact{words}")
}

/// The words Claude Code writes before a command.
fn caveat(entry: &Entry) -> bool {
    entry.kind == "user" && entry.text.starts_with("<local-command-caveat>")
}

/// A compaction's boundary.
fn boundary(entry: &Entry) -> bool {
    entry.kind == "system/compact_boundary"
}

/// Whether what was written is a cancel: the bare command and the words before a command,
/// and no boundary.
fn cancelled(written: &[Entry]) -> bool {
    written.iter().any(|entry| bare_compact(entry, ""))
        && written.iter().any(caveat)
        && !written.iter().any(boundary)
}

/// A record as a line of the report: what it is, and how its words open.
fn shape(entry: &Entry) -> String {
    let opening: String = entry.text.replace('\n', " ").chars().take(60).collect();
    format!("{} {opening}", entry.kind)
}

/// The state of the chief's lane.
fn chief_state(members: &Members) -> Result<String> {
    Ok(members.lane("chief")?["activity"]["state"]
        .as_str()
        .unwrap_or("no lane")
        .to_owned())
}

/// The result for the task `number` in the chief's inbox.
fn result_for(members: &Members, number: i64) -> Result<Option<Value>> {
    Ok(members
        .inbox(Some("chief"))?
        .into_iter()
        .find(|message| message["kind"] == "result" && message["taskNumber"] == number))
}

/// Where the result `id` went in among what was `written`: the place of the user's record
/// that holds its header, and whether that record begins with it, as Claude Code 2.1.310
/// writes a paste: in its wrapper (`<pasted_content id="…">`, on a line of its own).
fn where_it_went(written: &[Entry], id: i64) -> Option<(usize, bool)> {
    let header = format!("[ConsensFlow m-{id} ");
    let alone = |text: &str| {
        let text = text.trim_start();
        let unwrapped = text
            .strip_prefix("<pasted_content")
            .and_then(|rest| rest.split_once('>'))
            .map_or(text, |(_, rest)| rest.trim_start());
        unwrapped.starts_with(&header)
    };
    written
        .iter()
        .position(|entry| entry.kind == "user" && entry.text.contains(&header))
        .map(|at| (at, alone(&written[at].text)))
}

/// One case: the exchange, the cancel, and what comes after it.
fn drive(case_of: Case, mut members: Members, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    case.say(format!("trust: {}", members.trust().unwrap_or("none")));
    members.add(STAND_IN_WORKER, &[])?;
    let pane = members.lane("chief")?["pane"].clone();
    let session = chief_session(&members).unwrap_or_default();

    // An exchange to compact.
    members.paste(
        &pane,
        &ask::window_text("Reply with exactly one line: READY")?,
    )?;
    let answered = find(log, "the chief to answer", ANSWER, LOOK, || {
        let read = members.project().lane("chief")?["activity"]["state"] == "idle";
        let said = claude::record(&members.claude_config(), &session)
            .iter()
            .any(|entry| entry.kind == "assistant" && entry.text.contains("READY"));
        Ok((read && said).then_some(()))
    })?;
    if answered.is_none() {
        case.fail("the chief never answered its first message");
        case.closed(members.close());
        return Ok(case);
    }

    // The human types /compact, and Escape a moment after it began.
    let mark = claude::record(&members.claude_config(), &session).len();
    for keys in [COMPACT, ENTER] {
        members.rig().types(&pane, keys)?;
        sleep(KEYED);
    }
    let began = find(log, "the bare /compact", CANCELLED, LOOK, || {
        Ok(written_since(&members, &session, mark)
            .iter()
            .any(|entry| bare_compact(entry, ""))
            .then_some(()))
    })?;
    sleep(CANCEL_AFTER);
    members.rig().types(&pane, ESCAPE)?;
    let came = find(log, "the cancel's records", CANCELLED, LOOK, || {
        let written = written_since(&members, &session, mark);
        Ok((cancelled(&written) || written.iter().any(boundary)).then_some(written))
    })?;
    let written = came.unwrap_or_default();
    for entry in &written {
        case.say(format!("  {}", shape(entry)));
    }
    let was_cancelled = began.is_some() && cancelled(&written);
    case.check(
        "the human's Escape cancelled the compaction",
        was_cancelled,
        if was_cancelled {
            "the bare /compact and the words before a command, no boundary"
        } else if written.iter().any(boundary) {
            "the compaction finished before the Escape: no cancel to judge"
        } else {
            "Claude wrote no cancel"
        },
    );
    if !was_cancelled {
        case.closed(members.close());
        return Ok(case);
    }

    // Claude is back at its prompt, and so must the chief be.
    let idle = find(log, "the chief to read idle", IDLE, LOOK, || {
        Ok((chief_state(&members)? == "idle").then_some(()))
    })?;
    case.check(
        "the chief reads idle after the cancel",
        idle.is_some(),
        format!("its lane reads {}", chief_state(&members)?),
    );

    // `typed`: the human's words after what the box holds, not sent.
    if case_of == Case::Typed {
        members.rig().types(&pane, THEIR_WORDS.as_bytes())?;
    }

    // A result for the chief: the member's task, given as the chief would give it.
    let number = members.give(STAND_IN_WORKER, "Reply with exactly: DONE")?;
    let before_sent = written_since(&members, &session, mark).len();
    if case_of == Case::Typed {
        // The box holds the human's words: nothing is pasted into it while it does.
        let pasted = find(
            log,
            "anything written while the box holds",
            HELD,
            LOOK,
            || {
                let now = written_since(&members, &session, mark);
                Ok((now.len() > before_sent).then_some(now))
            },
        )?;
        let held = pasted.is_none()
            && result_for(&members, number)?.is_some_and(|result| result["state"] != "delivered");
        case.check(
            "the result waits while the box holds the human's words",
            held,
            match &pasted {
                None => format!("nothing was written in {} s", HELD.as_secs()),
                Some(now) => format!(
                    "Claude wrote {} while the box held them",
                    now[before_sent..]
                        .iter()
                        .map(shape)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
        );
        // The human sends what the box holds: their `/compact`, with their words.
        members.rig().types(&pane, ENTER)?;
        let compacted = find(
            log,
            "the human's /compact to compact",
            DELIVERED,
            LOOK,
            || {
                let now = written_since(&members, &session, mark);
                Ok((now.iter().any(|entry| bare_compact(entry, THEIR_WORDS))
                    && now.iter().any(boundary))
                .then_some(()))
            },
        )?;
        case.check(
            "what the box held was the cancelled /compact with the human's words",
            compacted.is_some(),
            if compacted.is_some() {
                "sent, it compacted with them as its instructions"
            } else {
                "no `/compact keep the plan` compacted"
            },
        );
    }

    let delivered = find(
        log,
        "the result to be handed to the chief",
        DELIVERED,
        LOOK,
        || Ok(result_for(&members, number)?.filter(|result| result["state"] == "delivered")),
    )?;
    let after = written_since(&members, &session, mark);
    let went = delivered
        .as_ref()
        .and_then(|result| result["id"].as_i64())
        .and_then(|id| where_it_went(&after, id));
    let boundaries_before = after
        .iter()
        .take(went.map_or(after.len(), |(at, _)| at))
        .filter(|entry| boundary(entry))
        .count();
    let compacted_by_it = after
        .iter()
        .skip(went.map_or(after.len(), |(at, _)| at))
        .any(boundary);
    case.check(
        "the result is handed to the chief alone",
        went.is_some_and(|(_, alone)| alone) && !compacted_by_it,
        match (went, compacted_by_it) {
            (Some((_, true)), false) => {
                "a message of Claude's record that begins with its header".to_owned()
            }
            (Some((at, false)), _) => {
                format!("it went in behind what the box held: {}", shape(&after[at]))
            }
            (Some(_), true) => "a compaction ran after it: it was its instructions".to_owned(),
            (None, _) => "never delivered, or no record of it".to_owned(),
        },
    );
    case.say(format!(
        "compactions before the result: {boundaries_before} (`typed` has the human's)"
    ));
    let rested = find(log, "the chief to read idle again", ANSWER, LOOK, || {
        Ok((chief_state(&members)? == "idle").then_some(()))
    })?;
    case.check(
        "the chief answers the result and reads idle",
        rested.is_some(),
        format!("its lane reads {}", chief_state(&members)?),
    );
    case.closed(members.close());
    Ok(case)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a Claude Code chief; cargo xtask live compact-cancelled"]
fn a_claude_chief_whose_compact_the_human_cancelled_reads_idle_and_is_handed_the_next_message_alone(
) -> Result {
    let log = Log::begin("live compact-cancelled")?;
    ask::prepare()?;
    let cases = named(
        &live::list("case"),
        &Case::ALL,
        Case::name,
        "case",
        &Case::ALL,
    )?;
    let mut checks = Vec::new();
    for case_of in cases {
        let scoped = log.scoped(format!("claude {}", case_of.name()));
        let case = Members::open(&plan(), Path::new(FAKE_AGENT), &scoped)
            .and_then(|members| drive(case_of, members, &scoped))
            .unwrap_or_else(|failed| {
                let mut case = Account::new();
                case.fail(failed);
                case
            });
        let version = live::version_of(Harness::Claude);
        log.block(
            &format!("== claude {}: {version}", case_of.name()),
            &case.text(),
        );
        checks.push(Check::new(
            &format!("claude {}", case_of.name()),
            if case.held() {
                "the chief read idle after the cancel and was handed the next message alone"
            } else {
                "the cancel, the chief's rest or the message after it did not come as it should"
            },
            case.held(),
            version,
        ));
    }
    report::conclude(&log, "what came of each cancelled compaction", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, text: &str) -> Entry {
        Entry {
            at_ms: None,
            kind: kind.to_owned(),
            text: text.to_owned(),
            model: None,
            raw: String::new(),
        }
    }

    #[test]
    fn both_cases_run_unless_one_is_named_and_one_that_is_none_is_refused() {
        let all = named(&[], &Case::ALL, Case::name, "case", &Case::ALL).unwrap();
        assert_eq!(all, Case::ALL);
        let one = named(
            &["typed".to_owned()],
            &Case::ALL,
            Case::name,
            "case",
            &Case::ALL,
        )
        .unwrap();
        assert_eq!(one, [Case::Typed]);
        assert!(named(
            &["other".to_owned()],
            &Case::ALL,
            Case::name,
            "case",
            &Case::ALL
        )
        .is_err());
    }

    #[test]
    fn a_cancel_is_the_bare_command_and_the_words_before_a_command_and_no_boundary() {
        let bare = || entry("user", "/compact");
        let words = || entry("user", "<local-command-caveat>Run by the user.");
        assert!(cancelled(&[bare(), words()]));
        for not in [
            vec![bare()],
            vec![words()],
            vec![bare(), entry("system/compact_boundary", ""), words()],
            vec![entry("user", "/compact more"), words()],
        ] {
            assert!(!cancelled(&not), "{not:?}");
        }
    }

    #[test]
    fn a_result_went_in_alone_where_a_users_record_begins_with_its_header() {
        let header = "[ConsensFlow m-7 · T-1 · result from @worker]\nDONE";
        assert_eq!(
            where_it_went(&[entry("user", "/compact"), entry("user", header)], 7),
            Some((1, true))
        );
        assert_eq!(
            where_it_went(&[entry("user", &format!("/compact{header}"))], 7),
            Some((0, false))
        );
        // In the wrapper Claude Code 2.1.310 writes a paste in.
        let wrapped = format!("\n\n<pasted_content id=\"88dd\">\n{header}\n</pasted_content>");
        assert_eq!(
            where_it_went(&[entry("user", &wrapped)], 7),
            Some((0, true))
        );
        let behind = format!("/compact\n<pasted_content id=\"88dd\">\n{header}");
        assert_eq!(
            where_it_went(&[entry("user", &behind)], 7),
            Some((0, false))
        );
        assert_eq!(where_it_went(&[entry("user", header)], 8), None);
        assert_eq!(where_it_went(&[entry("assistant", header)], 7), None);
    }
}
