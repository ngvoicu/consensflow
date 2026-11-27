//! Who says that a Pi window is at work (the brain's `architecture/pi-turn-state.md`, §8): the
//! sessions and words Pi's own loop wrote, read with the word and without it.
//!
//! The fixtures in `tests/fixtures/pi-turn-state/<Pi>/<case>-<form>/<moment>/` were written by
//! `tests/pi-loop/drive.mjs --record` over the published Pi 1.6.0 and 1.6.1 (ConsensFlow's
//! extension loaded from the command line, a person's from the settings, Pi's own stand-in for
//! the model): each moment's session file as Pi wrote it, the word the extension had written by
//! then, and what the driver saw (`expect.json`). Each case is in two forms: plain, and with a
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
                    name: moment.strip_prefix(&root).unwrap().display().to_string(),
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
    assert!(all.len() >= 60, "the recorded moments: {}", all.len());
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

/// The last answer the reading holds: its last assistant item's words.
fn last_answer(record: &Record) -> Option<&str> {
    record
        .items
        .iter()
        .rev()
        .find(|item| item.role == Role::Assistant && !item.synthetic)
        .map(|item| &*item.text)
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
        .filter(|role| ["user", "assistant", "toolResult"].contains(&role.as_str()))
        .last();
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
    if expect["items"] == json!(0) {
        assert!(answer(record).is_empty(), "{what}: {record:?}");
    }
}

#[test]
fn every_recorded_moment_reads_with_the_word_as_pi_said_and_without_it_by_the_clock() {
    for moment in moments() {
        let what = &moment.name;
        // With the word, the moment it was written: what Pi said, at once.
        let (mut stage, options) = staged(moment.text.as_deref());
        if let Some(word) = &moment.word {
            put_word(&stage, word);
        }
        let now = moment.text.as_deref().map_or(WRITTEN, newest) + 1_000;
        let record = known(&stage.reader.look(&options, now));
        match moment.expect["at"].as_str().unwrap() {
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
        // Without the word (an extension that did not load): the file, by its entries' times.
        let Some(text) = moment.text.as_deref() else {
            continue;
        };
        let (mut stage, options) = staged(Some(text));
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
        if moment.expect["at"] == "rest" {
            ended_as(&late, &moment.expect, &format!("{what}, no word"));
        }
    }
}

/// The recorded moments of one case, in both versions and forms.
fn case(name: &str) -> Vec<Moment> {
    let moments: Vec<Moment> = moments()
        .into_iter()
        .filter(|m| {
            m.name
                .split('/')
                .nth(1)
                .is_some_and(|c| c.starts_with(&format!("{name}-")))
        })
        .collect();
    assert!(moments.len() >= 4, "{name}");
    moments
}

#[test]
fn each_ended_run_is_reported_once_in_order_read_up_to_its_own_leaf() {
    // D3: a run's answer, `agent_settled`, then a run a person's extension triggered from it,
    // deferred by Pi, ended too before any look: the word says `working` with two ends.
    for moment in case("triggered-after-settle") {
        let what = &moment.name;
        let text = moment.text.as_deref().unwrap();
        let word = moment.word.clone().unwrap();
        assert_eq!(word["ended"], 2, "{what}");
        let (mut stage, options) = staged(Some(text));
        let now = newest(text) + 1_000;
        // A look before either end: the reader has taken none.
        let mut before = word.clone();
        before["ended"] = json!(0);
        before["ends"] = json!([]);
        put_word(&stage, &before);
        assert!(known(&stage.reader.look(&options, now)).in_flight, "{what}");
        put_word(&stage, &word);
        let first = known(&stage.reader.look(&options, now));
        assert!(
            first.settlement == Settlement::Settled && !first.failed && !first.aborted,
            "{what}: {first:?}"
        );
        assert_eq!(
            last_answer(&first),
            Some("First answer done."),
            "{what}: read up to its own leaf: {first:?}"
        );
        let second = known(&stage.reader.look(&options, now));
        assert_eq!(second.settlement, Settlement::Settled, "{what}: {second:?}");
        assert_eq!(
            last_answer(&second),
            Some("Automatically continued."),
            "{what}"
        );
        let third = known(&stage.reader.look(&options, now));
        assert!(
            third.in_flight && third.lines.said_at_work,
            "{what}: the word says working: {third:?}"
        );
        // A reader made new starts from the word's count: no end is reported twice.
        let (mut fresh, options) = staged(Some(text));
        put_word(&fresh, &word);
        assert!(known(&fresh.reader.look(&options, now)).in_flight, "{what}");
    }
    // Finding 8: a retry cancelled in its backoff, then a run triggered from its end: the first
    // end reported as interrupted (Pi's flag false, its omission names the failed step), then
    // at work.
    for moment in case("cancelled-then-triggered") {
        let what = &moment.name;
        let text = moment.text.as_deref().unwrap();
        let word = moment.word.clone().unwrap();
        let (mut stage, options) = staged(Some(text));
        let now = newest(text) + 1_000;
        let mut before = word.clone();
        before["ended"] = json!(0);
        before["ends"] = json!([]);
        put_word(&stage, &before);
        assert!(known(&stage.reader.look(&options, now)).in_flight);
        put_word(&stage, &word);
        let end = known(&stage.reader.look(&options, now));
        assert!(
            end.settlement == Settlement::Settled && end.aborted && !end.failed,
            "{what}: {end:?}"
        );
        assert!(
            answer(&end).is_empty(),
            "{what}: the omitted step's words are none: {end:?}"
        );
        assert!(known(&stage.reader.look(&options, now)).in_flight, "{what}");
    }
}

#[test]
fn more_ends_than_the_word_keeps_between_two_looks_are_merged_into_the_oldest_kept() {
    // A failed step, so that each end's own flag shows which end a look reports.
    let mut stage = Stage::with(&[
        header(),
        user("u1", "go"),
        assistant("a1", "error", "partial"),
    ]);
    let options = evidence_files::options(&stage, LAUNCH);
    put_word(
        &stage,
        &serde_json::from_str(&evidence_files::word(
            LAUNCH,
            SESSION,
            "working",
            &json!("a1"),
            &[(2, json!("a1"), json!(false))],
        ))
        .unwrap(),
    );
    assert!(known(&stage.look(1_000, &options)).in_flight);
    // Ten more runs ended before the next look; the word keeps the last eight (5 to 12).
    let ends: Vec<(u64, Value, Value)> =
        (5..=12).map(|n| (n, json!("a1"), json!(n == 5))).collect();
    put_word(
        &stage,
        &serde_json::from_str(&evidence_files::word(
            LAUNCH,
            SESSION,
            "working",
            &json!("a1"),
            &ends,
        ))
        .unwrap(),
    );
    let merged = known(&stage.look(1_000, &options));
    assert!(
        merged.settlement == Settlement::Settled && merged.aborted && !merged.failed,
        "runs 3 to 5 as 5, with 5's flag: {merged:?}"
    );
    for n in 6..=12 {
        let end = known(&stage.look(1_000, &options));
        assert!(
            end.settlement == Settlement::Settled && end.failed && !end.aborted,
            "run {n}, with its own flag: {end:?}"
        );
    }
    assert!(known(&stage.look(1_000, &options)).in_flight);
}

#[test]
fn a_file_that_lacks_the_leaf_the_word_names_is_a_reading_older_than_the_word() {
    // D5's other half: at work for that look, and at rest once the file holds it.
    for moment in case("stop")
        .into_iter()
        .filter(|m| m.name.ends_with("answered"))
    {
        let text = moment.text.as_deref().unwrap();
        let word = moment.word.clone().unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let behind = format!("{}\n", lines[..lines.len() - 1].join("\n"));
        let (mut stage, options) = staged(Some(&behind));
        put_word(&stage, &word);
        let now = newest(text) + 1_000;
        let early = known(&stage.reader.look(&options, now));
        assert!(
            early.in_flight && !early.lines.said_at_work,
            "{}: {early:?}",
            moment.name
        );
        stage.write_text(text);
        let caught = known(&stage.reader.look(&options, now));
        assert_eq!(caught.settlement, Settlement::Settled, "{}", moment.name);
        assert_eq!(answer(&caught), vec!["Done.".to_owned()], "{}", moment.name);
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
