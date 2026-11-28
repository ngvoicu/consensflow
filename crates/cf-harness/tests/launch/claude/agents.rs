//! A Claude window whose turn ended with background agents or workflows still
//! running (a research chief's agents, a worker that left some). Claude Code's
//! own status of its process (`sessions/<pid>.json`) says `busy` for as long as
//! any runs (`busy` is its turn loading or any such task not ended), though its
//! prompt is up and takes what is typed or pasted; the end of one wakes a turn
//! of its own. The transcript's own word is its `turn_duration`, which carries
//! how many are left (`pendingBackgroundAgentCount`, `pendingWorkflowCount`). A
//! window whose turn the transcript says ended with agents pending is at rest
//! whatever its status says of them: so a message for it goes in between turns,
//! the chief's and a worker's alike (2026-10-11: a chief with agents running
//! kept a result waiting for 12 minutes, the status `busy` all through).
//!
//! The records are the shapes Claude Code 2.1.296 wrote for such a turn: the
//! answer, the stop hook's summary, and a `turn_duration` that says the count.

use cf_harness::contract::Over;
use cf_harness::testing::EPOCH_MS;

use super::*;

const SESSION: &str = "b3a1c9de-52f4-4e37-8f10-6d0a7c2e9b15";
const BRIEF: &str = "[ConsensFlow m-1 · T-1 · task from @chief]\nResearch it with two agents";

/// A turn's `turn_duration`, which says `agents` of the background agents run.
fn duration_line(n: u32, agents: Option<u64>) -> String {
    let mut fields = json!({
        "type": "system",
        "subtype": "turn_duration",
        "isSidechain": false,
        "durationMs": 42_000,
        "messageCount": 3,
    });
    if let Some(agents) = agents {
        fields["pendingBackgroundAgentCount"] = json!(agents);
    }
    record(SESSION, n, &fields)
}

/// The records of a turn that began with the brief and ended with `agents` left.
fn turn(agents: Option<u64>) -> Vec<String> {
    vec![
        user_line(SESSION, 1, BRIEF),
        answer_line(SESSION, 2, "Started two agents; waiting for them."),
        stop_line(SESSION, 3),
        duration_line(4, agents),
    ]
}

/// A window on the conversation of `lines`, its status `word` where Claude
/// says one. The window's own process wrote what follows the first line: the
/// window was opened on the brief alone (opened when the record held the turn,
/// it would be a process after that turn's, whose agents went with it).
fn window_on(lines: &[String], word: Option<&str>) -> (Home, Rc<dyn Window>) {
    let home = Home::new();
    home.transcript(SESSION, &lines[..1]);
    let window = window(&home.adapter(), &Request::resumed(SESSION));
    home.transcript(SESSION, lines);
    if let Some(word) = word {
        home.status(SESSION, &json!({ "status": word }), me());
    }
    (home, window)
}

fn look(window: &Rc<dyn Window>) -> cf_harness::contract::Observed {
    finished(window.observe()).unwrap()
}

#[test]
fn a_window_busy_with_the_agents_its_turn_left_is_at_rest_and_the_look_says_the_agents_are_pending()
{
    let (_home, window) = window_on(&turn(Some(2)), Some("busy"));
    let observed = look(&window);
    assert!(
        observed.settled,
        "the turn is over and the prompt takes input"
    );
    assert!(observed.agents_pending());
    assert_eq!(observed.waiting, None);
    assert!(!observed.failed);
}

#[test]
fn a_window_busy_with_a_turn_that_left_no_agents_is_not_at_rest() {
    // The transcript says the turn is over, and nothing is left to keep Claude
    // busy: whatever it is busy with, it is not the agents of a turn.
    let (_home, window) = window_on(&turn(None), Some("busy"));
    let observed = look(&window);
    assert!(!observed.settled);
    assert!(!observed.agents_pending());
}

#[test]
fn agents_pending_are_no_word_of_the_window_where_claude_said_none() {
    // No status file: Claude's own word on whether it is at its prompt is
    // missing, and the transcript's does not stand in for it.
    let (_home, window) = window_on(&turn(Some(2)), None);
    let observed = look(&window);
    assert!(!observed.settled);
    assert!(observed.agents_pending(), "the reading still says so");
}

#[test]
fn a_window_on_its_own_dialog_waits_there_whatever_agents_its_turn_left() {
    let home = Home::new();
    home.transcript(SESSION, &turn(Some(2)));
    // The window's own Claude writes its status once the window is open: one written
    // before is another Claude's (T-800).
    let window = window(&home.adapter(), &Request::resumed(SESSION));
    home.status(
        SESSION,
        &json!({ "status": "waiting", "waitingFor": "permission prompt" }),
        me(),
    );
    let observed = look(&window);
    assert!(!observed.settled);
    assert_eq!(
        observed.waiting,
        Some(Waiting {
            reason: Some("permission prompt".to_owned())
        })
    );
}

#[test]
fn a_window_idle_or_with_a_shell_running_is_at_rest_as_it_always_was() {
    for word in ["idle", "shell"] {
        let (_home, window) = window_on(&turn(Some(2)), Some(word));
        let observed = look(&window);
        assert!(observed.settled, "{word}");
        assert!(observed.agents_pending(), "{word}");
        let (_home, window) = window_on(&turn(None), Some(word));
        let observed = look(&window);
        assert!(observed.settled, "{word}");
        assert!(!observed.agents_pending(), "{word}");
    }
}

#[test]
fn a_message_typed_between_turns_is_busy_again_and_says_nothing_of_the_agents_before_it() {
    let mut lines = turn(Some(2));
    lines.push(user_line(SESSION, 5, "How is it going?"));
    let (_home, window) = window_on(&lines, Some("busy"));
    let observed = look(&window);
    assert!(!observed.settled, "its turn is under way");
    assert!(!observed.agents_pending());
}

#[test]
fn a_message_is_pasted_into_a_window_busy_with_its_agents_as_into_any_at_rest() {
    let (_home, window) = window_on(&turn(Some(2)), Some("busy"));
    assert!(look(&window).settled);
    let host = AnsweringHost::new(|op| {
        Ok(match op {
            "pane.snapshot" => json!({ "ok": true, "pasteInFlight": false, "unsent": false }),
            _ => json!({ "ok": true }),
        })
    });
    let pane = Pane {
        id: "s1-chief".to_owned(),
        generation: 1,
    };
    assert_eq!(
        finished(window.ready(&host, &pane)),
        Ok(Readiness::Ready),
        "nothing in the window's way"
    );
    assert_eq!(
        finished(window.deliver(&host, &pane, "Result of T-2")),
        Ok(Admission::Admitted { queued: false })
    );
}

/// What the pane host said of the human's input at a look `ready()` took is the window's
/// to say, for the trace of a delivery that look held (T-883); a look that read no
/// snapshot says nothing.
#[test]
fn a_window_says_what_its_last_look_read_of_the_humans_input() {
    let (_home, window) = window_on(&turn(Some(2)), Some("busy"));
    assert_eq!(window.human_input(), None, "no look yet");
    let host = AnsweringHost::new(|op| {
        Ok(match op {
            "pane.snapshot" => json!({
                "ok": true, "pasteInFlight": false, "unsent": false, "typed": 3,
                "typedQuietMs": 1_900,
                "humanWrites": [{ "agoMs": 1_900, "length": 25, "classes": ["string"] }],
            }),
            _ => json!({ "ok": true }),
        })
    });
    let pane = Pane {
        id: "s1-chief".to_owned(),
        generation: 1,
    };
    assert_eq!(finished(window.ready(&host, &pane)), Ok(Readiness::Ready));
    assert_eq!(
        window.human_input(),
        Some(cf_proto::panes::HumanInput {
            typed: Some(3),
            typed_quiet_ms: Some(1_900),
            unsent: Some(false),
            writes: vec![cf_proto::panes::HumanWrite {
                ago_ms: 1_900,
                length: 25,
                classes: vec!["string"],
            }],
        })
    );
}

/// The records of a turn that began with the brief, started its agents, and was
/// interrupted before it answered (T-604, 2026-10-25): the interrupt's record ends
/// the turn, and no `turn_duration` counts the agents that still run.
fn interrupted() -> Vec<String> {
    let message = json!({
        "id": format!("{SESSION}-message-2"),
        "role": "assistant",
        "content": [{ "type": "text", "text": "Three agents are sweeping the docs." }],
    });
    let interrupt = json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{ "type": "text", "text": "[Request interrupted by user]" }],
        },
    });
    vec![
        user_line(SESSION, 1, BRIEF),
        record(
            SESSION,
            2,
            &json!({ "type": "assistant", "message": message }),
        ),
        record(SESSION, 3, &interrupt),
    ]
}

/// The records of a turn the API refused for quota, with nothing after the refusal.
fn refused() -> Vec<String> {
    let refusal = json!({
        "type": "assistant",
        "isApiErrorMessage": true,
        "apiErrorStatus": 429,
        "error": "rate_limit",
        "message": {
            "id": format!("{SESSION}-message-2"),
            "role": "assistant",
            "content": [{ "type": "text", "text": "You've hit your weekly limit · resets Oct 27 at 10am" }],
        },
    });
    vec![user_line(SESSION, 1, BRIEF), record(SESSION, 2, &refusal)]
}

/// A window on the conversation of `lines`, its status `word`, as `window_on` makes
/// one, on a clock the test moves: the record's word that a turn is over counts once it
/// has stood a moment.
struct Timed {
    home: Home,
    fakes: Fakes,
    window: Rc<dyn Window>,
}

impl Timed {
    fn on(lines: &[String], word: Option<&str>) -> Self {
        let home = Home::new();
        let fakes = Fakes::new(&home.env());
        let adapter = ClaudeAdapter::new(&fakes.services(&home.env(), Path::new(&home.root)));
        home.transcript(SESSION, &lines[..1]);
        let window = prepare(&adapter, &Request::resumed(SESSION))
            .unwrap()
            .window;
        home.transcript(SESSION, lines);
        if let Some(word) = word {
            home.status(SESSION, &json!({ "status": word }), me());
        }
        Self {
            home,
            fakes,
            window,
        }
    }

    /// A look at the window `ms` after the run began.
    fn look_at(&self, ms: i64) -> cf_harness::contract::Observed {
        self.fakes.time.settle_at(EPOCH_MS + ms);
        look(&self.window)
    }
}

/// How long the record's word that a turn is over must stand before it counts.
const HOLD: i64 = 1_000;

#[test]
fn a_turn_interrupted_while_its_agents_run_is_over_though_claude_says_busy_and_is_not_at_rest() {
    // T-604's window: the stop's Escape ended its turn, Claude stayed busy with
    // three agents, and the look read it at work through every round.
    let timed = Timed::on(&interrupted(), Some("busy"));
    let first = timed.look_at(0);
    assert_eq!(
        first.over,
        Over::Pending,
        "not yet: a turn may be beginning"
    );
    let observed = timed.look_at(HOLD);
    assert!(
        !observed.settled,
        "busy, and the record counts no agents: nothing goes in yet"
    );
    assert_eq!(
        observed.over,
        Over::Held,
        "the record says the turn ended: nothing in it is left to interrupt"
    );
}

#[test]
fn a_refused_turn_is_over_whatever_claude_says_of_its_status() {
    for word in [Some("busy"), None] {
        let timed = Timed::on(&refused(), word);
        assert_eq!(timed.look_at(0).over, Over::Pending, "{word:?}");
        let observed = timed.look_at(HOLD);
        assert!(observed.failed, "{word:?}");
        assert!(!observed.settled, "{word:?}");
        assert_eq!(observed.over, Over::Held, "{word:?}");
    }
}

#[test]
fn a_turn_beginning_is_not_over_in_the_moment_claude_says_busy_before_its_message_is_written() {
    // Claude's status goes busy through an effect and a file of its own, and the
    // message of the turn it begins is a separate append: a look between the two
    // reads the turn before over (Calliope on T-551, T-644).
    let timed = Timed::on(&turn(None), Some("busy"));
    assert_eq!(
        timed.look_at(0).over,
        Over::Pending,
        "the status flipped, the message is not written yet: no word yet"
    );
    let mut lines = turn(None);
    lines.push(user_line(SESSION, 5, "Go on with the parser"));
    timed.home.transcript(SESSION, &lines);
    for ms in [300, HOLD + 500] {
        let observed = timed.look_at(ms);
        assert!(
            !observed.settled && observed.over == Over::No,
            "at {ms} ms: a turn under way"
        );
    }
    // A turn that ends anew starts the wait anew: its message is not the one before.
    lines.push(answer_line(SESSION, 6, "Parser done."));
    lines.push(stop_line(SESSION, 7));
    timed.home.transcript(SESSION, &lines);
    assert_eq!(
        timed.look_at(HOLD + 600).over,
        Over::Pending,
        "its own moment first"
    );
    assert_eq!(timed.look_at(2 * HOLD + 600).over, Over::Held);
}

#[test]
fn a_turn_under_way_is_never_over_and_a_window_at_rest_is_settled_and_no_more() {
    for (lines, said) in [
        (turn(None)[..2].to_vec(), "the answer has no end yet"),
        (interrupted()[..2].to_vec(), "before the interrupt"),
    ] {
        let timed = Timed::on(&lines, Some("busy"));
        for ms in [0, HOLD, 3 * HOLD] {
            let observed = timed.look_at(ms);
            assert!(
                !observed.settled && observed.over == Over::No,
                "{said} at {ms} ms"
            );
        }
    }
    for word in ["idle", "shell"] {
        let timed = Timed::on(&interrupted(), Some(word));
        for ms in [0, HOLD] {
            let observed = timed.look_at(ms);
            assert!(
                observed.settled && observed.over == Over::No,
                "{word} at {ms} ms"
            );
        }
    }
}

/// The records Claude Code 2.1.329 wrote on the Mac (T-1857) for a turn whose agent
/// ran on after the Escape: the interrupt for a tool use is line 36, and a line the
/// human typed at the prompt opens a turn at 38, 1.697 s later, while the background
/// agent's command still ran. Its status stayed busy all through.
fn recorded_interrupt(lines: usize) -> Vec<String> {
    include_str!("../../fixtures/claude/interrupted-with-background-agent.jsonl")
        .replace("$SESSION", SESSION)
        .split_inclusive('\n')
        .take(lines)
        .map(str::to_owned)
        .collect()
}

/// Whether the Mac's prompt takes words after that interrupt, as Claude Code did there.
const MAC: bool = cfg!(target_os = "macos");

#[test]
fn a_mac_window_busy_with_a_background_agent_takes_words_once_the_tool_use_interrupt_has_stood() {
    // N23: the status says busy for the agent, so the look is not settled, and the
    // words that resume the task waited until the human typed. The recording shows the
    // prompt takes a typed line there; the second's stand (`Over::Held`) is the engine's.
    let timed = Timed::on(&recorded_interrupt(36), Some("busy"));
    let first = timed.look_at(0);
    assert!(!first.takes_words && !first.settled);
    assert_eq!(first.over, Over::Pending);
    let early = timed.look_at(HOLD - 1);
    assert!(!early.takes_words, "not before the interrupt's end stood");
    let held = timed.look_at(HOLD);
    assert!(!held.settled, "the result still waits for the agent");
    assert_eq!(held.over, Over::Held);
    assert_eq!(held.takes_words, MAC, "only the Mac's prompt was recorded");
    assert!(!held.shared && held.waiting.is_none());

    // The line typed at 38 opens a turn: it takes no more, and neither do the lines
    // that follow it.
    for lines in [38, 40] {
        timed.home.transcript(SESSION, &recorded_interrupt(lines));
        for ms in [HOLD + 1_700, 4 * HOLD] {
            let observed = timed.look_at(ms);
            assert!(
                !observed.takes_words && !observed.settled && observed.over == Over::No,
                "a turn under way ({lines} lines, {ms} ms)"
            );
        }
    }
}

#[test]
fn a_window_takes_no_words_after_an_interrupt_in_generation_nor_after_a_refusal_whatever_its_status_says(
) {
    // The interrupt while the model writes has no recording with background work, and a
    // turn a refusal ended is T-604's: main's gate holds for both.
    let in_generation: Vec<String> = recorded_interrupt(36)
        .into_iter()
        .map(|line| line.replace(" for tool use]", "]"))
        .collect();
    assert!(in_generation
        .last()
        .unwrap()
        .contains("[Request interrupted by user]"));
    for (said, lines) in [
        ("in generation", in_generation),
        ("refused", refused()),
        ("the in-generation interrupt of T-604", interrupted()),
    ] {
        let timed = Timed::on(&lines, Some("busy"));
        for ms in [0, HOLD, 4 * HOLD] {
            let observed = timed.look_at(ms);
            assert!(!observed.takes_words, "{said} at {ms} ms");
            assert!(!observed.settled, "{said} at {ms} ms");
        }
    }
}

#[test]
fn a_window_that_another_claude_shows_takes_no_words_after_the_interrupt_whatever_either_status_says(
) {
    // T-1904's case: another Claude on the same conversation can write that interrupt
    // while this window's own turn runs, and the record cannot tell whose it is. The flag
    // stays set once it is raised.
    let timed = Timed::on(&recorded_interrupt(35), Some("busy"));
    let before = timed.look_at(0);
    assert!(!before.shared && !before.takes_words);
    let other = OtherProcess::start();
    timed
        .home
        .status(SESSION, &json!({ "status": "idle" }), other.pid());
    timed.home.transcript(SESSION, &recorded_interrupt(36));
    for ms in [1_100, 4_200] {
        let observed = timed.look_at(ms);
        assert!(observed.shared, "{ms} ms");
        assert!(!observed.takes_words, "{ms} ms");
    }
    drop(other);
    let after = timed.look_at(6_000);
    assert!(after.shared && !after.takes_words, "the flag stays");
}

#[test]
fn a_window_that_says_idle_or_a_shell_takes_words_when_it_is_settled_and_no_other_time() {
    for word in ["idle", "shell"] {
        let timed = Timed::on(&interrupted(), Some(word));
        let observed = timed.look_at(HOLD);
        assert!(observed.settled && observed.takes_words, "{word}");
    }
    for lines in [turn(None)[..2].to_vec(), turn(Some(2))] {
        let timed = Timed::on(&lines, Some("busy"));
        for ms in [0, HOLD] {
            let observed = timed.look_at(ms);
            assert_eq!(observed.takes_words, observed.settled, "{ms} ms");
        }
    }
}
