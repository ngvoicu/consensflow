//! A command typed at Claude's prompt while the conversation is idle
//! (`/compact`, `/model`, `/effort`): Claude Code writes, as user's records, the
//! words before it (`<local-command-caveat>`, a meta record), the command
//! (`<command-name>/…</command-name>`) and what it printed
//! (`<local-command-stdout>`), and runs no model after. Claude Code 2.1.303 writes
//! the bare `/compact` first, as it is typed, and the compaction's boundary and
//! summary before the rest. The output, a child of the command, ends the turn the
//! records before it began.
//!
//! The long run of 2026-10-19 (Claude Code 2.1.303): the chief, idle, was given a
//! `/compact`, and its window read at work from then to the end of the run, 80
//! minutes; a task's result was never handed to it. The records here are in the
//! shape of its transcript, their words made up.

use super::compaction::{compact_summary, compacted};
use super::*;

/// A record of Claude Code's own that is no message, of the type `kind`.
fn own(kind: &str) -> Value {
    json!({ "type": kind, "sessionId": SESSION })
}

/// The records Claude Code 2.1.303 writes after a prompt it takes, which say
/// nothing of the turn.
fn bookkeeping() -> Vec<Value> {
    let mut records = vec![having(
        own("last-prompt"),
        json!({ "lastPrompt": "/compact" }),
    )];
    records.extend(
        [
            "ai-title",
            "mode",
            "permission-mode",
            "atis-latch",
            "bridge-session",
        ]
        .map(own),
    );
    records
}

/// The bare command `typed`, as 2.1.303 writes it the moment it is typed.
fn bare(uuid: &str, parent: &str, typed: &str) -> Value {
    having(
        user(uuid, json!(parent), json!(typed)),
        json!({ "promptId": "p1" }),
    )
}

/// The words Claude Code writes before a command typed at its prompt.
fn caveat(uuid: &str, parent: &str) -> Value {
    having(
        user(
            uuid,
            json!(parent),
            json!("<local-command-caveat>The command below was typed at the prompt; no answer is wanted.</local-command-caveat>"),
        ),
        json!({ "isMeta": true }),
    )
}

/// The command `/name`, typed with the arguments `args`.
pub(super) fn typed_command(uuid: &str, parent: &str, name: &str, args: &str) -> Value {
    user(
        uuid,
        json!(parent),
        json!(format!(
            "<command-name>/{name}</command-name>\n            <command-message>{name}</command-message>\n            <command-args>{args}</command-args>"
        )),
    )
}

/// What a command printed, a child of the command `parent`, in the tag `tag`
/// (`stdout`, `stderr`).
fn printed_to(uuid: &str, parent: &str, tag: &str, words: &str) -> Value {
    user(
        uuid,
        json!(parent),
        json!(format!(
            "<local-command-{tag}>{words}</local-command-{tag}>"
        )),
    )
}

/// What a command printed to its output.
pub(super) fn printed(uuid: &str, parent: &str, words: &str) -> Value {
    printed_to(uuid, parent, "stdout", words)
}

/// An attachment of the type `kind`, and of the hook `event` where it names one.
fn attached(uuid: &str, parent: &str, kind: &str, event: Option<&str>) -> Value {
    let record = in_attachment(attachment(uuid, parent), "type", Some(json!(kind)));
    in_attachment(record, "hookEvent", event.map(|event| json!(event)))
}

/// A turn that ended: the user's `Hello`, the answer, the stop hooks' summary
/// and the duration (`d1`).
fn at_rest() -> Vec<Value> {
    vec![
        hello(),
        answer("a1", "u1"),
        summary("h1", "a1"),
        duration("d1", "h1"),
    ]
}

/// A `/compact` typed while idle, as 2.1.303 writes it: the bare command, the
/// records of the prompt, the boundary and the summary, the words before the
/// command, the command, and what it printed (the compaction's word and a
/// `PostCompact` hook's), with the attachments the next prompt is to have.
/// Split where the compaction runs.
fn compact_typed() -> (Vec<Value>, Vec<Value>) {
    let mut typed = vec![own("file-history-snapshot"), bare("c0", "d1", "/compact")];
    typed.extend(bookkeeping());
    let words = "\u{1b}[2mCompacted \u{1b}[22m\nPostCompact says: the probe's own words\u{1b}[22m";
    let mut compacting = vec![
        compacted("b1", "c0"),
        having(compact_summary("s2", "b1"), json!({ "promptId": "p1" })),
        caveat("k1", "s2"),
        typed_command("n1", "k1", "compact", ""),
        having(
            printed("o1", "n1", words),
            json!({ "repeatsCompactSummary": true }),
        ),
    ];
    let mut parent = "o1".to_owned();
    for (at, (kind, event)) in [
        ("file", None),
        ("file", None),
        ("deferred_tools_delta", None),
        ("agent_listing_delta", None),
        ("environment", None),
        ("model", None),
        ("hook_success", Some("SessionStart")),
    ]
    .into_iter()
    .enumerate()
    {
        let uuid = format!("t{at}");
        compacting.push(attached(&uuid, &parent, kind, event));
        parent = uuid;
    }
    compacting.extend(bookkeeping());
    compacting.push(own("cost-state"));
    (typed, compacting)
}

#[test]
fn a_compact_typed_while_idle_is_at_rest_once_claude_says_what_it_printed() {
    let (typed, compacting) = compact_typed();
    let mut records = at_rest();
    assert_eq!(settlement_of(&records), Settlement::Settled);
    records.extend(typed);
    assert_eq!(
        settlement_of(&records),
        Settlement::InFlight,
        "the compaction runs"
    );
    records.extend(compacting);
    assert_eq!(
        settlement_of(&records),
        Settlement::Settled,
        "the compaction is over"
    );
    // A task's result reaches the chief: a prompt, which opens a turn, and its answer.
    records.push(user(
        "u2",
        json!("t6"),
        json!("[ConsensFlow m-9] T-13 is done"),
    ));
    assert_eq!(settlement_of(&records), Settlement::InFlight);
    let answered = assistant(
        "a2",
        "u2",
        json!("m2"),
        json!([text(json!("Read"))]),
        json!("end_turn"),
    );
    records.extend([answered, duration("d2", "a2")]);
    assert_eq!(settlement_of(&records), Settlement::Settled);
}

#[test]
fn a_reader_that_looks_through_the_compaction_reads_it_at_rest_after() {
    let (typed, compacting) = compact_typed();
    let read = looks(&[&at_rest(), &typed, &compacting]);
    let state = |reading: &str| {
        serde_json::from_str::<Value>(reading).unwrap()["settlement"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        read.iter()
            .map(|reading| state(reading))
            .collect::<Vec<_>>(),
        ["settled", "in-flight", "settled"]
    );
}

#[test]
fn each_command_typed_while_idle_is_at_rest_at_its_output() {
    let local = |name: &str, args: &str, output: Value| {
        vec![
            caveat("k1", "d1"),
            typed_command("n1", "k1", name, args),
            output,
        ]
    };
    let shapes = [
        // 2.1.301.
        (
            "model",
            local("model", "haiku", printed("o1", "n1", "Set model to Haiku")),
        ),
        (
            "effort",
            local(
                "effort",
                "low",
                printed("o1", "n1", "Set effort level to low"),
            ),
        ),
        (
            "printed nothing",
            local("cost", "", printed("o1", "n1", "")),
        ),
        (
            "an error",
            local(
                "model",
                "nonesuch",
                printed_to("o1", "n1", "stderr", "Model not found"),
            ),
        ),
        // 2.1.222: no bare command.
        ("compact", {
            let mut records = vec![compacted("b1", "d1"), compact_summary("s2", "b1")];
            records.extend([
                caveat("k1", "s2"),
                typed_command("n1", "k1", "compact", ""),
                printed("o1", "n1", "Compacted"),
            ]);
            records
        }),
    ];
    for (name, typed) in shapes {
        let mut records = at_rest();
        records.extend(typed.iter().take(typed.len() - 1).cloned());
        assert_eq!(
            settlement_of(&records),
            Settlement::InFlight,
            "{name}: the command runs"
        );
        records.push(typed[typed.len() - 1].clone());
        assert_eq!(settlement_of(&records), Settlement::Settled, "{name}");
    }
}

#[test]
fn the_output_ends_the_turn_only_where_it_follows_its_command_with_nothing_open() {
    let typed = |output: Value| {
        let mut records = at_rest();
        records.extend([
            caveat("k1", "d1"),
            typed_command("n1", "k1", "model", "haiku"),
            output,
        ]);
        settlement_of(&records)
    };
    let said = || printed("o1", "n1", "Set model to Haiku");
    assert_eq!(typed(said()), Settlement::Settled);
    for (why, output) in [
        ("a child of another record", printed("o1", "k1", "Set")),
        ("of no record", lacking(said(), "parentUuid")),
        (
            "of another session",
            having(said(), json!({ "sessionId": "other" })),
        ),
        (
            "on a sidechain",
            having(said(), json!({ "isSidechain": true })),
        ),
        ("of no main conversation", lacking(said(), "isSidechain")),
        (
            "with words before the tag",
            user(
                "o1",
                json!("n1"),
                json!(" <local-command-stdout>Set</local-command-stdout>"),
            ),
        ),
        (
            "the words before a command",
            user(
                "o1",
                json!("n1"),
                json!("<local-command-caveat>x</local-command-caveat>"),
            ),
        ),
    ] {
        assert_eq!(typed(output), Settlement::InFlight, "{why}");
    }
    // The output of a prompt, which no command is.
    let mut records = at_rest();
    records.extend([
        user("u2", json!("d1"), json!("Hello again")),
        printed("o1", "u2", "x"),
    ]);
    assert_eq!(settlement_of(&records), Settlement::InFlight);
}

#[test]
fn an_output_waits_while_the_stop_hooks_or_a_call_of_a_fragment_after_its_command_are_open() {
    // The command is the last pushed item, as a fragment of a message already
    // pushed pushes none: its stop hooks, or the call it opens, keep the output
    // of the command from ending the turn, as they keep a /clear's.
    let after = |first_stop: Value, fragment: Value| {
        let first = assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([text(json!("Hi"))]),
            first_stop,
        );
        settlement_of(&[
            hello(),
            first,
            typed_command("n1", "a1", "model", "haiku"),
            fragment,
            printed("o1", "n1", "Set model to Haiku"),
        ])
    };
    let said = |content: Value, stop: Value| assistant("a2", "n1", json!("m1"), content, stop);
    let more = || json!([text(json!("More"))]);
    assert_eq!(
        after(json!("end_turn"), said(more(), json!("end_turn"))),
        Settlement::InFlight
    );
    assert_eq!(
        after(
            Value::Null,
            said(json!([tool_use(json!("t1"))]), Value::Null)
        ),
        Settlement::InFlight
    );
    assert_eq!(
        after(Value::Null, said(more(), Value::Null)),
        Settlement::Settled
    );
}

#[test]
fn a_command_that_is_a_prompt_prints_nothing_and_its_turn_ends_at_its_answer() {
    // A skill, or a command of the user's own: the command, the prompt it stands
    // for (a meta record), and the model's answer.
    let mut records = at_rest();
    records.extend([
        typed_command("n1", "d1", "review", ""),
        having(
            user("p1", json!("n1"), json!("Review the change.")),
            json!({ "isMeta": true }),
        ),
    ]);
    assert_eq!(settlement_of(&records), Settlement::InFlight);
    let answered = assistant(
        "a2",
        "p1",
        json!("m2"),
        json!([text(json!("Looks fine"))]),
        json!("end_turn"),
    );
    records.extend([answered, duration("d2", "a2")]);
    assert_eq!(settlement_of(&records), Settlement::Settled);
}
