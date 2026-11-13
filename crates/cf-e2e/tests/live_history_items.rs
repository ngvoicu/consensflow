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

/// What the human asks: a thought (`ultrathink` is Claude Code's word for thinking hard,
/// which a cheap model may otherwise skip), a read, a command that fails and an edit, then a
/// line.
const ASK: &str = "Ultrathink: think hard about the order first. Then do these three things with your tools, one tool call each, in this order: 1) read the file notes.txt; 2) run the shell command `ls no-such-folder` (it fails, which is expected); 3) edit notes.txt so that the word red reads blue. Then reply with exactly one line: DONE.";

/// What the run's own Pi is set to: to keep little of the newest when it compacts.
const PI_SETTINGS: &str = "{\"compaction\":{\"keepRecentTokens\":200}}\n";

/// How long a turn has, and how often the history is looked at.
const TURN: Duration = Duration::from_secs(420);
const COMPACTED: Duration = Duration::from_secs(180);
const LOOK: Duration = Duration::from_secs(2);

/// What each harness's record says, and so what its reader is held to: a call's status, the
/// change an edit made (Devin's store keeps the file before the edit, not the change), and a
/// compaction.
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
            status: true,
            diff: false,
            compaction: true,
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

/// Where a run asked to keep what it made (`--keep`) copies each harness's records, into a new
/// folder of its own there: the tree's own scratch, which the readers' fixtures are made from.
const RECORDS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../app/src-tauri/target/scratch/history-items"
);

/// The folders of the run's own a harness keeps its records in, by the variable the rig
/// gives its windows, and the folder below it.
fn record_folders(harness: Harness) -> &'static [(&'static str, &'static str)] {
    match harness {
        Harness::Claude => &[("CLAUDE_CONFIG_DIR", "projects")],
        Harness::Codex => &[("CODEX_HOME", "sessions")],
        Harness::Pi => &[("PI_CODING_AGENT_DIR", "sessions")],
        Harness::Devin => &[("XDG_DATA_HOME", "devin")],
        Harness::Muse => &[("XDG_DATA_HOME", "muse"), ("XDG_STATE_HOME", "muse")],
    }
}

/// Copies what `harness`'s windows recorded of the run into [`RECORDS`], where `--keep` was
/// asked: the folders are the run's own, and go with it.
fn keep_records(members: &Members, harness: Harness, log: &Log) {
    if !live::keep_requested() {
        return;
    }
    // The scratch folder is shared by every run that keeps records: a run makes a new folder of
    // its own in it (`<harness>-<pid>-<ms>-<n>`), and never writes into one that is there.
    let into = match live::export_folder(Some(RECORDS), harness.name()) {
        Ok(Some(into)) => into,
        Ok(None) => return,
        Err(failed) => {
            log.say(format!(
                "kept none of the records of {}: {failed}",
                harness.name()
            ));
            return;
        }
    };
    for (variable, below) in record_folders(harness) {
        let Some(home) = members.rig().var(variable) else {
            continue;
        };
        let from = Path::new(home).join(below);
        match copy_tree(&from, &into.join(variable).join(below)) {
            Ok(count) => log.say(format!(
                "kept {count} files of {} in {}",
                from.display(),
                into.display()
            )),
            Err(failed) => log.say(format!("kept none of {}: {failed}", from.display())),
        }
    }
}

/// `from`'s files copied into `to`, folders and all: how many.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<usize> {
    let mut count = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            count += copy_tree(&entry.path(), &target)?;
        } else if entry.file_type()?.is_file() {
            cf_e2e::door::make_dir(to)?;
            cf_e2e::door::copy_keeping_mode(entry.path(), &target)?;
            count += 1;
        }
    }
    Ok(count)
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
fn judge(harness: Harness, items: &[Value], refused: bool) -> Vec<Check> {
    let who = harness.name();
    let mut expect = expect(harness);
    expect.compaction &= !refused;
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
            if refused {
                "the harness compacted nothing (its screen says so): no record for its reader"
                    .to_owned()
            } else {
                format!(
                    "{:?}",
                    of_kind(items, "compaction")
                        .iter()
                        .map(|item| format!("tokens {:?}", item["tokens"]))
                        .collect::<Vec<_>>()
                )
            },
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
    // Pi compacts nothing of a session shorter than what it keeps of the newest
    // (`keepRecentTokens`, 20,000 by default: "Nothing to compact (session too small)"); the
    // run's own Pi keeps little, so that `/compact` compacts a few turns.
    let plan = if harness == Harness::Pi {
        plan.with_home_file(
            Harness::Pi,
            "PI_CODING_AGENT_DIR",
            "settings.json",
            PI_SETTINGS,
        )
    } else {
        plan
    };
    let members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    cf_e2e::door::write(members.folder().join(NOTES), NOTES_TEXT)
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
    // Whether the harness itself refused to compact ([`refused_to_compact`]): its record then
    // holds no compaction for its reader to read.
    let mut refused = false;
    if done.is_some() && members.chief_is_idle(log)? {
        says(&members, "/compact")?;
        // Pi's picker takes the first Enter for the command it lists, and runs it on the second.
        if harness == Harness::Pi {
            std::thread::sleep(Duration::from_secs(2));
            members.rig().types(&chief_pane(&members), b"\r")?;
        }
        let compacted = find(log, "the compaction", COMPACTED, LOOK, || {
            Ok((!of_kind(&history(&members)?, "compaction").is_empty()).then_some(()))
        })?;
        if compacted.is_none() {
            let pane = format!("p{}-chief", members.project_id());
            let screen = members.tail(&pane);
            log.say(format!("no compaction; the chief's screen: {screen}"));
            refused = refused_to_compact(&screen);
        }
        let _ = members.chief_is_idle(log)?;
    }
    let items = history(&members)?;
    for item in &items {
        log.say(format!("  {}", line(item)));
    }
    checks.extend(judge(harness, &items, refused));
    keep_records(&members, harness, log);
    members.close()?;
    Ok(checks)
}

/// Whether a window's screen says its harness refused to compact, in the words each wrote
/// live (T-728): Pi 1.4.0's "Nothing to compact (session too small)", Muse Code 1.4.14's
/// "no compactable run is available".
fn refused_to_compact(screen: &str) -> bool {
    let screen = screen.to_lowercase();
    ["nothing to compact", "no compactable run"]
        .iter()
        .any(|words| screen.contains(words))
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
    fn the_records_a_run_keeps_go_in_a_new_folder_of_its_own_and_an_earlier_runs_are_as_they_were()
    {
        use cf_e2e::live::layout::Layout;
        let mut layout = Layout::new().unwrap();
        let scratch = layout.home().join("scratch");
        layout
            .plant_tree(
                &scratch,
                &[(
                    "claude-1-2-0/CLAUDE_CONFIG_DIR/projects/a.jsonl",
                    "an earlier run's record",
                )],
            )
            .unwrap();
        // The harness's records the run made, a folder with a file below a folder.
        let made = cf_e2e::door::made("live-history-items-test-").unwrap();
        cf_e2e::door::make_dir(made.path().join("projects/p")).unwrap();
        cf_e2e::door::write(made.path().join("projects/p/s.jsonl"), "this run's record").unwrap();
        for _ in 0..2 {
            let into = live::export_folder(Some(&scratch.display().to_string()), "claude")
                .unwrap()
                .unwrap();
            let kept = copy_tree(
                &made.path().join("projects"),
                &into.join("CLAUDE_CONFIG_DIR/projects"),
            );
            assert_eq!(kept.unwrap(), 1);
            assert_eq!(
                std::fs::read_to_string(into.join("CLAUDE_CONFIG_DIR/projects/p/s.jsonl")).unwrap(),
                "this run's record"
            );
        }
        // The earlier run's record is as it was, and each run's copy is in a folder of its own.
        layout.untouched().unwrap();
        let children = std::fs::read_dir(&scratch).unwrap().count();
        assert_eq!(
            children, 3,
            "the earlier run's folder and one for each of two runs"
        );
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
        assert_eq!(held(&judge(Harness::Claude, &items, false)), [true; 6]);
        // A command that failed and says it did not, an edit with no change, no compaction.
        let mut plain = items.clone();
        plain[3]["status"] = json!("ok");
        plain[4].as_object_mut().unwrap().remove("diff");
        plain.pop();
        assert_eq!(
            held(&judge(Harness::Pi, &plain, false)),
            [true, true, false, false, false, true]
        );
        // Devin's store says no edit's change: that gap of its is not held against it.
        assert_eq!(
            held(&judge(Harness::Devin, &plain, false)),
            [true, true, false, true, false, true]
        );
        assert!(refused_to_compact(
            "DONE ⏎ Error: Compaction failed: Nothing to compact (session too small)"
        ));
        assert!(refused_to_compact(
            "MM8 ⏎ no compactable run is available ⏎ ❯"
        ));
        assert!(!refused_to_compact("Compacted. ⏎ ❯"));
        // A compaction the harness refused is no reader's to show.
        assert!(held(&judge(Harness::Pi, &plain, true))[4]);
        // Thinking taken for the agent's words is never held.
        let mut taken = items;
        taken[1]["role"] = json!("assistant");
        assert!(!held(&judge(Harness::Claude, &taken, false))[5]);
    }
}
