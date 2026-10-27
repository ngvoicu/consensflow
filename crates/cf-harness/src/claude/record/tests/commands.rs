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

/// What a command printed, written as the system's record (`local_command`), a
/// child of the command `parent`.
fn printed_by_the_system(uuid: &str, parent: &str, words: &str) -> Value {
    of_session(json!({
        "type": "system", "subtype": "local_command", "isMeta": false, "level": "info",
        "content": format!("<local-command-stdout>{words}</local-command-stdout>"),
        "uuid": uuid, "parentUuid": parent,
    }))
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
            local("model", "", printed("o1", "n1", "")),
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

/// A `/compact` typed into a conversation with nothing to compact, as Claude Code
/// 2.1.304 writes it (the live run of 2026-10-20, `cancelled`, on a conversation
/// just cleared): no bare command, the words before the command, the command,
/// and what it printed as the system's record.
#[test]
fn a_compact_that_found_nothing_to_compact_is_at_rest_at_what_the_system_says_it_printed() {
    let mut records = at_rest();
    records.extend([
        own("file-history-snapshot"),
        caveat("k1", "d1"),
        typed_command("n1", "k1", "compact", ""),
    ]);
    assert_eq!(
        settlement_of(&records),
        Settlement::InFlight,
        "the command runs"
    );
    records.push(printed_by_the_system(
        "o1",
        "n1",
        "Nothing here to compact yet.",
    ));
    assert_eq!(settlement_of(&records), Settlement::Settled);
    // The next prompt is the user's, and opens a turn.
    records.push(user("u2", json!("o1"), json!("Hello again")));
    assert_eq!(settlement_of(&records), Settlement::InFlight);
}

#[test]
fn each_command_whose_output_the_system_writes_is_at_rest_after_it_and_one_it_writes_whole_never_began_a_turn(
) {
    // 2.1.219's `/reload-skills`: the command the user's, what it printed the system's.
    let mut reloaded = at_rest();
    reloaded.extend([
        caveat("k1", "d1"),
        typed_command("n1", "k1", "reload-skills", ""),
        printed_by_the_system("o1", "n1", "Reloaded the skills"),
    ]);
    assert_eq!(settlement_of(&reloaded), Settlement::Settled);
    // `/mcp`, `/plugin`: the command and what it printed both the system's, no turn.
    let mut dismissed = at_rest();
    let mut command = printed_by_the_system("n1", "d1", "x");
    command["content"] = json!("<command-name>/mcp</command-name>");
    dismissed.extend([
        command,
        printed_by_the_system("o1", "n1", "Dialog dismissed"),
    ]);
    assert_eq!(settlement_of(&dismissed), Settlement::Settled);
}

/// What one look at a transcript of `records` reads.
fn record_of(records: &[Value]) -> Record {
    let mut stage = Stage::new();
    stage.write(records);
    match &*stage.reader.look(&Options::default(), 0) {
        Reading::Known(record) => record.clone(),
        Reading::Unknown(reason) | Reading::Missing(reason) => panic!("unknown: {reason}"),
    }
}

/// A compaction cancelled with Escape, as Claude Code 2.1.304 writes it (the live suite
/// `commands-at-rest`, `cancelled`, 2026-10-20): the bare `/compact` as it was typed, and at
/// the Escape the words before a command, a sibling of it (the same parent), and nothing
/// more. Claude puts the `/compact` back in its input box, which the reading says.
#[test]
fn a_compaction_cancelled_with_escape_is_at_rest_at_the_words_before_a_command_beside_it() {
    let mut records = at_rest();
    records.extend([own("file-history-snapshot"), bare("c0", "d1", "/compact")]);
    records.extend(bookkeeping());
    let compacting = record_of(&records);
    assert_eq!(
        compacting.settlement,
        Settlement::InFlight,
        "the compaction runs"
    );
    assert!(!compacting.lines.put_back);
    records.push(caveat("k1", "d1"));
    let cancelled = record_of(&records);
    assert_eq!(
        cancelled.settlement,
        Settlement::Settled,
        "it was cancelled"
    );
    assert!(
        cancelled.lines.put_back,
        "the `/compact` is back in the box"
    );
    // What the human sends next is a prompt, the `/compact` Claude put back with it.
    records.extend([
        own("file-history-snapshot"),
        bare("c1", "k1", "/compact keep the plan"),
    ]);
    let sent = record_of(&records);
    assert_eq!(sent.settlement, Settlement::InFlight);
    assert!(!sent.lines.put_back);
    // A message the human sends instead, answered: its turn put nothing back.
    let mut answered = at_rest();
    answered.extend([bare("c0", "d1", "/compact"), caveat("k1", "d1")]);
    answered.extend([
        user("u2", json!("k1"), json!("Hello again")),
        assistant(
            "a2",
            "u2",
            json!("m2"),
            json!([text(json!("Hi again"))]),
            json!("end_turn"),
        ),
        summary("h2", "a2"),
        duration("d2", "h2"),
    ]);
    let read = record_of(&answered);
    assert_eq!(read.settlement, Settlement::Settled);
    assert!(!read.lines.put_back, "the box Claude put back was sent");
    // The human sends what the box holds, and it compacts: its turn ends at what the command
    // printed, with no answer, and put nothing back.
    let (typed, done) = compact_typed();
    let mut compacted = at_rest();
    compacted.extend([bare("x0", "d1", "/compact"), caveat("x1", "d1")]);
    compacted.extend(typed);
    compacted.extend(done);
    let read = record_of(&compacted);
    assert_eq!(read.settlement, Settlement::Settled);
    assert!(!read.lines.put_back, "the /compact the box held was sent");
    // A message sent instead and stopped with Escape before a word of it: the interrupt ends
    // its turn, and put nothing back that is still there.
    let mut stopped = at_rest();
    stopped.extend([
        bare("c0", "d1", "/compact"),
        caveat("k1", "d1"),
        user("u2", json!("k1"), json!("Hello again")),
        having(
            user(
                "u3",
                json!("u2"),
                json!([text(json!("[Request interrupted by user]"))]),
            ),
            json!({ "interruptedMessageId": "m2" }),
        ),
    ]);
    let read = record_of(&stopped);
    assert_eq!(read.settlement, Settlement::Settled);
    assert!(!read.lines.put_back, "a turn since the cancel");
}

#[test]
fn a_compaction_at_work_or_one_that_finished_is_never_read_cancelled() {
    let typed = || {
        let mut records = at_rest();
        records.push(bare("c0", "d1", "/compact"));
        records.extend(bookkeeping());
        records
    };
    let with = |more: Vec<Value>| {
        let mut records = typed();
        records.extend(more);
        record_of(&records)
    };
    let not_meta = lacking(caveat("k1", "d1"), "isMeta");
    for (why, more) in [
        (
            "the words, a child of the bare command",
            vec![caveat("k1", "c0")],
        ),
        ("words that are no meta record", vec![not_meta]),
        (
            "the words of another session",
            vec![having(caveat("k1", "d1"), json!({ "sessionId": "other" }))],
        ),
        (
            "a prompt between",
            vec![user("u2", json!("d1"), json!("Hello")), caveat("k1", "d1")],
        ),
        (
            "the model between",
            vec![
                assistant(
                    "a2",
                    "c0",
                    json!("m2"),
                    json!([text(json!("Compacting"))]),
                    Value::Null,
                ),
                caveat("k1", "d1"),
            ],
        ),
    ] {
        let read = with(more);
        assert_eq!(read.settlement, Settlement::InFlight, "{why}");
        assert!(!read.lines.put_back, "{why}");
    }
    // A compaction that finished writes the words after its boundary and summary, their
    // child: it ends at what the command printed, and puts nothing back.
    let finished = with(vec![
        compacted("b1", "c0"),
        compact_summary("s1", "b1"),
        caveat("k1", "s1"),
    ]);
    assert_eq!(finished.settlement, Settlement::InFlight, "not yet printed");
    assert!(!finished.lines.put_back);
    // A command that only begins as `/compact` is none.
    let mut other = at_rest();
    other.extend([bare("c0", "d1", "/compacted"), caveat("k1", "d1")]);
    assert_eq!(settlement_of(&other), Settlement::InFlight);
}

/// A command that goes on to ask the model what it printed, as 2.1.303's `/plan
/// <description>` does (Sol's look at round 1): the command, what it printed, and
/// then the model's turn, whose end ends it. Any command but the ones Claude does
/// by itself is read so: a skill, a command of the user's own, a name unknown.
#[test]
fn a_command_that_goes_on_to_the_model_is_at_work_from_what_it_printed_to_the_end_of_the_turn() {
    for (name, args) in [
        ("plan", "a plan for the release"),
        ("my-skill", ""),
        ("nonesuch", ""),
    ] {
        let mut records = at_rest();
        records.extend([
            caveat("k1", "d1"),
            typed_command("n1", "k1", name, args),
            printed("o1", "n1", "Planning the release"),
        ]);
        assert_eq!(
            settlement_of(&records),
            Settlement::InFlight,
            "/{name}: what it printed"
        );
        let mut by_the_system = records.clone();
        by_the_system.pop();
        by_the_system.push(printed_by_the_system("o1", "n1", "Planning"));
        assert_eq!(
            settlement_of(&by_the_system),
            Settlement::InFlight,
            "/{name}: what it printed, as the system's record"
        );
        records.push(assistant(
            "a2",
            "o1",
            json!("m2"),
            json!([text(json!("The plan:"))]),
            Value::Null,
        ));
        assert_eq!(
            settlement_of(&records),
            Settlement::InFlight,
            "/{name}: the model at work"
        );
        records.extend([
            assistant(
                "a3",
                "a2",
                json!("m2"),
                json!([text(json!("Done."))]),
                json!("end_turn"),
            ),
            duration("d2", "a3"),
        ]);
        assert_eq!(settlement_of(&records), Settlement::Settled, "/{name}");
    }
}

#[test]
fn a_model_turn_after_what_a_command_printed_is_at_work_until_its_answer() {
    // Should a command go on to run the model, its answer's records open the turn
    // again: the output ends nothing the model does after it.
    let mut records = at_rest();
    records.extend([
        caveat("k1", "d1"),
        typed_command("n1", "k1", "compact", ""),
        printed_by_the_system("o1", "n1", "Done"),
    ]);
    let working = assistant(
        "a2",
        "o1",
        json!("m2"),
        json!([tool_use(json!("t1"))]),
        Value::Null,
    );
    records.push(working);
    assert_eq!(settlement_of(&records), Settlement::InFlight);
    let answered = assistant(
        "a3",
        "a2",
        json!("m2"),
        json!([text(json!("Done"))]),
        json!("end_turn"),
    );
    records.extend([
        user(
            "r1",
            json!("a2"),
            json!([tool_result(json!("t1"), json!("ok"))]),
        ),
        answered,
        duration("d2", "a3"),
    ]);
    assert_eq!(settlement_of(&records), Settlement::Settled);
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
