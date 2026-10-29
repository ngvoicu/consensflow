//! A result went into a chief's window twice, the second copy glued in front of the
//! next result.
//!
//! Seen on the owner's Windows machine on 2026-10-15 (batch B of that day's field
//! reports, B5), on a version nobody noted: the chief said m-246, a result, came to it
//! twice, and the second copy came stuck to the front of m-248, the next result. The
//! workers of that machine were Devin, and the chief most likely was too. No logs were
//! kept. The report is `issues/field-reports-2026-10-15.md` in consensflow-sme, which
//! reads it as the shape 3.0.4 fixed for a Devin chief (`devin_notes_twice`, and
//! `issues/devin-chief-got-notes-twice.md`): Devin takes what it is handed in a turn into
//! its agent's queue and writes it to its store when it runs it, so its record of a paste
//! came late, the daemon took the paste for lost and pasted it again, and Devin, holding
//! both, gave the copy to its agent with what came next. If that machine ran 3.0.3, this
//! is that bug. 3.0.4 left one shape of it, the third case below: a turn whose agent says
//! nothing for longer than the 30 s the daemon reads it busy and the daemon's arrival window
//! (a minute) after them. A Devin resumed after a cut sat so in the live probe of B1 of the same report
//! (the Mac, 3000.11.92), and the windows of that machine had closed in the middle of their
//! tasks (B2).
//!
//! What the owner should have seen: each result goes into the chief's window once, whole,
//! and apart from the other, whenever Devin's record shows it.
//!
//! The chief is the stand-in Devin (`fake-devin`), which takes a message it is handed in a
//! turn into its queue and writes its row when it runs it, and whose turns take a model's
//! time: to the agent's first word (in which the wire shows none of the turn's work) and
//! then the work. Its first word here comes 40 s after the prompt, past the 30 s the daemon
//! reads a Devin window busy from its prompt, and the work takes 25 s more: a result pasted
//! in the silence goes into a turn that has begun, and Devin's record shows it when that
//! turn ends, some 35 s after the paste, past the daemon's arrival window, which is 15 s here
//! (`ARRIVAL_MS`: the minute it gives a person's, cut by the variable the daemon reads, so
//! that a turn which outlasts it is seconds longer and no minute and a half). The workers are
//! the stand-in Claude, two of them, each given a task through the chief's window as `cf` does.
//!
//! - The first result's record is late: the human's turn is in front of it.
//! - The same, with the chief at work on a turn when the second result comes.
//! - The same, the turn in front silent for the gap and the arrival window and more
//!   (`THINK`), as a Devin resumed after a cut sat at "Thinking" for minutes: the window reads
//!   at rest for more than the daemon's arrival window while Devin holds the first result.
//!   This one failed on 3.0.4: the daemon took the
//!   paste for lost after its arrival window of the window's rest and pasted it again, into the
//!   queue that held the first. A paste made while the TUI's prompt is asked is now held
//!   in that turn for as long as a prompt is (`Window::held`, from Devin's wire log): the
//!   window counts no rest meanwhile, and the paste never goes again while it is held.
//! - The first paste's record never comes: Devin's TUI dropped the paste
//!   (`FAKE_DEVIN_DROP`), and the daemon pastes it again after its arrival window at rest.
//! - The same, and the human asks the chief why the result is still queued, quoting its
//!   header (the reviews of 2026-10-16): the daemon took the words for Devin's word that
//!   its agent had the paste, and waited for its record for good. Words that quote a
//!   header are no paste, and a word that a paste is in a turn lasts that turn: the
//!   result goes again after the human's turn and the daemon's arrival window.
//!
//! What proves it is the daemon's own word for its pastes (`delivery.begun`,
//! `delivery.retried`): a second copy would be read by the chief after the turns before it,
//! later than this looks, so the case ends at the paste over the count, the daemon's word being
//! enough. And what the chief was given by then, read from its store: each result's
//! header in one message only, that message holding the result's words and what the chief
//! decides with, and no other header. Each case runs where the rig runs, Windows too, where
//! a paste reaches Devin as the console carries it (`console_text`) and its Enter as the
//! pseudoconsole does.

use cf_e2e::devin_store::{self, Message};
use cf_e2e::files;
use cf_e2e::live::trace::at_ms;
use cf_e2e::rig::{Config, Project, Rig};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::devin_notes_twice::{arrival, store_of, work_ms, ARRIVAL_MS, WORK_MS};
use crate::{config, secs, Outcome, FAKE_DEVIN};

/// How long the stand-in's agent takes to its first word, in seconds: silent past the
/// gap the daemon reads a Devin window busy from its prompt for ([`GAP_SECONDS`]). Its work
/// after it takes [`WORK_MS`], longer than the arrival window by more than a slow
/// machine's passes take.
const FIRST_WORD_SECONDS: u64 = 40;

/// The 30 s the daemon reads a Devin window busy from a prompt of its TUI, which the
/// stand-in's first word has to outlast (the adapter's `PROMPT_GAP_MS`, which no variable sets).
const GAP_SECONDS: u64 = 30;

/// What the second `cf task add` and its worker's launch add to the worker's sleep before
/// its result is made, in seconds. Measured on 2026-10-22 (the Mac, under load) with a sleep
/// of 45: the second result was made 16 s after the first paste and 19 s before the
/// record showed that paste, the middle of the 35 s between them; the sleep is the
/// middle of the turn less this.
const SETUP_SECONDS: u64 = 2;

/// How long the agent of the case whose turn says nothing for a long time is silent, in
/// seconds: the 30 s the daemon reads the window busy from its prompt, then the arrival
/// window at rest, and a margin for the paste to come late in it.
const THINK_SECONDS: i64 = GAP_SECONDS as i64 + ARRIVAL_MS / 1_000 + 15;

/// Where every header of the daemon's begins.
const HEADER: &str = "[ConsensFlow m-";

/// How long the stand-in's agent takes to its first word, in milliseconds.
fn first_word_ms() -> String {
    (FIRST_WORD_SECONDS * 1_000).to_string()
}

/// The roster of the cases: a Devin (the stand-in) is the chief, and two workers of the
/// stand-in Claude.
fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "devin", "model": "fake" },
            { "id": "worker", "kind": "claude-code", "model": "fake" },
            { "id": "worker2", "kind": "claude-code", "model": "fake" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// A rig whose chief is the stand-in Devin, its agent as quick as `first_word` and
/// `work` say (in milliseconds), the daemon's arrival window [`ARRIVAL_MS`].
fn a_rig(first_word: &str, work: &str) -> Config {
    arrival(config().devin(FAKE_DEVIN))
        .var("FAKE_DEVIN_FIRST_WORD_MS", first_word)
        .var("FAKE_DEVIN_WORK_MS", work)
}

/// A result of the case: the task it is of, and the words its worker answers with.
#[derive(Clone, Copy)]
struct Asked {
    task: i64,
    words: &'static str,
}

const FIRST: Asked = Asked {
    task: 1,
    words: "RESULT_ONE",
};
const SECOND: Asked = Asked {
    task: 2,
    words: "RESULT_TWO",
};

/// A project with the two workers, its chief's window open: the frame `cf` runs in, as
/// the chief's agent runs it.
fn a_project(rig: &Rig) -> cf_e2e::Result<(Project<'_>, String)> {
    write_roster(rig)?;
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    project.add_member("worker2")?;
    let tier = added["member"]["tier"]
        .as_str()
        .ok_or_else(|| Error::Daemon(format!("the worker has no tier: {added}")))?
        .to_owned();
    Ok((project, tier))
}

/// The chief gives the task `asked` is of, as `cf` does in its window: its worker works
/// `seconds` and answers the words.
fn give(rig: &Rig, project: &Project, tier: &str, asked: Asked, seconds: u64) -> Outcome {
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), secs(30))?;
    let brief = format!("SLEEP {seconds} Reply with exactly: {}", asked.words);
    let given = rig.cf_in_window(&chief, ["task", "add", "--tier", tier, brief.as_str()])?;
    assert_eq!(given.code, Some(0), "{}", given.output());
    Ok(())
}

/// The human types to the chief, and a turn begins: the turn in front of the results.
fn the_human_types_to_the_chief(rig: &Rig, project: &Project) -> Outcome {
    // The chief's launch may still be at work on what it was opened with.
    rig.tell_within(project.id(), "Reply with exactly: BUSY", secs(180))?;
    Ok(())
}

/// The result `asked` asks for, as the chief's inbox holds it.
fn result_of(project: &Project, asked: Asked) -> cf_e2e::Result<Option<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "result" && message["body"] == asked.words))
}

/// A result, once the daemon has made it: its message's id.
fn id_of(project: &Project, asked: Asked) -> cf_e2e::Result<i64> {
    result_of(project, asked)?
        .and_then(|message| message["id"].as_i64())
        .ok_or_else(|| Error::Daemon(format!("no result says {}", asked.words)))
}

/// Waits for both results to be confirmed in the chief's window: the two results' ids. A
/// result pasted more often than `pasted` says ends the case there, before the chief has
/// run the copy: the daemon's word for it is enough. Where both are confirmed the chief's
/// store holds both, for a result is confirmed by the header in its row.
fn until_both_are_in(rig: &Rig, project: &Project, pasted: [usize; 2]) -> cf_e2e::Result<[i64; 2]> {
    rig.wait_for("both results to be made", secs(300), || {
        Ok(result_of(project, FIRST)?.is_some() && result_of(project, SECOND)?.is_some())
    })?;
    let ids = [id_of(project, FIRST)?, id_of(project, SECOND)?];
    rig.wait_for("the chief to be given both results", secs(600), || {
        pasted_no_more_than(&rig.events(), ids, pasted);
        let inbox = project.inbox("chief")?;
        Ok(ids.iter().all(|id| {
            inbox
                .iter()
                .any(|message| message["id"] == *id && message["state"] == "delivered")
        }))
    })?;
    Ok(ids)
}

/// The daemon's events of `kind` about the message `id`.
fn of_message<'a>(events: &'a [Value], kind: &str, id: i64) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| event["kind"] == kind && event["data"]["message"] == id)
        .collect()
}

/// When the daemon's first event of `kind` about the message `id` was written, in
/// milliseconds since the epoch.
fn first_at(events: &[Value], kind: &str, id: i64) -> Option<i64> {
    of_message(events, kind, id).into_iter().find_map(at_ms)
}

/// When the result `id` was made: its task was done with it.
fn made_at(events: &[Value], id: i64) -> Option<i64> {
    events
        .iter()
        .filter(|event| event["kind"] == "task.state" && event["data"]["result"] == id)
        .find_map(at_ms)
}

/// How late the record showed the message `id`: from its first paste to its
/// confirmation.
fn how_late(events: &[Value], id: i64) -> Option<i64> {
    Some(first_at(events, "delivery.confirmed", id)? - first_at(events, "delivery.begun", id)?)
}

/// The daemon pasted each result no more often than `pasted` says: once, but for a paste
/// Devin's TUI dropped.
fn pasted_no_more_than(events: &[Value], ids: [i64; 2], pasted: [usize; 2]) {
    for ((id, asked), pastes) in ids.iter().zip([FIRST, SECOND]).zip(pasted) {
        let begun = of_message(events, "delivery.begun", *id).len();
        assert!(
            begun <= pastes,
            "{} (m-{id}) was pasted {begun} times, not {pastes}; the daemon took it for lost: {:?}",
            asked.words,
            of_message(events, "delivery.retried", *id)
        );
    }
}

/// What the owner should have seen: the daemon pasted each result as often as `pasted`
/// says (once, but for a paste Devin's TUI dropped) and took no paste Devin had for lost;
/// and the chief was given each once, whole (its words, and what it decides with) and
/// apart (no other header in the message).
fn each_went_in_once_whole_and_apart(rig: &Rig, ids: [i64; 2], pasted: [usize; 2]) -> Outcome {
    let events = rig.events();
    pasted_no_more_than(&events, ids, pasted);
    for ((id, asked), pastes) in ids.iter().zip([FIRST, SECOND]).zip(pasted) {
        let (begun, retried) = (
            of_message(&events, "delivery.begun", *id),
            of_message(&events, "delivery.retried", *id),
        );
        assert!(
            begun.len() == pastes && retried.len() == pastes - 1,
            "{} (m-{id}) was pasted {} times, not {pastes}: {retried:?}",
            asked.words,
            begun.len()
        );
    }
    let given: Vec<Message> = devin_store::messages(&store_of(rig))?
        .into_iter()
        .filter(|message| message.role == "user")
        .collect();
    let firsts = || -> Vec<&str> {
        given
            .iter()
            .map(|message| message.text.lines().next().unwrap_or_default())
            .collect()
    };
    for (id, asked) in ids.iter().zip([FIRST, SECOND]) {
        let marker = format!("{HEADER}{id} ");
        let holding: Vec<&Message> = given
            .iter()
            .filter(|message| message.times(&marker) > 0)
            .collect();
        let times: usize = holding.iter().map(|message| message.times(&marker)).sum();
        assert_eq!(
            times,
            1,
            "{} (m-{id}) went into the chief's window {times} times; what it was given: {:?}",
            asked.words,
            firsts()
        );
        let text = &holding[0].text;
        assert!(
            text.starts_with(&marker)
                && text.contains(asked.words)
                && text.contains(&format!("cf task accept T-{}", asked.task)),
            "{} (m-{id}) did not go in whole: {text:?}",
            asked.words
        );
        assert_eq!(
            text.matches(HEADER).count(),
            1,
            "{} (m-{id}) went in with another message: {text:?}",
            asked.words
        );
    }
    Ok(())
}

#[test]
fn two_results_for_a_devin_chief_whose_record_of_the_first_is_late_go_in_once_each_whole_and_apart(
) -> Outcome {
    let rig = Rig::start(a_rig(&first_word_ms(), &work_ms()))?;
    let (project, tier) = a_project(&rig)?;
    // The human's turn is in front: the first result, made in it, is pasted in its
    // silence, and the second waits behind the first.
    the_human_types_to_the_chief(&rig, &project)?;
    give(&rig, &project, &tier, FIRST, 2)?;
    give(&rig, &project, &tier, SECOND, 6)?;
    let ids = until_both_are_in(&rig, &project, [1, 1])?;

    // What the case is about: the record showed the first result later than the daemon's
    // arrival window after its paste. Without it the case would hold nothing.
    let late = how_late(&rig.events(), ids[0]);
    assert!(
        late.is_some_and(|late| late > ARRIVAL_MS),
        "the record of the first result was not late: {late:?} ms"
    );
    each_went_in_once_whole_and_apart(&rig, ids, [1, 1])?;
    rig.close()?;
    Ok(())
}

#[test]
fn two_results_for_a_devin_chief_at_work_when_the_second_comes_go_in_once_each_whole_and_apart(
) -> Outcome {
    let rig = Rig::start(a_rig(&first_word_ms(), &work_ms()))?;
    let (project, tier) = a_project(&rig)?;
    the_human_types_to_the_chief(&rig, &project)?;
    give(&rig, &project, &tier, FIRST, 2)?;
    // The second worker answers once the first result is in the chief's turn, and the
    // chief at work: half way between the first paste (the gap after the prompt) and the
    // end of the turn (its first word and its work), where a slow runner has the most
    // room on either side.
    let turn = FIRST_WORD_SECONDS + WORK_MS as u64 / 1_000;
    give(
        &rig,
        &project,
        &tier,
        SECOND,
        (GAP_SECONDS + turn) / 2 - SETUP_SECONDS,
    )?;
    let ids = until_both_are_in(&rig, &project, [1, 1])?;

    // What the case is about: the second result was made after the first was pasted and
    // before Devin's record showed it, the chief in its turn all the while.
    let events = rig.events();
    let made = made_at(&events, ids[1]);
    let (pasted, shown) = (
        first_at(&events, "delivery.begun", ids[0]),
        first_at(&events, "delivery.confirmed", ids[0]),
    );
    assert!(
        matches!((pasted, made, shown), (Some(pasted), Some(made), Some(shown)) if pasted < made && made < shown),
        "the second result was not made while the chief held the first in its turn: pasted {pasted:?}, made {made:?}, shown {shown:?}"
    );
    let late = how_late(&events, ids[0]);
    assert!(
        late.is_some_and(|late| late > ARRIVAL_MS),
        "the record of the first result was not late: {late:?} ms"
    );
    each_went_in_once_whole_and_apart(&rig, ids, [1, 1])?;
    rig.close()?;
    Ok(())
}

/// The turn in front is one whose agent says nothing for as long as the window reads busy
/// from its prompt and then for the arrival window and more ([`THINK_SECONDS`]), as a
/// Devin resumed after a cut sat at "Thinking" for minutes (B1 of the same report, a Devin
/// worker on the same machine; the live suite `devin-resume`, 3000.11.92). The daemon reads
/// the window busy for 30 s of the prompt and then at rest, so the first result goes into
/// the turn, where Devin holds it, and its record shows it only when the turn ends: more
/// than the arrival window of the window's rest after the paste.
#[test]
fn two_results_for_a_devin_chief_whose_turn_says_nothing_for_minutes_go_in_once_each_whole_and_apart(
) -> Outcome {
    let rig = Rig::start(a_rig("2000", "5000"))?;
    let (project, tier) = a_project(&rig)?;
    rig.tell_within(
        project.id(),
        &format!("THINK {THINK_SECONDS} Reply with exactly: BUSY"),
        secs(180),
    )?;
    give(&rig, &project, &tier, FIRST, 2)?;
    give(&rig, &project, &tier, SECOND, 6)?;
    let ids = until_both_are_in(&rig, &project, [1, 1])?;

    // What the case is about: the window rested more than the daemon's arrival window
    // while Devin held the first result, by half a window and more, as the case was
    // written (a minute and a half for the minute). The record shows the result when the
    // turn ends, so it is that late after the paste, less the gap the paste waited for.
    // A turn silent for less would leave the held rule out of the shape.
    let late = how_late(&rig.events(), ids[0]);
    assert!(
        late.is_some_and(|late| late > ARRIVAL_MS + ARRIVAL_MS / 2),
        "the record of the first result was not late enough: {late:?} ms, not more than {} ms",
        ARRIVAL_MS + ARRIVAL_MS / 2
    );
    each_went_in_once_whole_and_apart(&rig, ids, [1, 1])?;
    rig.close()?;
    Ok(())
}

#[test]
fn two_results_for_a_devin_chief_whose_tui_dropped_the_first_paste_go_in_once_each_whole_and_apart(
) -> Outcome {
    // A chief quick to answer: what the case is about is the paste no record ever shows.
    let rig = Rig::start(a_rig("2000", "5000").var("FAKE_DEVIN_DROP", FIRST.words))?;
    let (project, tier) = a_project(&rig)?;
    give(&rig, &project, &tier, FIRST, 2)?;
    give(&rig, &project, &tier, SECOND, 6)?;
    // The daemon's arrival window of the window's rest, and its next look: the paste goes again.
    rig.wait_for("the first result to be made", secs(120), || {
        Ok(result_of(&project, FIRST)?.is_some())
    })?;
    let first = id_of(&project, FIRST)?;
    rig.wait_for("the dropped paste to go again", secs(150), || {
        Ok(of_message(&rig.events(), "delivery.begun", first).len() >= 2)
    })?;
    let ids = until_both_are_in(&rig, &project, [2, 1])?;

    // The paste Devin dropped went again, once, an arrival window on: that was its one arrival.
    let events = rig.events();
    // The daemon was given the window the case sizes its turns by, not the minute it gives a
    // person's: a daemon that did not read the variable pastes again a minute on.
    let pasted_at: Vec<i64> = of_message(&events, "delivery.begun", ids[0])
        .into_iter()
        .filter_map(at_ms)
        .collect();
    assert!(
        matches!(pasted_at.as_slice(), [first, again] if again - first < 3 * ARRIVAL_MS),
        "the paste Devin dropped did not go again within three arrival windows ({ARRIVAL_MS} ms each); a daemon that does not read the variable takes its minute: {pasted_at:?}"
    );
    let retried = of_message(&events, "delivery.retried", ids[0]);
    assert!(
        retried.iter().all(|event| event["data"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.starts_with("the harness record never showed it"))),
        "{retried:?}"
    );
    each_went_in_once_whole_and_apart(&rig, ids, [2, 1])?;
    rig.close()?;
    Ok(())
}

#[test]
/// Since 3.0.7 (chief-meeting, CMT-23, the chief's decision of 2026-10-26): a record that
/// holds a message's header after its hand-over, in an item the proof cannot match, is never
/// pasted over. The human's words that quote it are such an item: the result is taken for
/// arrived, unproven, and the chief is told once that it can read it. Before, it went again.
fn a_result_whose_paste_devins_tui_dropped_is_not_pasted_over_the_humans_words_that_quote_its_header(
) -> Outcome {
    let rig = Rig::start(a_rig("2000", "5000").var("FAKE_DEVIN_DROP", FIRST.words))?;
    let (project, tier) = a_project(&rig)?;
    give(&rig, &project, &tier, FIRST, 2)?;
    rig.wait_for("the first result to be made", secs(120), || {
        Ok(result_of(&project, FIRST)?.is_some())
    })?;
    let first = id_of(&project, FIRST)?;
    rig.wait_for("the first result to be pasted", secs(60), || {
        Ok(!of_message(&rig.events(), "delivery.begun", first).is_empty())
    })?;
    // The human asks why it is still queued, quoting its header as the window shows it:
    // on Windows, as the console carries it to Devin.
    let sender = result_of(&project, FIRST)?
        .and_then(|result| result["sender"].as_str().map(str::to_owned))
        .ok_or_else(|| Error::Daemon("the result has no sender".to_owned()))?;
    let mark = if cfg!(windows) { "|" } else { "·" };
    let header = format!(
        "{HEADER}{first} {mark} T-{} {mark} result from @{sender}]",
        FIRST.task
    );
    rig.tell_within(
        project.id(),
        &format!("Why is \"{header}\" still queued?"),
        secs(50),
    )?;
    // The human's turn, and the daemon's arrival window of the window's rest after it: the
    // paste would have gone again, and does not: the record holds its header.
    rig.wait_for(
        "the result to be taken for arrived, unproven",
        secs(240),
        || Ok(!of_message(&rig.events(), "delivery.unproven", first).is_empty()),
    )?;
    let events = rig.events();
    assert_eq!(
        (
            of_message(&events, "delivery.begun", first).len(),
            of_message(&events, "delivery.retried", first).len()
        ),
        (1, 0),
        "pasted once, never over the human's words"
    );
    let shape = &of_message(&events, "delivery.unproven", first)[0]["data"]["shape"];
    assert_eq!(shape["before"], "words", "{shape}");
    let settled = result_of(&project, FIRST)?
        .ok_or_else(|| Error::Daemon("the result is gone".to_owned()))?;
    assert_eq!(
        (
            settled["state"].clone(),
            settled["receipt"]["unproven"].clone()
        ),
        (json!("delivered"), json!(true))
    );
    let told: Vec<String> = project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["sender"].is_null())
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .filter(|body| body.contains("couldn't be proven"))
        .collect();
    assert_eq!(
        told,
        [format!(
            "m-{first}'s arrival couldn't be proven from the record; it was not pasted again. @chief can read it with `cf inbox read m-{first}`."
        )],
        "the chief is told once"
    );
    rig.close()?;
    Ok(())
}
