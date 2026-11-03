//! Live: a message pasted into a Claude Code window while its turn works is read as
//! arrived, once, from the record Claude Code writes of it. It is opt-in (`cargo xtask live
//! absorbed`, `npm run live:absorbed`): it spends the machine's quota on one Claude Code
//! window, on its cheap model, so it is ignored unless asked for and is no part of `check`
//! or of CI.
//!
//! The field bug it answers (T-716, T-793's CMT-02; the chief's m-4182 and m-4697 on
//! 2026-10-26, Claude Code 2.1.310): a message pasted while Claude Code works is queued, and
//! at the next tool's result Claude Code takes it into the turn as an attachment
//! (`queued_command`, `"commandMode":"prompt"`) and writes no user's record of it. The
//! daemon's reader read no item of it, so the arrival was never proven, and a minute of
//! rest after the turn the message was pasted again.
//!
//! The window is asked to run a 40 s loop of short `sleep`s and then reply with one word. While the
//! command runs, a message shaped like a ConsensFlow note is pasted as the daemon pastes
//! one (`pane.write_paste`), asking for a sum as well. When the turn has ended, the record
//! is read as the daemon reads it (`harness-ask`), and held to this: Claude Code took the
//! note into the turn (its attachment is in the transcript; a run where it did not, because
//! the command ended first, proves nothing and fails), and the reading holds the note as one
//! user's item, the attachment's, and the turn settled.
//!
//! ```text
//! cargo xtask live absorbed            Claude Code, the one harness that takes a paste so
//! cargo xtask live absorbed -- --list  lists this test
//! ```

use std::fs;
use std::time::Duration;

use cf_e2e::live::claude;
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Open, Window, ANSWER};
use cf_e2e::live::{self, ask, Harness, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};
use serde_json::Value;

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows: the
/// window here is opened on the real harness's own path, but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The folder the window works in, and its id.
const FOLDER: &str = "absorbed";

/// The turn the note is pasted into: a command that runs long enough for the paste, in the
/// turn (one run in the background ends the turn at once: the first live run, Sonnet 5.5),
/// and no lone `sleep`, which Claude Code 2.1.311's Bash tool refuses in the foreground.
const WORK: &str = "Use your Bash tool to run exactly this command in the foreground, with run_in_background false, and wait for it to finish: for i in 1 2 3 4 5 6 7 8; do echo tick $i; /bin/sleep 5; done . When it has finished, reply with the word SLEPT, and do whatever else a message given to you meanwhile asks.";

/// The note's header, which the record must hold once.
const HEADER: &str = "[ConsensFlow m-7 · note from @human]";

/// The note: a header, and a sum it does not hold.
const NOTE: &str = "[ConsensFlow m-7 · note from @human]\nWhen you reply, also give the sum of 2468 and 1357, in digits.";

/// The sum the note asks for.
const SUM: &str = "3825";

/// How long the turn may take to show its call in the record.
const CALLED: Duration = Duration::from_secs(120);

/// How often the record is read.
const LOOK: Duration = Duration::from_millis(500);

/// The transcript's records, as Claude Code wrote them.
fn raw_records(window: &Window) -> Vec<Value> {
    let session = window.session().unwrap_or_default();
    claude::transcript_file(&window.claude_config(), &session)
        .and_then(|file| fs::read_to_string(file).ok())
        .map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The uuid of each attachment that took a human's message holding `header` into a turn.
fn taken_in(records: &[Value], header: &str) -> Vec<String> {
    records
        .iter()
        .filter(|record| {
            let attachment = &record["attachment"];
            record["type"] == "attachment"
                && attachment["type"] == "queued_command"
                && attachment["commandMode"] == "prompt"
                && attachment["origin"]["kind"] == "human"
                && attachment["prompt"]
                    .as_str()
                    .is_some_and(|prompt| prompt.contains(header))
        })
        .filter_map(|record| record["uuid"].as_str().map(str::to_owned))
        .collect()
}

/// How many user's records of the transcript hold `header`: none, where Claude Code took
/// the message into the turn.
fn user_records(records: &[Value], header: &str) -> usize {
    records
        .iter()
        .filter(|record| record["type"] == "user" && record["message"].to_string().contains(header))
        .count()
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of Claude Code; cargo xtask live absorbed"]
fn a_message_claude_code_takes_into_a_turn_under_way_is_read_as_arrived_once() -> Result {
    let log = Log::begin("live absorbed")?;
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let claude = log.found(Harness::Claude)?;
    let rig = Rig::start(live::rig(std::slice::from_ref(&claude), FAKE_AGENT))?;
    let window = Open::new(&rig, &claude, FOLDER, FOLDER).open_telling(&log, "claude")?;
    window
        .ready()
        .map_err(|screen| Error::Daemon(format!("the window did not open: {screen}")))?;
    window.paste(&window.given(WORK)?)?;
    // The turn has called its command once the record's last item is the assistant's call.
    let called = live::until(CALLED, LOOK, || {
        let record = window.record()?;
        let last = record.items.last()?;
        (record.settlement == "in-flight" && last.role == "assistant").then_some(())
    });
    if called.is_none() {
        return Err(Error::Daemon(format!(
            "the turn never called its command: {}",
            window.tail()
        )));
    }
    log.say("claude: the turn runs its command; the note is pasted");
    let pasted = window.paste(&window.given(NOTE)?)?;
    let answered = live::until(ANSWER, LOOK, || {
        let record = window.record()?;
        (record.settled && window.screen().contains(SUM)).then_some(())
    });
    let records = raw_records(&window);
    let attachments = taken_in(&records, HEADER);
    let items: Vec<ask::Item> = window
        .recorded()
        .into_iter()
        .filter(|item| item.role == "user" && item.text.contains(HEADER))
        .collect();
    let settled = window.record().is_some_and(|record| record.settled);
    let checks = vec![
        Check::new(
            "claude",
            "the paste went in",
            pasted["ok"] == true,
            pasted.to_string(),
        ),
        Check::new(
            "claude",
            "Claude Code took the note into the turn under way",
            attachments.len() == 1,
            format!(
                "{} attachment(s) of it, {} user's record(s) of it: a run where none took it in proves nothing",
                attachments.len(),
                user_records(&records, HEADER)
            ),
        ),
        Check::new(
            "claude",
            "the reading holds the note once, as the attachment's user's item",
            items.len() == 1 && attachments.first() == items.first().map(|item| &item.id),
            format!(
                "items {:?}, attachments {attachments:?}",
                items.iter().map(|item| &item.id).collect::<Vec<_>>()
            ),
        ),
        Check::new(
            "claude",
            "the turn ended, answering the note",
            answered.is_some() && settled,
            window.tail(),
        ),
    ];
    drop(window);
    rig.close()?;
    report::conclude(&log, "what each check came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_note_taken_in_is_found_by_its_attachment_and_by_no_user_s_record() {
        let records = vec![
            json!({ "type": "user", "message": { "content": "Use your Bash tool" } }),
            json!({ "type": "attachment", "uuid": "q1", "attachment": {
                "type": "queued_command", "commandMode": "prompt", "origin": { "kind": "human" },
                "prompt": NOTE } }),
            json!({ "type": "attachment", "uuid": "q2", "attachment": {
                "type": "queued_command", "commandMode": "prompt", "origin": { "kind": "peer" },
                "prompt": NOTE } }),
        ];
        assert_eq!(taken_in(&records, HEADER), ["q1"]);
        assert_eq!(user_records(&records, HEADER), 0);
        let typed = json!({ "type": "user", "message": { "content": NOTE } });
        assert_eq!(user_records(&[typed], HEADER), 1);
    }

    #[test]
    fn the_note_holds_its_header_and_not_its_sum() {
        assert!(NOTE.starts_with(HEADER));
        assert!(!NOTE.contains(SUM));
        assert!(!WORK.contains(SUM));
    }
}
