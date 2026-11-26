//! A line a person types into a Claude window that begins no turn (T-2330): what the window's
//! look says of it when the reader holds the turn in flight. The records are the listed
//! commands' recorded shape under the name of one outside the reader's closed list (the repository
//! holds no recording of such a command: `/cost` and the like are named in the record tests).

use cf_harness::contract::Over;

use super::*;

const SESSION: &str = "c4b2d0e1-3a57-4f68-9b21-7e5d8a1f6c03";

fn line(n: u32, parent: Option<u32>, content: &str, meta: bool) -> String {
    record(
        SESSION,
        n,
        &json!({
            "type": "user",
            "isSidechain": false,
            "isMeta": meta,
            "parentUuid": parent.map(|parent| format!("{SESSION}-{parent}")),
            "message": { "role": "user", "content": content },
        }),
    )
}

/// A turn that ended, then a command outside the list typed at the idle prompt: the caveat,
/// the echo, and what it printed.
fn typed() -> Vec<String> {
    vec![
        line(1, None, "Hello", false),
        answer_line(SESSION, 2, "Hi"),
        stop_line(SESSION, 3),
        line(
            4,
            Some(3),
            "<local-command-caveat>The messages below were made by a command the user ran; do not answer them.</local-command-caveat>",
            true,
        ),
        line(
            5,
            Some(4),
            "<command-name>/cost</command-name>\n            <command-message>cost</command-message>\n            <command-args></command-args>",
            false,
        ),
        line(
            6,
            Some(5),
            "<local-command-stdout>Total cost: $0.00</local-command-stdout>",
            false,
        ),
    ]
}

fn look(window: &Rc<dyn Window>) -> cf_harness::contract::Observed {
    finished(window.observe()).unwrap()
}

/// Claude's own status says `idle` (its prompt is up and takes words), and the record holds
/// the turn the command opened: the look reads the window at work. Claude's status is no
/// second signal that ends it; nothing but a turn's end does.
#[test]
fn claudes_idle_status_does_not_end_a_turn_the_record_holds_open_after_such_a_command() {
    let home = Home::new();
    let lines = typed();
    home.transcript(SESSION, &lines[..1]);
    let window = window(&home.adapter(), &Request::resumed(SESSION));
    home.transcript(SESSION, &lines);
    home.status(SESSION, &json!({ "status": "idle" }), me());
    for _ in 0..3 {
        let observed = look(&window);
        assert!(!observed.settled, "the record holds the turn open");
        assert!(!observed.takes_words, "nothing is pasted into it");
        assert!(observed.at_prompt, "Claude's own word: its prompt is up");
        assert_eq!(observed.over, Over::No);
    }
}

/// The listed command in the same lines: at rest at its output, so the window takes words.
#[test]
fn the_same_lines_of_a_listed_command_leave_the_window_at_rest() {
    let home = Home::new();
    let lines: Vec<String> = typed()
        .into_iter()
        .map(|line| line.replace("/cost</command-name>", "/model</command-name>"))
        .collect();
    home.transcript(SESSION, &lines[..1]);
    let window = window(&home.adapter(), &Request::resumed(SESSION));
    home.transcript(SESSION, &lines);
    home.status(SESSION, &json!({ "status": "idle" }), me());
    let observed = look(&window);
    assert!(observed.settled && observed.takes_words);
}
