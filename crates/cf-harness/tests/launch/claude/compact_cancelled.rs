//! A Claude window whose `/compact` the human cancelled with Escape, and how the look
//! at it and the next paste read it: on the records Claude Code wrote in a live run of
//! `cargo xtask live commands-at-rest --command cancelled` (scrubbed by `cargo xtask
//! scrub-transcript`).
//!
//! `compact-cancelled`: a message, answered; `/compact` typed, and Escape a second
//! later. Claude wrote the bare `/compact` and, at the Escape, the words it writes
//! before a command, a sibling of the bare command, and nothing more; it read idle,
//! with the `/compact` back in its input box. The window reads at rest, and the next
//! paste clears the box first where the screen shows it holds something, goes in with
//! no key where the box reads empty, and waits where the screen shows no box.

use cf_harness::contract::HostError;

use super::*;

const SESSION: &str = "5b1a3c0e-7d42-4f19-9c55-2e8f0a6d4b71";

/// The records of the live run, for `session`.
fn fixture(session: &str) -> Vec<String> {
    include_str!("../../fixtures/claude/compact-cancelled.jsonl")
        .replace("$SESSION", session)
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect()
}

/// A window on the conversation of the live run, idle, and what it is made of.
struct Cancelled {
    _home: Home,
    _fakes: Fakes,
    window: Rc<dyn Window>,
}

impl Cancelled {
    fn new() -> Self {
        let home = Home::new();
        let fakes = Fakes::new(&home.env());
        let adapter = ClaudeAdapter::new(&fakes.services(&home.env(), Path::new(&home.root)));
        let window = prepare(&adapter, &Request::resumed(SESSION))
            .unwrap()
            .window;
        home.transcript(SESSION, &fixture(SESSION));
        home.status(SESSION, &json!({ "status": "idle" }), me());
        Self {
            _home: home,
            _fakes: fakes,
            window,
        }
    }

    /// Whether a look reads the window at rest.
    fn settled(&self) -> bool {
        finished(self.window.observe()).unwrap().settled
    }
}

/// The screen's last lines with Claude's input box holding `typed`.
fn screen(typed: &str) -> Value {
    let rule = "─".repeat(40);
    json!([
        "⏺ READY",
        "",
        rule,
        format!("❯ {typed}"),
        rule,
        "  ⏵⏵ bypass permissions on (shift+tab to cycle)"
    ])
}

/// A host whose snapshot says the human left nothing unsent, or something (`unsent`),
/// and shows the screen's last lines `bottom` (none: no lines).
fn host(
    unsent: bool,
    bottom: Option<Value>,
) -> AnsweringHost<impl Fn(&str) -> Result<Value, HostError>> {
    AnsweringHost::new(move |op| {
        Ok(match op {
            "pane.snapshot" => {
                let mut snapshot = json!({ "ok": true, "pasteInFlight": false, "unsent": unsent,
                    "typed": u64::from(unsent), "typedQuietMs": 60_000, "outputQuietMs": 30_000 });
                if let Some(bottom) = &bottom {
                    snapshot["bottom"] = bottom.clone();
                }
                snapshot
            }
            _ => json!({ "ok": true }),
        })
    })
}

/// What was asked of the host but its snapshots: its operation, and the keys or the text.
fn asked(host: &AnsweringHost<impl Fn(&str) -> Result<Value, HostError>>) -> Vec<(String, Value)> {
    host.asked
        .borrow()
        .iter()
        .filter(|(op, _)| op != "pane.snapshot")
        .map(|(op, body)| {
            let what = body.get("bytes").or_else(|| body.get("body")).cloned();
            (op.clone(), what.unwrap_or(Value::Null))
        })
        .collect()
}

fn pane() -> Pane {
    Pane {
        id: "s1-chief".to_owned(),
        generation: 3,
    }
}

/// The window made ready and handed `text` through `host`: its readiness, and what
/// was asked of the host where it was ready.
fn handed(
    cancelled: &Cancelled,
    host: &AnsweringHost<impl Fn(&str) -> Result<Value, HostError>>,
    text: &str,
) -> Readiness {
    let ready = finished(cancelled.window.ready(host, &pane())).unwrap();
    if ready == Readiness::Ready {
        let sent = finished(cancelled.window.deliver(host, &pane(), text));
        assert_eq!(sent, Ok(Admission::Admitted { queued: false }));
    }
    ready
}

#[test]
fn a_window_whose_compaction_was_cancelled_is_at_rest() {
    assert!(Cancelled::new().settled());
}

#[test]
fn the_compact_claude_put_back_is_cleared_before_the_next_message_is_pasted() {
    let cancelled = Cancelled::new();
    assert!(cancelled.settled());
    let shown = host(false, Some(screen("/compact")));
    assert_eq!(handed(&cancelled, &shown, "Result: DONE"), Readiness::Ready);
    assert_eq!(
        asked(&shown),
        [
            ("pane.input".to_owned(), json!([3])),
            ("pane.write_paste".to_owned(), json!("Result: DONE")),
        ],
        "Ctrl+C once, and then the paste"
    );
}

#[test]
fn a_box_the_screen_shows_empty_after_the_cancel_is_pasted_into_with_no_key() {
    // The human emptied it: a Ctrl+C now could be the second in a row, which ends Claude.
    let cancelled = Cancelled::new();
    assert!(cancelled.settled());
    let empty = host(false, Some(screen("")));
    assert_eq!(handed(&cancelled, &empty, "Result: DONE"), Readiness::Ready);
    assert_eq!(
        asked(&empty),
        [("pane.write_paste".to_owned(), json!("Result: DONE"))]
    );
}

#[test]
fn a_box_the_screen_does_not_show_after_the_cancel_holds_the_paste() {
    let cancelled = Cancelled::new();
    assert!(cancelled.settled());
    for bottom in [None, Some(json!(["fake agent started on s-1"]))] {
        let unread = host(false, bottom);
        // Held as the human's unsent words are, which the board tells them.
        assert_eq!(
            handed(&cancelled, &unread, "Result: DONE"),
            Readiness::Held(Held::Unsent)
        );
        assert_eq!(asked(&unread), [], "nothing pressed, nothing pasted");
    }
}

#[test]
fn words_the_human_typed_after_the_cancel_hold_the_paste_as_any_unsent_words_do() {
    let cancelled = Cancelled::new();
    assert!(cancelled.settled());
    let typed = host(true, Some(screen("/compact keep the plan")));
    assert_eq!(
        handed(&cancelled, &typed, "Result: DONE"),
        Readiness::Held(Held::Unsent)
    );
    assert_eq!(asked(&typed), []);
}

#[test]
fn a_box_cleared_once_is_not_cleared_again_by_the_next_paste() {
    let cancelled = Cancelled::new();
    assert!(cancelled.settled());
    handed(
        &cancelled,
        &host(false, Some(screen("/compact"))),
        "Result: DONE",
    );
    // The paste is in and its record is not yet: nothing of the cancel is left to clear,
    // and a second Ctrl+C at an empty box ends Claude.
    let again = host(false, Some(screen("")));
    finished(cancelled.window.deliver(&again, &pane(), "More")).unwrap();
    assert_eq!(
        asked(&again),
        [("pane.write_paste".to_owned(), json!("More"))]
    );
}
