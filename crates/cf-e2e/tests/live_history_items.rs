//! Live: what each harness's reader makes of a real chief's conversation for the history
//! (chief-harness CHF-19). It is opt-in (`cargo xtask live history-items`): it spends the
//! machine's quota on a chief of each harness named, a few turns of its cheap model, and
//! needs each installed and logged in, so it is ignored unless asked for and is no part of
//! `check` or of CI.
//!
//! A real harness is the chief of a project on the native daemon, in the kit's home of the
//! run's own. The human asks it, in its window, to think, read a file, run a command that
//! fails and edit the file; then types `/compact` (the harness's own command; no other is
//! typed). The check reads the chief's history as the history modal does (`history.get`,
//! the tools' outputs too) and holds each reader to what its harness's record says
//! ([`Expect`]): the calls as tool items with their headers (`read …`, `$ ls …`, an edit's),
//! the failed command failed, an edit's change, thinking where the model thought, and the
//! compaction. What a harness's record cannot say is an honest gap ([`Expect`]), and said.
//!
//! ```text
//! cargo xtask live history-items                          every harness, one after the other
//! cargo xtask live history-items claude pi                those named
//! cargo xtask live history-items claude --keep            keeps the run's homes, records too
//! ```

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::members::{find, member_as, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, Harness, Log};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// The stand-in, which no window of this check runs: the kit asks for one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the folder tells the chief.
const BRIEF: &str = "This folder is an automated ConsensFlow test. Do only what the human asks in this window. When a ConsensFlow message arrives, reply with exactly one line, SEEN, and run no tools.\n";

/// The file the chief reads and edits.
const NOTES: &str = "notes.txt";
const NOTES_TEXT: &str = "The light is red.\nThe door is shut.\n";

/// What the human asks: a thought, a read, a command that fails and an edit, then a line.
const ASK: &str = "Think it through first. Then do these three things with your tools, one tool call each, in this order: 1) read the file notes.txt; 2) run the shell command `ls no-such-folder` (it fails, which is expected); 3) edit notes.txt so that the word red reads blue. Then reply with exactly one line: DONE.";

/// How long a turn has, and how often the history is looked at.
const TURN: Duration = Duration::from_secs(420);
const LOOK: Duration = Duration::from_secs(2);

/// What each harness's record says, and so what its reader is held to: a call's status
/// (Devin's record says none), the change an edit made, and a compaction (Devin's record
/// keeps none: its compacted chain keeps the steps' ids, and no message says it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Expect {
    status: bool,
    diff: bool,
    compaction: bool,
}

fn expect(harness: Harness) -> Expect {
    match harness {
        Harness::Claude | Harness::Codex | Harness::Pi | Harness::Muse => Expect {
            status: true,
            diff: true,
            compaction: true,
        },
        Harness::Devin => Expect {
            status: false,
            diff: false,
            compaction: false,
        },
    }
}

/// The chief's pane, as the pane host takes it: its newest generation.
fn chief_pane(members: &Members) -> Value {
    let id = format!("p{}-chief", members.project_id());
    let generation = members
        .rig()
        .open_frames()
        .into_iter()
        .rev()
        .find(|frame| frame["id"] == id.as_str())
        .and_then(|frame| frame["generation"].as_u64())
        .unwrap_or(1);
    json!({ "id": id, "generation": generation })
}

/// The human types `words` into the chief's window and sends them: Enter is a key of its
/// own, pressed a moment after the words, as a person presses it.
fn says(members: &Members, words: &str) -> Result<()> {
    let pane = chief_pane(members);
    members.rig().types(&pane, words.as_bytes())?;
    std::thread::sleep(Duration::from_millis(700));
    members.rig().types(&pane, b"\r")
}

/// The chief's whole history, oldest first, as the modal reads it (the tools' outputs too),
/// a page at a time from the newest back.
fn history(members: &Members) -> Result<Vec<Value>> {
    let mut items = Vec::new();
    let mut before: Option<Value> = None;
    for _ in 0..50 {
        let mut ask = json!({ "handle": "chief", "tools": true });
        if let Some(key) = &before {
            ask["before"] = key.clone();
        }
        let page = members.page("history.get", ask)?;
        let page = &page["history"];
        let mut shown = page["items"].as_array().cloned().unwrap_or_default();
        shown.extend(items);
        items = shown;
        match &page["older"] {
            Value::Null => break,
            key => before = Some(key.clone()),
        }
    }
    Ok(items)
}

/// What the history shows of kind `kind`.
fn of_kind<'a>(items: &'a [Value], kind: &str) -> Vec<&'a Value> {
    items.iter().filter(|item| item["kind"] == kind).collect()
}

/// An item in a line, for the log: its kind, its header or the start of its words.
fn line(item: &Value) -> String {
    format!(
        "{} {} {}{}",
        item["kind"].as_str().unwrap_or("plain"),
        item["role"].as_str().unwrap_or_default(),
        item["header"].as_str().map_or_else(
            || live::cut(item["text"].as_str().unwrap_or_default(), 60),
            str::to_owned
        ),
        item["status"]
            .as_str()
            .map_or_else(String::new, |status| format!(" [{status}]"))
    )
}

/// What the check holds of the history of a chief on `harness`.
fn judge(harness: Harness, items: &[Value]) -> Vec<Check> {
    let who = harness.name();
    let expect = expect(harness);
    let tools = of_kind(items, "tool");
    let header = |words: &str| {
        tools.iter().find(|item| {
            item["header"]
                .as_str()
                .is_some_and(|header| header.contains(words))
        })
    };
    let read = header(NOTES).filter(|item| {
        item["header"]
            .as_str()
            .is_some_and(|header| !header.starts_with("edit"))
    });
    let failed = header("no-such-folder");
    let edited = tools.iter().find(|item| {
        item["diff"].is_string()
            || item["header"]
                .as_str()
                .is_some_and(|header| header.starts_with("edit") || header.contains("apply_patch"))
    });
    let mut checks = vec![
        Check::new(
            who,
            "each call is a tool item with a header",
            !tools.is_empty() && tools.iter().all(|item| item["header"].is_string()),
            format!(
                "{} tool items: {:?}",
                tools.len(),
                tools.iter().map(|item| line(item)).collect::<Vec<_>>()
            ),
        ),
        Check::new(
            who,
            "the read is headed with its file",
            read.is_some(),
            format!("{:?}", read.map(|item| line(item))),
        ),
        Check::new(
            who,
            "the command that failed says so",
            failed.is_some_and(|item| !expect.status || item["status"] == "failed"),
            format!("{:?}", failed.map(|item| line(item))),
        ),
        Check::new(
            who,
            "the edit says its change",
            edited.is_some_and(|item| !expect.diff || item["diff"].is_string()),
            format!(
                "{:?}",
                edited.map(|item| format!(
                    "{} diff {:?}",
                    line(item),
                    item["diff"].as_str().map(|diff| live::cut(diff, 80))
                ))
            ),
        ),
        Check::new(
            who,
            "the compaction is a compaction",
            !expect.compaction || !of_kind(items, "compaction").is_empty(),
            format!(
                "{:?}",
                of_kind(items, "compaction")
                    .iter()
                    .map(|item| format!("tokens {:?}", item["tokens"]))
                    .collect::<Vec<_>>()
            ),
        ),
    ];
    let thinking = of_kind(items, "thinking");
    // Thinking is the model's, which a cheap model may skip: said, and held only to never
    // being taken for the agent's words.
    checks.push(Check::new(
        who,
        "thinking, where the model thought, is thinking and no words of the agent's",
        thinking.iter().all(|item| item["role"] != "assistant"),
        format!("{} thinking items", thinking.len()),
    ));
    checks
}

/// One chief on `harness`: the human's ask, then `/compact`, then the history.
fn run(log: &Log, harness: Harness) -> Result<Vec<Check>> {
    let plan = Plan::new(
        &format!("history-items-{}", harness.name()),
        member_as(harness, "chief"),
    )
    .told(BRIEF);
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    std::fs::write(members.folder().join(NOTES), NOTES_TEXT)
        .map_err(|failed| Error::Daemon(format!("{NOTES}: {failed}")))?;
    if !members.chief_is_idle(log)? {
        let _ = members.close();
        return Err(Error::Daemon(
            "the chief's window never went idle".to_owned(),
        ));
    }
    says(&members, ASK)?;
    let done = find(log, "the chief's DONE", TURN, LOOK, || {
        let items = history(&members)?;
        let answered = items.iter().any(|item| {
            item["role"] == "assistant"
                && item["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("DONE"))
        });
        Ok(answered.then_some(()))
    })?;
    let mut checks = vec![Check::new(
        harness.name(),
        "the chief did what the human asked",
        done.is_some(),
        format!("in {} s at most", TURN.as_secs()),
    )];
    if done.is_some() && members.chief_is_idle(log)? {
        says(&members, "/compact")?;
        find(log, "the compaction", TURN, LOOK, || {
            Ok((!of_kind(&history(&members)?, "compaction").is_empty()).then_some(()))
        })?;
        let _ = members.chief_is_idle(log)?;
    }
    let items = history(&members)?;
    for item in &items {
        log.say(format!("  {}", line(item)));
    }
    checks.extend(judge(harness, &items));
    members.close()?;
    Ok(checks)
}

/// The harnesses named, every one where none is.
fn chiefs(named: &[String]) -> Result<Vec<Harness>> {
    if named.is_empty() {
        return Ok(Harness::ALL.to_vec());
    }
    named
        .iter()
        .map(|word| {
            Harness::from_name(word).ok_or_else(|| {
                Error::Daemon(format!(
                    "no chief is called {word}: {}",
                    Harness::ALL.map(Harness::name).join(", ")
                ))
            })
        })
        .collect()
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of each harness named; cargo xtask live history-items"]
fn each_readers_history_of_a_real_chief_holds_its_calls_thinking_and_compaction() -> Result {
    let log = Log::begin("live history-items")?;
    let mut checks = Vec::new();
    for harness in chiefs(&live::list("harness"))? {
        log.found(harness)?;
        checks.extend(
            run(&log, harness)
                .map_err(|error| Error::Daemon(format!("{}: {error}", harness.name())))?,
        );
    }
    report::conclude(&log, "what each check came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(header: &str, status: &str, diff: Option<&str>) -> Value {
        let mut item = json!({ "kind": "tool", "role": "tool", "text": "", "header": header, "status": status });
        if let Some(diff) = diff {
            item["diff"] = json!(diff);
        }
        item
    }

    fn held(checks: &[Check]) -> Vec<bool> {
        checks.iter().map(|check| check.ok).collect()
    }

    #[test]
    fn holds_a_history_with_the_calls_their_ends_a_change_and_a_compaction() {
        let items = vec![
            json!({ "role": "user", "text": ASK }),
            json!({ "kind": "thinking", "role": "custom", "text": "first the read" }),
            tool("read notes.txt", "ok", None),
            tool("$ ls no-such-folder", "failed", None),
            tool(
                "edit notes.txt",
                "ok",
                Some("-The light is red.\n+The light is blue."),
            ),
            json!({ "role": "assistant", "text": "DONE" }),
            json!({ "kind": "compaction", "role": "user", "text": "summary", "tokens": 12_000 }),
        ];
        assert_eq!(held(&judge(Harness::Claude, &items)), [true; 6]);
        // A command that failed and says it did not, an edit with no change, no compaction.
        let mut plain = items.clone();
        plain[3]["status"] = json!("ok");
        plain[4].as_object_mut().unwrap().remove("diff");
        plain.pop();
        assert_eq!(
            held(&judge(Harness::Pi, &plain)),
            [true, true, false, false, false, true]
        );
        // Devin's record says none of them: no gap of its is held against it.
        assert_eq!(held(&judge(Harness::Devin, &plain)), [true; 6]);
        // Thinking taken for the agent's words is never held.
        let mut taken = items;
        taken[1]["role"] = json!("assistant");
        assert!(!held(&judge(Harness::Claude, &taken))[5]);
    }
}
