//! Who says that a Pi window is at work (the brain's `architecture/pi-turn-state.md`, §8): the
//! sessions and words Pi's own loop wrote, read with the word and without it.
//!
//! The fixtures in `tests/fixtures/pi-turn-state/<Pi>/<case>-<form>/<moment>/` were written by
//! `tests/pi-loop/drive.mjs --record` over the published Pi 0.87.0, 1.1.0, 1.6.0 and 1.6.1
//! (ConsensFlow's extension loaded from the command line, a person's from the settings, Pi's own
//! stand-in for the model): each moment's session file as Pi wrote it, the word the extension had
//! written by then (on 0.87.0 and 1.1.0, once a run had begun, its mark that the launch has no
//! word), and what the driver saw (`expect.json`). Each case is in two forms: plain, and with a
//! person's extension's own entries inside the open turn and after its end.

use std::path::Path as FsPath;

use super::*;

const LAUNCH: &str = "launch-loop";

/// A recorded moment: its name, its session's text (none where Pi had written no file), the
/// word (none where the extension had written none), and what the driver saw.
struct Moment {
    name: String,
    text: Option<String>,
    word: Option<Value>,
    expect: Value,
}

impl Moment {
    /// Whether the extension had written a word by then: a state, not its mark of a launch with
    /// none.
    fn worded(&self) -> bool {
        self.word
            .as_ref()
            .is_some_and(|word| word.get("state").is_some())
    }
}

fn moments() -> Vec<Moment> {
    let root = FsPath::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi-turn-state");
    let mut all = Vec::new();
    let each = |dir: &FsPath| {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        entries
    };
    for version in each(&root) {
        for case in each(&version) {
            for moment in each(&case) {
                let read = |name: &str| fs::read_to_string(moment.join(name)).ok();
                all.push(Moment {
                    // `<Pi>/<case>-<form>/<moment>`, with `/` on every platform.
                    name: moment
                        .strip_prefix(&root)
                        .unwrap()
                        .components()
                        .map(|part| part.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/"),
                    text: read("hazy-ridge.jsonl"),
                    word: read(&format!("{LAUNCH}.state.json")).map(|w| {
                        // The stage's session: the word is of the session it reads.
                        let mut word: Value = serde_json::from_str(&w).unwrap();
                        word["sessionId"] = json!(SESSION);
                        word
                    }),
                    expect: serde_json::from_str(&read("expect.json").unwrap()).unwrap(),
                });
            }
        }
    }
    assert!(all.len() >= 120, "the recorded moments: {}", all.len());
    all
}

/// A stage holding `text` (none: no file), and the options that name the launch.
fn staged(text: Option<&str>) -> (Stage, Options) {
    let mut stage = Stage::new();
    if let Some(text) = text {
        stage.write_text(text);
    }
    let options = evidence_files::options(&stage, LAUNCH);
    (stage, options)
}

/// [`staged`], with a reader that finds the process that wrote the word gone.
fn staged_gone(text: Option<&str>) -> (Stage, Options) {
    let mut stage = Stage::gone();
    if let Some(text) = text {
        stage.write_text(text);
    }
    let options = evidence_files::options(&stage, LAUNCH);
    (stage, options)
}

fn put_word(stage: &Stage, word: &Value) {
    evidence_files::put(stage, &format!("{LAUNCH}.state.json"), word.to_string());
}

/// The words of the answer: the assistant's items since the last user message that are not
/// left out of it.
fn answer(record: &Record) -> Vec<String> {
    let from = record
        .items
        .iter()
        .rposition(|item| item.role == Role::User)
        .map_or(0, |at| at + 1);
    record.items[from..]
        .iter()
        .filter(|item| item.role == Role::Assistant && !item.synthetic && !item.text.is_empty())
        .map(|item| item.text.to_string())
        .collect()
}

/// Whether the file's last message is the user's or a tool's result: a request Pi has to make.
fn tail_asks(text: &str) -> bool {
    let role = text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| row["type"] == "message")
        .map(|row| {
            row["message"]["role"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .rfind(|role| ["user", "assistant", "toolResult"].contains(&role.as_str()));
    matches!(role.as_deref(), Some("user" | "toolResult"))
}

/// What the expectation says of how a run ended, against a reading at rest.
fn ended_as(record: &Record, expect: &Value, what: &str) {
    match expect["ended"].as_str() {
        Some("stop") => assert!(!record.failed && !record.aborted, "{what}: {record:?}"),
        Some("aborted") => assert!(record.aborted && !record.failed, "{what}: {record:?}"),
        Some("failed") => {
            assert!(record.failed && !record.aborted, "{what}: {record:?}");
            assert_eq!(
                record.failure.as_deref(),
                expect["failure"].as_str(),
                "{what}"
            );
        }
        _ => {}
    }
    if let Some(words) = expect["words"].as_str() {
        assert_eq!(
            answer(record),
            vec![words.to_owned()],
            "{what}: only the retry's words"
        );
    }
    if expect["quota"] == json!(true) {
        assert!(
            record.quota.is_some(),
            "{what}: the refusal is published: {record:?}"
        );
    }
    if expect["items"] == json!(0) {
        assert!(answer(record).is_empty(), "{what}: {record:?}");
    }
}

#[test]
fn every_recorded_moment_reads_with_the_word_as_pi_said_and_without_it_by_the_clock() {
    for moment in moments() {
        let what = &moment.name;
        // With the word, the moment it was written: what Pi said, at once.
        if moment.worded() {
            with_the_word(&moment);
        } else if let Some(word) = &moment.word {
            // The mark of a launch with no word (Pi 0.87.0 and 1.1.0 once a run began): read as no
            // word at all.
            assert_eq!(word["late"], true, "{what}: {word}");
            if moment.text.is_none() {
                continue;
            }
        }
        // Without the word (an extension that did not load, a window an older ConsensFlow
        // opened, a Pi that sets its run flag late): the file, by its entries' times.
        let Some(text) = moment.text.as_deref() else {
            continue;
        };
        let (mut stage, options) = staged(Some(text));
        if let Some(word) = moment.word.as_ref().filter(|_| !moment.worded()) {
            put_word(&stage, word);
        }
        let early = known(&stage.reader.look(&options, newest(text) + 1_000));
        assert!(
            early.in_flight && early.settlement == Settlement::InFlight,
            "{what}, no word, 1 s: {early:?}"
        );
        let late = known(&stage.reader.look(&options, newest(text) + 121_000));
        if tail_asks(text) {
            assert!(
                late.in_flight && late.lines.settles_at.is_none(),
                "{what}, no word: a request to make: {late:?}"
            );
            continue;
        }
        // No call of an aborted or failed step holds it open, and the quiet ends it as the word
        // would have.
        assert!(
            late.settlement == Settlement::Settled && !late.in_flight,
            "{what}, no word, 121 s: {late:?}"
        );
        // A person's `/tree` back to another step is read along its branch only with the word;
        // without it the file's last step stands (a limit of a window with no word).
        if moment.expect["at"] == "rest" && !what.contains("/tree") {
            ended_as(&late, &moment.expect, &format!("{what}, no word"));
        }
    }
}

/// A moment read with the word as the extension wrote it, the moment it was written: what Pi said,
/// at once; and, for an end being written, once the process that wrote the word is gone.
fn with_the_word(moment: &Moment) {
    let what = &moment.name;
    {
        let (mut stage, options) = staged(moment.text.as_deref());
        put_word(&stage, moment.word.as_ref().unwrap());
        let now = moment.text.as_deref().map_or(WRITTEN, newest) + 1_000;
        let record = known(&stage.reader.look(&options, now));
        // A run's end being written (Pi cleared its run; the word says `working` with as many
        // ends as runs): neither, whatever the driver saw.
        let writing_an_end = moment
            .word
            .as_ref()
            .is_some_and(|word| word["state"] == "working" && word["begun"] == word["ended"]);
        let at = if writing_an_end {
            "neither"
        } else {
            moment.expect["at"].as_str().unwrap()
        };
        match at {
            "rest" => {
                assert!(
                    record.settlement == Settlement::Settled
                        && !record.in_flight
                        && !record.neither(),
                    "{what}: at rest: {record:?}"
                );
                ended_as(&record, &moment.expect, what);
            }
            "work" => assert!(
                record.in_flight
                    && record.settlement == Settlement::InFlight
                    && record.lines.said_at_work,
                "{what}: at work: {record:?}"
            ),
            "neither" => assert!(record.neither() && !record.in_flight, "{what}: {record:?}"),
            other => panic!("{what}: {other}"),
        }
    }
    // An end being written, and the window's process gone before the next pass: the turn is over,
    // read as at rest (finding 16).
    if let Some(gone) = moment.expect["gone"].as_str() {
        let mut word = moment.word.clone().unwrap();
        // Its process alive (this test's own): the word stands.
        word["pid"] = json!(std::process::id());
        let (mut stage, options) = staged(moment.text.as_deref());
        put_word(&stage, &word);
        let now = moment.text.as_deref().map_or(WRITTEN, newest) + 1_000;
        let record = known(&stage.reader.look(&options, now));
        assert!(record.neither(), "{what}: its process alive: {record:?}");
        let (mut stage, options) = staged_gone(moment.text.as_deref());
        put_word(&stage, &word);
        let now = moment.text.as_deref().map_or(WRITTEN, newest) + 1_000;
        let record = known(&stage.reader.look(&options, now));
        assert!(
            record.settlement == Settlement::Settled && !record.in_flight && !record.neither(),
            "{what}: gone, at rest: {record:?}"
        );
        let mut expect = moment.expect.clone();
        expect["ended"] = json!(gone);
        ended_as(&record, &expect, &format!("{what}, gone"));
    }
}

/// The recorded moments of one case with the word, in both versions and forms.
fn case(name: &str) -> Vec<Moment> {
    let moments: Vec<Moment> = moments()
        .into_iter()
        .filter(|m| {
            m.worded()
                && m.name
                    .split('/')
                    .nth(1)
                    .is_some_and(|c| c.starts_with(&format!("{name}-")))
        })
        .collect();
    assert!(moments.len() >= 4, "{name}");
    moments
}

/// The rows of a session's text.
fn rows(text: &str) -> Vec<Value> {
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_run_begun_before_a_look_goes_on_with_the_turn_and_an_end_being_written_is_neither() {
    // D3, finding 4: a run's answer, `agent_settled`, then a run a person's extension triggered
    // from it, ended too before any look. The present is the word's alone: `working` with as many
    // ends as runs is an end being written, neither. At rest, the turn is read along the selected
    // leaf, and its words are the member's since the latest message: both runs' answers.
    for moment in case("triggered-after-settle") {
        let what = &moment.name;
        let text = moment.text.as_deref().unwrap();
        let word = moment.word.clone().unwrap();
        assert_eq!(
            (word["begun"].clone(), word["ended"].clone()),
            (json!(2), json!(2)),
            "{what}"
        );
        let (mut stage, options) = staged(Some(text));
        let now = newest(text) + 1_000;
        put_word(&stage, &word);
        let writing = known(&stage.reader.look(&options, now));
        assert!(
            writing.neither() && !writing.in_flight && writing.quota.is_none(),
            "{what}: {writing:?}"
        );
        let mut rested = word.clone();
        rested["state"] = json!("rest");
        put_word(&stage, &rested);
        let rest = known(&stage.reader.look(&options, now));
        assert!(
            rest.settlement == Settlement::Settled && !rest.failed && !rest.aborted,
            "{what}: {rest:?}"
        );
        assert_eq!(
            answer(&rest),
            vec![
                "First answer done.".to_owned(),
                "Automatically continued.".to_owned()
            ],
            "{what}"
        );
    }
    // Finding 8 of round one: a retry cancelled in its backoff, then a run triggered from its end:
    // run 2 at work, and no refusal or result of run 1 is read beside it.
    for moment in case("cancelled-then-triggered") {
        let what = &moment.name;
        let text = moment.text.as_deref().unwrap();
        let (mut stage, options) = staged(Some(text));
        put_word(&stage, moment.word.as_ref().unwrap());
        let look = known(&stage.reader.look(&options, newest(text) + 1_000));
        assert!(
            look.in_flight && look.lines.said_at_work && look.quota.is_none() && !look.failed,
            "{what}: run 2 is on: {look:?}"
        );
    }
}

#[test]
fn a_rest_whose_leaf_the_file_lacks_is_paid_and_takes_nothing_until_it_has_it() {
    // Findings 3 (its second half) and 8: the word rests at the last end's leaf, which the file as
    // read has not yet: rested (the stop is paid with no key; no result, no message) until the
    // file holds it. No file at all with an end named is the same; a new session with no file
    // and no end is at rest with nothing to read.
    for moment in case("stop")
        .into_iter()
        .filter(|m| m.name.ends_with("answered"))
    {
        let what = &moment.name;
        let text = moment.text.as_deref().unwrap();
        let word = moment.word.clone().unwrap();
        let leaf = word["last"]["leaf"].as_str().unwrap().to_owned();
        let lines: Vec<&str> = text.lines().collect();
        let cut = lines
            .iter()
            .position(|line| line.contains(&format!("\"id\":\"{leaf}\"")))
            .unwrap();
        let behind = format!("{}\n", lines[..cut].join("\n"));
        let now = newest(text) + 1_000;
        for file in [None, Some(behind.as_str())] {
            let (mut stage, options) = staged(file);
            put_word(&stage, &word);
            let early = known(&stage.reader.look(&options, now));
            assert!(
                early.lines.rested
                    && early.settlement != Settlement::Settled
                    && !early.in_flight
                    && early.quota.is_none(),
                "{what}: {early:?}"
            );
            stage.write_text(text);
            let caught = known(&stage.reader.look(&options, now));
            assert!(
                caught.settlement == Settlement::Settled && !caught.lines.rested,
                "{what}: {caught:?}"
            );
            assert_eq!(answer(&caught), vec!["Done.".to_owned()], "{what}");
        }
    }
    for moment in case("new-session") {
        let (mut stage, options) = staged(moment.text.as_deref());
        assert!(moment.text.is_none() && moment.word.as_ref().unwrap()["last"].is_null());
        put_word(&stage, moment.word.as_ref().unwrap());
        let record = known(&stage.reader.look(&options, WRITTEN));
        assert!(
            record.settlement == Settlement::Settled
                && !record.lines.rested
                && record.items.is_empty(),
            "{}: {record:?}",
            moment.name
        );
    }
}

#[test]
fn an_omission_is_read_along_the_selected_leaf_only() {
    // Finding 5: Pi's omissions apply along the selected leaf's own parents. An omission of the
    // first run's answer on another branch changes nothing; one on the selected chain leaves that
    // step's words out.
    for moment in case("triggered-after-settle") {
        let what = &moment.name;
        let text = moment.text.as_deref().unwrap();
        let mut word = moment.word.clone().unwrap();
        word["state"] = json!("rest");
        let all = rows(text);
        let first = all
            .iter()
            .find(|row| row["message"]["role"] == "assistant")
            .unwrap()["id"]
            .clone();
        // Off the selected chain: a child of the session's first entry.
        let root = all
            .iter()
            .find(|row| row["parentId"].is_null() && row["type"] != "session")
            .unwrap()["id"]
            .clone();
        let aside = json!({ "type": "context_edit", "id": "aside-edit", "parentId": root, "timestamp": "2026-11-27T10:00:00.000Z", "targetId": first, "replacement": null });
        let (mut stage, options) = staged(Some(&format!("{text}{aside}\n")));
        put_word(&stage, &word);
        let record = known(&stage.reader.look(&options, newest(text) + 1_000));
        assert_eq!(
            answer(&record),
            vec![
                "First answer done.".to_owned(),
                "Automatically continued.".to_owned()
            ],
            "{what}: an omission on another branch"
        );
        // On the selected chain: the selected leaf is the omission.
        let on = json!({ "type": "context_edit", "id": "later-edit", "parentId": word["leaf"], "timestamp": "2026-11-27T10:00:00.000Z", "targetId": first, "replacement": null });
        let (mut stage, options) = staged(Some(&format!("{text}{on}\n")));
        word["leaf"] = json!("later-edit");
        put_word(&stage, &word);
        let record = known(&stage.reader.look(&options, newest(text) + 1_000));
        assert_eq!(
            answer(&record),
            vec!["Automatically continued.".to_owned()],
            "{what}: an omitted step gives no words"
        );
    }
}

#[test]
fn a_rest_after_a_persons_tree_navigation_is_read_along_its_branch() {
    // Finding 7, with the word: an answer, a later failure, then `/tree` back to the answer with
    // a label: the answer, not the abandoned branch's failure.
    for moment in case("tree") {
        let what = &moment.name;
        let text = moment.text.as_deref().unwrap();
        let (mut stage, options) = staged(Some(text));
        put_word(&stage, moment.word.as_ref().unwrap());
        let record = known(&stage.reader.look(&options, newest(text) + 1_000));
        if what.contains("/tree-abort-") {
            continue;
        }
        assert!(
            record.settlement == Settlement::Settled
                && !record.failed
                && !record.aborted
                && record.quota.is_none(),
            "{what}: {record:?}"
        );
        assert_eq!(answer(&record), vec!["First answer".to_owned()], "{what}");
    }
}

#[test]
fn the_last_ends_abort_counts_only_where_its_step_is_the_selected_one() {
    // Finding 17: a genuine final 400, a later run interrupted, then `/tree` back to the 400: the
    // selected step's own failure, not the abandoned run's abort.
    for moment in case("tree-abort") {
        let what = &moment.name;
        let text = moment.text.as_deref().unwrap();
        let word = moment.word.clone().unwrap();
        assert_eq!(word["last"]["aborted"], true, "{what}");
        let (mut stage, options) = staged(Some(text));
        put_word(&stage, &word);
        let record = known(&stage.reader.look(&options, newest(text) + 1_000));
        assert!(record.failed && !record.aborted, "{what}: {record:?}");
        assert_eq!(
            record.failure.as_deref(),
            Some("400 Invalid request."),
            "{what}"
        );
    }
    // A label after the same terminal step keeps its abort: the interrupted answer stays so.
    for moment in case("escape-text") {
        let what = &moment.name;
        let text = moment.text.as_deref().unwrap();
        let mut word = moment.word.clone().unwrap();
        let step = rows(text)
            .iter()
            .rev()
            .find(|row| row["message"]["role"] == "assistant")
            .unwrap()["id"]
            .clone();
        let label = json!({ "type": "label", "id": "a-label", "parentId": word["leaf"], "timestamp": "2026-11-27T10:00:00.000Z", "targetId": step, "label": "Kept" });
        let (mut stage, options) = staged(Some(&format!("{text}{label}\n")));
        word["leaf"] = json!("a-label");
        put_word(&stage, &word);
        let record = known(&stage.reader.look(&options, newest(text) + 1_000));
        assert!(record.aborted && !record.failed, "{what}: {record:?}");
    }
}

/// A step that a provider refused for a moment (`503`), as Pi writes it.
fn refused(id: &str) -> Value {
    message(
        id,
        json!({ "role": "assistant", "stopReason": "error", "errorMessage": "503 Service Unavailable: overloaded",
            "timestamp": 1_790_000_000_000_i64, "content": [{ "type": "text", "text": "partial" }] }),
    )
}

#[test]
fn a_refusal_is_published_only_at_rest_with_its_failure_and_never_inside_a_run() {
    // Findings 1 and 20: a refusal is the turn's only where the turn ended at it, at rest, with its
    // failure in the same look; inside a run that Pi goes on from (its retry) it is never
    // published; a refusal Pi omitted is none.
    let rows = vec![header(), user("u1", "go"), refused("a1")];
    let word = |state: &str, ended: &[(u64, Value, Value)]| -> Value {
        serde_json::from_str(&evidence_files::word(
            LAUNCH,
            SESSION,
            state,
            &json!("a1"),
            ended,
        ))
        .unwrap()
    };
    let mut stage = Stage::with(&rows);
    let options = evidence_files::options(&stage, LAUNCH);
    put_word(&stage, &word("working", &[]));
    let on = known(&stage.look(1_000, &options));
    assert!(
        on.in_flight && on.quota.is_none() && !on.failed,
        "inside the run: {on:?}"
    );
    let mut writing = word("working", &[(1, json!("a1"), json!(false))]);
    writing["begun"] = json!(1);
    put_word(&stage, &writing);
    let ending = known(&stage.look(1_000, &options));
    assert!(
        ending.neither() && ending.quota.is_none(),
        "an end being written: {ending:?}"
    );
    // At rest, with the file behind the last end: nothing of it yet.
    put_word(
        &stage,
        &word("rest", &[(1, json!("not-written-yet"), json!(false))]),
    );
    let behind = known(&stage.look(1_000, &options));
    assert!(
        behind.lines.rested && behind.quota.is_none() && !behind.failed,
        "the file behind: {behind:?}"
    );
    put_word(&stage, &word("rest", &[(1, json!("a1"), json!(false))]));
    let rest = known(&stage.look(1_000, &options));
    assert!(
        rest.quota.is_some() && rest.failed,
        "at rest, with its failure: {rest:?}"
    );
    // Pi's own omission of it (a retry cancelled in its backoff): no refusal, interrupted.
    let mut omitted = rows.clone();
    omitted.push(json!({ "type": "context_edit", "id": "e1", "timestamp": "2026-09-21T14:13:21.000Z", "targetId": "a1", "replacement": null }));
    let mut stage = Stage::with(&omitted);
    let options = evidence_files::options(&stage, LAUNCH);
    let mut cancelled = word("rest", &[(1, json!("e1"), json!(false))]);
    cancelled["leaf"] = json!("e1");
    put_word(&stage, &cancelled);
    let rest = known(&stage.look(1_000, &options));
    assert!(
        rest.quota.is_none() && !rest.failed && rest.aborted,
        "omitted: {rest:?}"
    );
}

#[test]
fn a_word_whose_process_is_gone_says_nothing_of_the_present() {
    // Finding 16: inside `agent_settled` the word says `working` with as many ends as runs; the
    // window gone before the next pass, that is a finished turn. A run still on, a word that is
    // busy, or an end whose leaf the file lacks, says nothing more for being gone.
    for moment in case("stop")
        .into_iter()
        .filter(|m| m.name.ends_with("settling"))
    {
        let what = &moment.name;
        let text = moment.text.as_deref().unwrap();
        let mut word = moment.word.clone().unwrap();
        word["pid"] = json!(4242);
        let now = newest(text) + 1_000;
        let (mut stage, options) = staged_gone(Some(text));
        put_word(&stage, &word);
        let over = known(&stage.reader.look(&options, now));
        assert!(
            over.settlement == Settlement::Settled && !over.neither(),
            "{what}: {over:?}"
        );
        assert_eq!(answer(&over), vec!["Done.".to_owned()], "{what}");
        let mut on = word.clone();
        on["begun"] = json!(2);
        put_word(&stage, &on);
        assert!(
            known(&stage.reader.look(&options, now)).in_flight,
            "{what}: a run on"
        );
        let mut busy = word.clone();
        busy["state"] = json!("busy");
        put_word(&stage, &busy);
        assert!(
            known(&stage.reader.look(&options, now)).neither(),
            "{what}: busy"
        );
        let mut missing = word.clone();
        missing["last"]["leaf"] = json!("not-written-yet");
        put_word(&stage, &missing);
        assert!(
            known(&stage.reader.look(&options, now)).lines.rested,
            "{what}: its leaf to come"
        );
    }
}

/// One entry of each kind Pi 1.6 can write to a session (`session-manager.d.ts`'s
/// `SessionEntry`, the roles of its `message` entries, and a kind from a later Pi), at `at`, and
/// whether a request can follow it.
fn kinds(at: &str) -> Vec<(&'static str, Value, bool)> {
    let entry = |kind: Value| {
        let mut entry = json!({ "id": "x1", "parentId": "a1", "timestamp": at });
        entry
            .as_object_mut()
            .unwrap()
            .extend(kind.as_object().unwrap().clone());
        entry
    };
    vec![
        (
            "compaction",
            entry(
                json!({ "type": "compaction", "summary": "User asked for X.", "firstKeptEntryId": "u1", "tokensBefore": 506298 }),
            ),
            true,
        ),
        (
            "branch_summary",
            entry(json!({ "type": "branch_summary", "fromId": "u1", "summary": "Tried A." })),
            true,
        ),
        (
            "custom",
            entry(json!({ "type": "custom", "customType": "keeper", "data": { "count": 42 } })),
            false,
        ),
        (
            "custom_message shown",
            entry(
                json!({ "type": "custom_message", "customType": "keeper", "content": "Cache kept warm.", "display": true }),
            ),
            true,
        ),
        (
            "custom_message hidden",
            entry(
                json!({ "type": "custom_message", "customType": "keeper", "content": [{ "type": "text", "text": "context" }], "display": false }),
            ),
            true,
        ),
        (
            "label",
            entry(json!({ "type": "label", "targetId": "u1", "label": "checkpoint" })),
            false,
        ),
        (
            "session_info",
            entry(json!({ "type": "session_info", "name": "Refactor" })),
            false,
        ),
        (
            "thinking_level_change",
            entry(json!({ "type": "thinking_level_change", "thinkingLevel": "high" })),
            false,
        ),
        (
            "model_change",
            entry(json!({ "type": "model_change", "provider": "openai", "modelId": "gpt-4o" })),
            false,
        ),
        (
            "usage cache_warm",
            entry(
                json!({ "type": "usage", "kind": "cache_warm", "provider": "anthropic", "model": "m", "usage": { "input": 0, "output": 0, "cacheRead": 50000, "cacheWrite": 0, "totalTokens": 50000 } }),
            ),
            false,
        ),
        (
            "context_edit",
            entry(json!({ "type": "context_edit", "targetId": "u1", "replacement": null })),
            true,
        ),
        (
            "message system",
            entry(
                json!({ "type": "message", "message": { "role": "system", "content": "", "toolsAdded": [], "timestamp": 1 } }),
            ),
            true,
        ),
        (
            "message bashExecution",
            entry(
                json!({ "type": "message", "message": { "role": "bashExecution", "command": "ls", "output": "a", "exitCode": 0, "cancelled": false, "truncated": false, "timestamp": 1 } }),
            ),
            true,
        ),
        (
            "a kind from a later Pi",
            entry(json!({ "type": "future_kind", "data": 1 })),
            false,
        ),
    ]
}

#[test]
fn an_entry_that_asks_the_model_nothing_changes_nothing_and_one_a_request_can_follow_starts_the_clock(
) {
    // Notes 10 and D6, after a turn that ended in its answer and one an Escape ended with a call
    // begun, with and without the word.
    let ended = [
        (
            "stop",
            vec![header(), user("u1", "go"), assistant("a1", "stop", "done")],
            json!(false),
        ),
        (
            "aborted",
            include_str!("../../../../tests/fixtures/pi-interrupted/while-tool-arguments.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect(),
            json!(true),
        ),
    ];
    for (how, base, flag) in ended {
        let last = base.last().unwrap();
        let step_at = cf_base::time::parse(last["timestamp"].as_str().unwrap()).unwrap();
        // The person's extension writes 200 s after the step: a triggered message, an omission,
        // a compaction at rest, or something that asks nothing.
        let later = step_at + 200_000;
        let later_text = jiff::Timestamp::from_millisecond(later)
            .unwrap()
            .to_string();
        for (kind, extra, asks) in kinds(&later_text) {
            let mut rows = base.clone();
            rows.push(extra);
            let what = format!("{kind} after a turn that ended in {how}");
            // The word at rest named the step (the person's extension wrote after our poll), or
            // the entry itself: at rest, ended as it ended.
            for leaf in [last["id"].clone(), json!("x1")] {
                let mut stage = Stage::with(&rows);
                let options = evidence_files::options(&stage, LAUNCH);
                put_word(
                    &stage,
                    &serde_json::from_str(&evidence_files::word(
                        LAUNCH,
                        SESSION,
                        "rest",
                        &leaf,
                        &[(1, last["id"].clone(), flag.clone())],
                    ))
                    .unwrap(),
                );
                let record = known(&stage.reader.look(&options, later + 1_000));
                assert!(
                    record.settlement == Settlement::Settled && !record.in_flight,
                    "{what}, word at {leaf}: {record:?}"
                );
                assert_eq!(record.aborted, how == "aborted", "{what}");
                assert!(!record.failed, "{what}");
            }
            // No word: an entry a request can follow starts the clock again (a turn another
            // extension triggers, a continuation whose only entry is an omission); any other
            // leaves it at the step, long passed.
            let mut stage = Stage::with(&rows);
            let record = stage.at_time(later + 1_000);
            if asks {
                assert!(
                    record.in_flight && record.lines.settles_at == Some(later + 120_000),
                    "{what}, no word: {record:?}"
                );
                assert_eq!(
                    stage.at_time(later + 120_000).settlement,
                    Settlement::Settled,
                    "{what}"
                );
            } else {
                assert!(
                    record.settlement == Settlement::Settled && !record.in_flight,
                    "{what}, no word: {record:?}"
                );
            }
        }
    }
}

#[test]
fn only_pis_own_omission_of_the_failed_step_makes_a_failure_an_interrupt() {
    // D4 and note 11: `user`, `error`, Pi's flag false at `agent_settled`.
    for moment in case("final-error") {
        let text = moment.text.as_deref().unwrap();
        let word = moment.word.clone().unwrap();
        let rows: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let user = rows
            .iter()
            .rev()
            .find(|row| row["message"]["role"] == "user")
            .unwrap()["id"]
            .clone();
        let failed = rows
            .iter()
            .rev()
            .find(|row| row["message"]["stopReason"] == "error")
            .unwrap()["id"]
            .clone();
        let edit = |target: &Value, replacement: Value| json!({ "type": "context_edit", "id": "e9", "parentId": rows.last().unwrap()["id"], "timestamp": rows.last().unwrap()["timestamp"], "targetId": target, "replacement": replacement });
        for (edits, interrupted, why) in [
            (vec![], false, "no omission: failed"),
            (
                vec![edit(&user, json!({ "content": "Shorter context" }))],
                false,
                "an edit of the user's entry: failed",
            ),
            (
                vec![edit(&failed, json!({ "content": "Shorter" }))],
                false,
                "the step replaced, not omitted: failed",
            ),
            (
                vec![edit(&user, Value::Null)],
                false,
                "another entry omitted: failed",
            ),
            (
                vec![edit(&failed, Value::Null)],
                true,
                "Pi's omission of that very step: a retry cancelled",
            ),
        ] {
            let mut all = rows.clone();
            all.extend(edits);
            let mut stage = Stage::with(&all);
            let options = evidence_files::options(&stage, LAUNCH);
            let mut word = word.clone();
            word["leaf"] = all.last().unwrap()["id"].clone();
            put_word(&stage, &word);
            let record = known(&stage.reader.look(&options, newest(text) + 1_000));
            assert_eq!(
                record.settlement,
                Settlement::Settled,
                "{}: {why}",
                moment.name
            );
            assert_eq!(
                (record.aborted, record.failed),
                (interrupted, !interrupted),
                "{}: {why}: {record:?}",
                moment.name
            );
        }
    }
}

#[test]
fn a_turn_interrupted_at_any_point_is_over_and_its_abandoned_call_holds_nothing_open() {
    // The earlier branch's four points of an Escape (T-2356), built from what Pi 1.6.0's loop
    // writes: with the word at once, without it by the quiet.
    for (point, text) in [
        (
            "while the model writes text",
            include_str!("../../../../tests/fixtures/pi-interrupted/while-text.jsonl"),
        ),
        (
            "while it writes a call's arguments",
            include_str!("../../../../tests/fixtures/pi-interrupted/while-tool-arguments.jsonl"),
        ),
        (
            "while a tool runs",
            include_str!("../../../../tests/fixtures/pi-interrupted/while-tool-runs.jsonl"),
        ),
        (
            "between a tool's result and the next step",
            include_str!(
                "../../../../tests/fixtures/pi-interrupted/between-result-and-next-step.jsonl"
            ),
        ),
    ] {
        let last: Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
        let (mut stage, options) = staged(Some(text));
        put_word(
            &stage,
            &serde_json::from_str(&evidence_files::word(
                LAUNCH,
                SESSION,
                "rest",
                &last["id"],
                &[(1, last["id"].clone(), json!(true))],
            ))
            .unwrap(),
        );
        let record = known(&stage.reader.look(&options, newest(text) + 1_000));
        assert!(
            record.settlement == Settlement::Settled && record.aborted && !record.failed,
            "{point}: {record:?}"
        );
        let (mut stage, _) = staged(Some(text));
        let quiet = stage.at_time(newest(text) + 120_000);
        assert!(
            quiet.settlement == Settlement::Settled && quiet.aborted && !quiet.failed,
            "{point}, no word: {quiet:?}"
        );
        assert!(
            quiet.last_call.is_none()
                || point == "while a tool runs"
                || point == "between a tool's result and the next step",
            "{point}: an abandoned call is never named: {quiet:?}"
        );
    }
}

#[test]
fn a_resume_onto_a_leaf_of_each_kind_is_at_rest_by_the_word() {
    // The quit cut a run anywhere; Pi resumed at its prompt says `rest` at its session's start
    // (no end of this launch to take). No open call, no user message holds it at work.
    let leaves = [
        ("a user message", vec![header(), user("u1", "go")]),
        (
            "a call not answered",
            vec![header(), user("u1", "go"), calling("a1", &json!("c1"))],
        ),
        (
            "a tool's result",
            vec![
                header(),
                user("u1", "go"),
                calling("a1", &json!("c1")),
                result("r1", &json!("c1")),
            ],
        ),
        (
            "an answer",
            vec![header(), user("u1", "go"), assistant("a1", "stop", "done")],
        ),
        (
            "an error",
            vec![header(), user("u1", "go"), assistant("a1", "error", "down")],
        ),
        (
            "an abort",
            vec![
                header(),
                user("u1", "go"),
                assistant("a1", "aborted", "partial"),
            ],
        ),
    ];
    for (kind, rows) in leaves {
        let leaf = rows.last().unwrap()["id"].clone();
        let mut stage = Stage::with(&rows);
        let options = evidence_files::options(&stage, LAUNCH);
        put_word(
            &stage,
            &serde_json::from_str(&evidence_files::word(LAUNCH, SESSION, "rest", &leaf, &[]))
                .unwrap(),
        );
        let record = known(&stage.look(1_000, &options));
        assert!(
            record.settlement == Settlement::Settled && !record.in_flight,
            "{kind}: {record:?}"
        );
        assert_eq!(record.failed, kind == "an error", "{kind}");
    }
}
