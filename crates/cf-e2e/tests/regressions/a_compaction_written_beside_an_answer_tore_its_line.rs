//! The 3.0.6-alpha.1 release's dry run of 2026-10-21 (CI run 39692888986, the macOS
//! runner, T-187): **`the_history_reads_a_chief_s_conversations_whole_across_a_compaction_and_a_switch_of_harness`
//! failed with "timed out after 60 s waiting for the summary to be copied", and
//! passed twice before on the same commit.**
//!
//! What the case saw: the summary of a compaction never reached the ledger's copy
//! of the chief's conversation, and neither did anything the chief wrote after it.
//! The cause was the case's own. It waited for "the chief to answer" by the words
//! it had told the chief, which the record holds from the user's line on, so it
//! compacted the record while the stand-in was still writing the answer; and its
//! helper wrote each record of the compaction with `writeln!`, a token at a time.
//! The stand-in's answer, one write, landed inside the boundary's line. A line cut
//! by another record is malformed JSON for good, and the Claude reader fails a look
//! at a record with one (as Node's did), so the daemon copied nothing more of the
//! conversation. The rig's report of the failed case names it ("not JSON:"). Real
//! Claude Code writes each record whole, which is what the helper does now.
//!
//! What should happen: whatever the stand-in writes beside a compaction, the summary
//! and the answer both reach the copy, and a case that compacts "while idle" waits
//! for the turn's end in the record, never for the words that began it.
//!
//! The first case writes a compaction as the stand-in answers: its writer stalls
//! after its first write, for longer than the stand-in takes to answer, which is what
//! a test thread the runner left waiting for its turn does to a line of many writes.
//! The answer takes 3 s and the stall 6, so that the first write is in the record
//! before the answer even when the thread is a couple of seconds late to it, as under
//! a landing's parallel builds; with less margin a late thread would write its first
//! byte after the answer, and the plant that tears the line would pass. The second is
//! the wait.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::rig::{Project, Rig};
use serde_json::json;

use crate::idle_compaction_read_as_a_cut_turn::{
    history, says, told, wait_for_the_turn_to_end, write_compaction, SUMMARY,
};
use crate::{config, secs, Outcome};

/// A record written to, whose first write is followed by a stall.
struct Stalling {
    record: File,
    stall: Duration,
    stalled: bool,
}

impl Write for Stalling {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.record.write(bytes)?;
        if !self.stalled {
            self.stalled = true;
            sleep(self.stall);
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.record.flush()
    }
}

#[test]
fn a_compaction_written_as_the_stand_in_answers_is_copied_with_the_answer() -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_ANSWER_MS", "3000"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let words = "Reply with exactly: BEFORE_THE_COMPACTION";
    let session = told(&rig, &project, words)?;
    // The human's words are in the record, and the answer is not.
    rig.wait_for("the human's words to be in the record", secs(60), || {
        Ok(rig.transcript(&session).contains(words))
    })?;
    let claude = rig
        .var("CLAUDE_CONFIG_DIR")
        .ok_or("the rig gives Claude no configuration")?
        .to_owned();
    let file = Path::new(&claude)
        .join("projects")
        .join("integration")
        .join(format!("{session}.jsonl"));
    let mut record = Stalling {
        record: OpenOptions::new().append(true).open(file)?,
        stall: Duration::from_secs(6),
        stalled: false,
    };
    write_compaction(&mut record, &session)?;

    rig.wait_for("the summary and the answer to be copied", secs(60), || {
        let read = history(&rig, &project)?;
        Ok(says(&read, "user", SUMMARY) && says(&read, "assistant", "BEFORE_THE_COMPACTION"))
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn the_wait_for_a_turn_s_end_returns_when_the_stand_in_has_answered() -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_ANSWER_MS", "3000"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let words = "Reply with exactly: THE_ANSWER";
    let session = told(&rig, &project, words)?;
    wait_for_the_turn_to_end(&rig, &session, words)?;
    let record = rig.transcript(&session);
    assert!(
        record.contains(r#""text":"THE_ANSWER""#),
        "the wait returned before the answer: {record}"
    );
    rig.close()?;
    Ok(())
}
