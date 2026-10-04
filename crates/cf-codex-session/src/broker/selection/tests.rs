use super::*;
use serde_json::json;

const A: &str = "01a09094-938f-7fd1-a2d3-315cf92b4559";
const B: &str = "01a09094-a559-7db0-bf50-e2309856c3c0";
const TUI: ClientId = ClientId(1);
const OTHER: ClientId = ClientId(2);

/// A broker's state with one TUI connection speaking through it.
struct Window {
    selection: Selection,
    requests: Requests,
}

impl Window {
    fn new() -> Self {
        Self::bypassing(false)
    }

    fn bypassing(fresh_bypass: bool) -> Self {
        let mut selection = Selection::new(fresh_bypass);
        selection.ready();
        Self {
            selection,
            requests: Requests::default(),
        }
    }

    /// What the TUI says; what goes to Codex in its place, if anything.
    fn says(&mut self, message: Value) -> Option<Value> {
        self.selection.tui_said(TUI, &mut self.requests, &message)
    }

    /// What Codex answers the TUI.
    fn answers(&mut self, message: Value) {
        self.selection.codex_said(TUI, &mut self.requests, &message);
    }

    /// The TUI starts its main thread and Codex answers it.
    fn starts(&mut self, id: u64, thread: Value) {
        self.says(start(id));
        self.answers(json!({ "id": id, "result": { "thread": thread } }));
    }

    fn shown(&self) -> Option<&str> {
        self.selection.session_id()
    }
}

fn start(id: impl Into<Value>) -> Value {
    json!({
        "id": id.into(),
        "method": "thread/start",
        "params": { "ephemeral": false, "threadSource": "user" },
    })
}

fn resume(id: impl Into<Value>, thread: &str) -> Value {
    json!({
        "id": id.into(),
        "method": "thread/resume",
        "params": { "threadId": thread, "runtimeWorkspaceRoots": [] },
    })
}

fn idle(id: u64, thread: &str) -> Value {
    json!({ "id": id, "result": { "thread": { "id": thread, "status": { "type": "idle" } } } })
}

#[test]
fn a_main_start_resume_and_fork_are_told_from_the_rest() {
    let switches = |message: Value| {
        let mut window = Window::new();
        window.says(message);
        window.selection.revision() == 1
    };
    let params =
        |extra: Value| json!({ "id": 1, "method": extra["method"], "params": extra["params"] });
    for (message, main) in [
        (start(1), true),
        (
            params(
                json!({ "method": "thread/start", "params": { "ephemeral": true, "threadSource": "system" } }),
            ),
            false,
        ),
        (
            params(json!({ "method": "thread/start", "params": { "threadSource": "user" } })),
            false,
        ),
        (
            params(
                json!({ "method": "thread/start", "params": { "ephemeral": false, "threadSource": "subagent" } }),
            ),
            false,
        ),
        (
            params(json!({ "method": "thread/start", "params": { "ephemeral": false } })),
            false,
        ),
        (resume(1, B), true),
        // Since Codex 0.159 the roots are null: it is the field being there that counts.
        (
            params(
                json!({ "method": "thread/resume", "params": { "threadId": B, "runtimeWorkspaceRoots": null } }),
            ),
            true,
        ),
        (
            params(json!({ "method": "thread/resume", "params": { "threadId": B } })),
            false,
        ),
        (
            params(json!({ "method": "thread/resume", "params": null })),
            false,
        ),
        (
            params(
                json!({ "method": "thread/fork", "params": { "threadSource": "user", "runtimeWorkspaceRoots": [] } }),
            ),
            true,
        ),
        (
            params(
                json!({ "method": "thread/fork", "params": { "threadSource": "user", "runtimeWorkspaceRoots": [], "ephemeral": false } }),
            ),
            true,
        ),
        (
            params(
                json!({ "method": "thread/fork", "params": { "threadSource": "user", "runtimeWorkspaceRoots": [], "ephemeral": true } }),
            ),
            false,
        ),
        (
            params(json!({ "method": "thread/fork", "params": { "threadSource": "user" } })),
            false,
        ),
        (
            params(
                json!({ "method": "thread/fork", "params": { "threadSource": "system", "runtimeWorkspaceRoots": [] } }),
            ),
            false,
        ),
        (
            json!({ "id": 1, "method": "turn/start", "params": { "threadId": A } }),
            false,
        ),
        (json!(5), false),
        (json!(null), false),
    ] {
        assert_eq!(switches(message.clone()), main, "{message}");
    }
}

#[test]
fn a_switch_names_the_thread_once_codex_answers_it() {
    let mut window = Window::new();
    window.says(start(1));
    assert_eq!(
        window.shown(),
        None,
        "the window shows none while it switches"
    );
    assert!(!window.selection.available(true));
    window.answers(json!({
        "id": 1,
        "result": { "thread": { "id": A, "turns": [], "status": { "type": "idle" } } },
    }));
    assert_eq!(window.shown(), Some(A));
    assert!(window.selection.is_empty());
    assert!(window.selection.available(true));
    let admission = window.selection.admit(A, true).unwrap();
    assert_eq!(admission.thread, A);
    assert!(admission.idle, "an idle thread's message starts its turn");
}

#[test]
fn a_thread_is_empty_only_with_no_turn_and_an_idle_status() {
    for (thread, empty) in [
        (
            json!({ "id": A, "turns": [], "status": { "type": "idle" } }),
            true,
        ),
        (
            json!({ "id": A, "turns": [{}], "status": { "type": "idle" } }),
            false,
        ),
        (
            json!({ "id": A, "turns": [], "status": { "type": "active" } }),
            false,
        ),
        (json!({ "id": A, "status": { "type": "idle" } }), false),
        (json!({ "id": A, "turns": [] }), false),
    ] {
        let mut window = Window::new();
        window.starts(1, thread.clone());
        assert_eq!(window.selection.is_empty(), empty, "{thread}");
        assert_eq!(window.shown(), Some(A));
    }
}

#[test]
fn a_thread_that_cannot_be_delivered_to_is_not_shown() {
    for result in [
        json!({ "thread": { "id": "not-a-uuid" } }),
        json!({ "thread": { "id": 7 } }),
        json!({ "thread": {} }),
        json!({}),
        json!(null),
        json!({ "thread": { "id": A }, "readOnly": true }),
        json!({ "thread": { "id": A }, "readOnly": "yes" }),
    ] {
        let mut window = Window::new();
        window.says(start(1));
        window.answers(json!({ "id": 1, "result": result }));
        assert_eq!(window.shown(), None, "{result}");
        assert!(!window.selection.available(true), "{result}");
        // Not switching any more: the answer came.
        assert_eq!(window.selection.revision(), 1);
    }
    let mut window = Window::new();
    window.says(start(1));
    window.answers(json!({ "id": 1, "result": { "thread": { "id": A }, "readOnly": false } }));
    assert_eq!(window.shown(), Some(A));
}

#[test]
fn an_error_answer_goes_back_to_what_the_window_showed() {
    let mut window = Window::new();
    window.starts(
        1,
        json!({ "id": A, "turns": [], "status": { "type": "idle" } }),
    );
    window.says(resume(2, B));
    assert_eq!(window.shown(), None);
    window.answers(json!({ "id": 2, "error": { "code": -1, "message": "not found" } }));
    assert_eq!(window.shown(), Some(A));
    assert!(
        window.selection.is_empty(),
        "its emptiness comes back with it"
    );
    assert!(window.selection.available(true));
}

#[test]
fn an_error_with_nothing_shown_before_leaves_nothing_shown() {
    let mut window = Window::new();
    window.says(start(1));
    window.answers(json!({ "id": 1, "error": { "message": "no" } }));
    assert_eq!(window.shown(), None);
    assert!(!window.selection.available(true));
}

#[test]
fn a_null_error_is_no_error() {
    let mut window = Window::new();
    window.says(start(1));
    window.answers(json!({ "id": 1, "error": null, "result": { "thread": { "id": A } } }));
    assert_eq!(window.shown(), Some(A));
}

#[test]
fn a_switch_while_switching_goes_back_to_none_on_error() {
    let mut window = Window::new();
    window.starts(1, json!({ "id": A }));
    window.says(resume(2, B));
    // A second switch before the first answered: it has nothing to go back to.
    window.says(resume(3, A));
    window.answers(json!({ "id": 3, "error": { "message": "no" } }));
    assert_eq!(window.shown(), None);
}

#[test]
fn ids_are_compared_as_json_values_numbers_and_strings_alike() {
    let mut window = Window::new();
    window.says(start("1"));
    window.answers(idle(1, A));
    assert_eq!(window.shown(), None, "the number 1 is not the string \"1\"");
    window.answers(json!({ "id": "1", "result": { "thread": { "id": A } } }));
    assert_eq!(window.shown(), Some(A));

    let mut window = Window::new();
    window.says(start(1));
    window.answers(json!({ "id": "1", "result": { "thread": { "id": A } } }));
    assert_eq!(window.shown(), None);
    window.answers(idle(1, A));
    assert_eq!(window.shown(), Some(A));
}

#[test]
fn a_request_of_codexs_own_that_shares_an_id_with_the_switch_is_not_its_answer() {
    // The server numbers the requests it sends the TUI on its own; one may
    // carry the id of a switch the TUI is waiting on.
    let mut window = Window::new();
    window.says(start(1));
    window.answers(
        json!({ "id": 1, "method": "item/commandExecution/requestApproval", "params": {} }),
    );
    assert_eq!(window.shown(), None);
    assert!(!window.selection.available(true), "it is still switching");
    window.answers(idle(1, A));
    assert_eq!(
        window.shown(),
        Some(A),
        "the real answer, coming after, still counts"
    );
}

#[test]
fn a_notification_never_answers_a_request() {
    let mut window = Window::new();
    window.says(start(1));
    window.answers(json!({ "method": "turn/started", "params": { "threadId": A } }));
    window.answers(json!({ "method": null, "id": 1, "result": { "thread": { "id": A } } }));
    assert_eq!(window.shown(), None);
}

#[test]
fn only_the_latest_switch_applies_and_an_older_answer_after_it_is_stale() {
    let mut window = Window::new();
    window.says(start(1));
    window.says(resume(2, B));
    // The newest answered first.
    window.answers(idle(2, B));
    assert_eq!(window.shown(), Some(B));
    window.answers(idle(1, A));
    assert_eq!(
        window.shown(),
        Some(B),
        "the first switch's answer is stale"
    );
    assert!(window.selection.available(true));
}

#[test]
fn an_older_answer_before_the_latest_leaves_the_window_switching() {
    let mut window = Window::new();
    window.says(start(1));
    window.says(resume(2, B));
    window.answers(idle(1, A));
    assert_eq!(window.shown(), None);
    assert!(
        !window.selection.available(true),
        "still waiting for the latest"
    );
    window.answers(idle(2, B));
    assert_eq!(window.shown(), Some(B));
}

#[test]
fn another_connections_answer_does_not_apply_to_the_owner() {
    let mut selection = Selection::new(false);
    selection.ready();
    let (mut first, mut second) = (Requests::default(), Requests::default());
    selection.tui_said(TUI, &mut first, &start(1));
    selection.tui_said(OTHER, &mut second, &resume(1, B));
    // The first connection chose first; the second owns the window now.
    selection.codex_said(TUI, &mut first, &idle(1, A));
    assert_eq!(selection.session_id(), None);
    selection.codex_said(OTHER, &mut second, &idle(1, B));
    assert_eq!(selection.session_id(), Some(B));
}

#[test]
fn the_owners_connection_lost_forgets_the_thread_and_another_ones_does_not() {
    let mut window = Window::new();
    window.starts(1, json!({ "id": A }));
    let before = window.selection.revision();
    window.selection.retire(OTHER);
    assert_eq!(
        window.shown(),
        Some(A),
        "a picker's connection never owned it"
    );
    assert_eq!(window.selection.revision(), before);
    window.selection.retire(TUI);
    assert_eq!(window.shown(), None);
    assert_eq!(window.selection.revision(), before + 1);
    assert!(!window.selection.available(true));
    // And while it was switching: not switching any more.
    let mut window = Window::new();
    window.says(start(1));
    window.selection.retire(TUI);
    window.says(start(2));
    window.answers(idle(2, B));
    assert_eq!(window.shown(), Some(B));
}

#[test]
fn a_connection_lost_before_it_chose_leaves_what_was_chosen() {
    let mut selection = Selection::new(false);
    selection.ready();
    let mut owner = Requests::default();
    selection.tui_said(TUI, &mut owner, &start(1));
    selection.codex_said(TUI, &mut owner, &idle(1, A));
    selection.retire(OTHER);
    assert_eq!(selection.session_id(), Some(A));
}

#[test]
fn the_first_start_opens_in_full_permission_mode_and_nothing_else_is_rewritten() {
    let mut window = Window::bypassing(true);
    let sent = json!({
        "id": 1,
        "method": "thread/start",
        "params": {
            "ephemeral": false,
            "threadSource": "user",
            "approvalPolicy": "on-request",
            "sandbox": "read-only",
            "permissions": { "profile": "home" },
            "cwd": "/work",
        },
    });
    let rewritten = window.says(sent.clone()).unwrap();
    assert_eq!(
        rewritten.to_string(),
        r#"{"id":1,"method":"thread/start","params":{"ephemeral":false,"threadSource":"user","approvalPolicy":"never","sandbox":"danger-full-access","permissions":null,"cwd":"/work"}}"#
    );
    // A start that is not the main one, and a resume or fork, go as they are.
    assert_eq!(window.says(resume(2, B)), None);
    assert_eq!(
        window.says(json!({ "id": 3, "method": "thread/start", "params": { "ephemeral": true } })),
        None
    );
    // Without the bypass nothing is rewritten.
    assert_eq!(Window::new().says(sent), None);
}

#[test]
fn a_start_that_names_no_permission_gets_them_added_after_its_own_fields() {
    let mut window = Window::bypassing(true);
    let rewritten = window.says(start(1)).unwrap();
    assert_eq!(
        rewritten.to_string(),
        r#"{"id":1,"method":"thread/start","params":{"ephemeral":false,"threadSource":"user","approvalPolicy":"never","sandbox":"danger-full-access","permissions":null}}"#
    );
}

#[test]
fn the_bypass_outlives_the_first_start_alone() {
    let mut window = Window::bypassing(true);
    window.starts(1, json!({ "id": A }));
    assert!(
        window.says(start(2)).is_some(),
        "a /new is a fresh thread too"
    );
}

#[test]
fn a_main_resume_that_took_ends_the_bypass_and_one_that_failed_does_not() {
    let mut window = Window::bypassing(true);
    window.says(resume(1, B));
    window.answers(json!({ "id": 1, "error": { "message": "no" } }));
    assert!(
        window.says(start(2)).is_some(),
        "a failed resume changed no permissions"
    );
    window.answers(idle(2, A));
    window.says(resume(3, B));
    window.answers(idle(3, B));
    assert!(
        window.says(start(4)).is_none(),
        "a resume keeps the thread's own permissions"
    );
    // A resume answered with a thread nobody can deliver to does not count either.
    let mut window = Window::bypassing(true);
    window.says(resume(1, B));
    window.answers(json!({ "id": 1, "result": { "thread": { "id": B }, "readOnly": true } }));
    assert!(window.says(start(2)).is_some());
}

#[test]
fn a_permission_change_on_the_shown_thread_that_took_ends_the_bypass() {
    let update = |id: u64, thread: &str, key: &str| json!({ "id": id, "method": "thread/settings/update", "params": { "threadId": thread, key: "on-request" } });
    for key in ["approvalPolicy", "sandbox", "permissions"] {
        let mut window = Window::bypassing(true);
        window.starts(1, json!({ "id": A }));
        window.says(update(2, A, key));
        window.answers(json!({ "id": 2, "result": {} }));
        assert!(window.says(start(3)).is_none(), "{key}");
    }
    // Not on another thread, not naming a permission, not when Codex refuses it.
    let mut window = Window::bypassing(true);
    window.starts(1, json!({ "id": A }));
    window.says(update(2, B, "approvalPolicy"));
    window.answers(json!({ "id": 2, "result": {} }));
    window.says(json!({ "id": 3, "method": "thread/settings/update", "params": { "threadId": A, "model": "x" } }));
    window.answers(json!({ "id": 3, "result": {} }));
    window.says(update(4, A, "sandbox"));
    window.answers(json!({ "id": 4, "error": { "message": "no" } }));
    assert!(window.says(start(5)).is_some());
}

#[test]
fn a_permission_change_asked_while_no_thread_is_shown_is_not_tracked() {
    let mut window = Window::bypassing(true);
    window.says(start(1));
    window.says(json!({
        "id": 2,
        "method": "thread/settings/update",
        "params": { "threadId": null, "approvalPolicy": "never" },
    }));
    window.answers(json!({ "id": 2, "result": {} }));
    assert!(window.says(start(3)).is_some());
}

#[test]
fn threads_are_idle_as_codex_last_said() {
    let mut window = Window::new();
    window.starts(1, json!({ "id": A, "status": { "type": "idle" } }));
    let admit = |window: &mut Window| window.selection.admit(A, true).unwrap().idle;
    // Using the thread takes its idleness.
    assert!(admit(&mut window));
    assert!(!admit(&mut window));
    window.answers(json!({ "method": "turn/completed", "params": { "threadId": A } }));
    assert!(admit(&mut window), "a turn ended: idle again");
    window
        .selection
        .control_said(&json!({ "method": "turn/started", "params": { "threadId": A } }));
    assert!(!admit(&mut window));
    window.selection.control_said(&json!({
        "method": "thread/status/changed",
        "params": { "threadId": A, "status": { "type": "idle" } },
    }));
    assert!(admit(&mut window));
    window.answers(json!({
        "method": "thread/status/changed",
        "params": { "threadId": A, "status": { "type": "active" } },
    }));
    assert!(!admit(&mut window));
    // Said of another thread, or of none: nothing changes for this one.
    window.answers(json!({ "method": "turn/completed", "params": { "threadId": B } }));
    window.answers(json!({ "method": "turn/completed", "params": {} }));
    assert!(!admit(&mut window));
}

#[test]
fn a_thread_that_started_without_a_status_is_not_idle() {
    let mut window = Window::new();
    window.starts(1, json!({ "id": A }));
    assert!(!window.selection.admit(A, true).unwrap().idle);
}

#[test]
fn a_turn_the_tui_or_codex_starts_ends_the_threads_emptiness() {
    let empty = json!({ "id": A, "turns": [], "status": { "type": "idle" } });
    for sent in [
        json!({ "id": 2, "method": "turn/start", "params": { "threadId": A, "input": [] } }),
        json!({ "id": 2, "method": "thread/queue/add", "params": { "threadId": A } }),
    ] {
        let mut window = Window::new();
        window.starts(1, empty.clone());
        assert!(window.selection.is_empty());
        window.says(sent);
        assert!(!window.selection.is_empty());
        assert!(
            !window.selection.admit(A, true).unwrap().idle,
            "the turn is running"
        );
    }
    // One aimed at another thread changes nothing.
    let mut window = Window::new();
    window.starts(1, empty.clone());
    window.says(json!({ "id": 2, "method": "turn/start", "params": { "threadId": B } }));
    assert!(window.selection.is_empty());
    // Nor does a turn Codex starts on another thread; one on this one does.
    window.answers(json!({ "method": "turn/started", "params": { "threadId": B } }));
    assert!(window.selection.is_empty());
    window.answers(json!({ "method": "turn/started", "params": { "threadId": A } }));
    assert!(!window.selection.is_empty());
}

#[test]
fn a_delivery_is_taken_only_for_the_thread_shown_and_only_when_the_broker_can_reach_codex() {
    let mut window = Window::new();
    let unavailable = Err(Refusal::NativeSessionUnavailable);
    assert_eq!(
        window.selection.admit(A, true),
        unavailable,
        "no thread yet"
    );
    window.says(start(1));
    assert_eq!(window.selection.admit(A, true), unavailable, "switching");
    window.answers(idle(1, A));
    assert_eq!(
        window.selection.admit(B, true),
        Err(Refusal::NativeSessionChanged)
    );
    assert_eq!(
        window.selection.admit(A, false),
        unavailable,
        "its connection to Codex is down"
    );
    assert!(window.selection.admit(A, true).is_ok());
    window.selection.lose_control();
    assert_eq!(window.selection.admit(A, true), unavailable);
    assert!(!window.selection.available(true));
}

#[test]
fn unavailable_comes_before_changed() {
    let mut window = Window::new();
    window.starts(1, json!({ "id": A }));
    window.selection.close();
    assert_eq!(
        window.selection.admit(B, true),
        Err(Refusal::NativeSessionUnavailable)
    );
}

#[test]
fn an_admission_is_current_until_the_window_switches_or_is_lost() {
    let mut window = Window::new();
    window.starts(1, json!({ "id": A }));
    let admission = window.selection.admit(A, true).unwrap();
    assert!(window.selection.is_current(&admission));
    window.says(resume(2, A));
    assert!(
        !window.selection.is_current(&admission),
        "it is switching, to the same thread even"
    );
    window.answers(idle(2, A));
    assert!(
        !window.selection.is_current(&admission),
        "a switch was made since"
    );
    let again = window.selection.admit(A, true).unwrap();
    window.selection.retire(TUI);
    assert!(!window.selection.is_current(&again));
}

#[test]
fn a_closed_broker_shows_no_thread_and_takes_nothing() {
    let mut window = Window::new();
    window.starts(1, json!({ "id": A }));
    window.selection.close();
    assert!(window.selection.is_closed());
    assert_eq!(window.shown(), None);
    assert!(!window.selection.available(true));
}

#[test]
fn a_broker_is_not_available_before_its_connection_is_initialized() {
    let mut selection = Selection::new(false);
    let mut requests = Requests::default();
    selection.tui_said(TUI, &mut requests, &start(1));
    selection.codex_said(TUI, &mut requests, &idle(1, A));
    assert_eq!(selection.session_id(), Some(A));
    assert!(!selection.available(true));
    selection.ready();
    assert!(selection.available(true));
}

#[test]
fn a_thread_id_is_a_hyphenated_uuid_of_version_1_to_8_and_variant_8_to_b_in_any_case() {
    for good in [
        A,
        B,
        "01A09094-938F-7FD1-A2D3-315CF92B4559",
        "00000000-0000-1000-8000-000000000000",
        "ffffffff-ffff-8fff-bfff-ffffffffffff",
        "0f8fad5b-d9cb-469f-a165-70867728950e",
    ] {
        assert!(is_thread_id(good), "{good}");
    }
    for bad in [
        "",
        "not-a-uuid",
        "01a09094938f7fd1a2d3315cf92b4559",
        "01a09094-938f-0fd1-a2d3-315cf92b4559",
        "01a09094-938f-9fd1-a2d3-315cf92b4559",
        "01a09094-938f-7fd1-72d3-315cf92b4559",
        "01a09094-938f-7fd1-c2d3-315cf92b4559",
        "01a09094-938f-7fd1-a2d3-315cf92b455",
        "01a09094-938f-7fd1-a2d3-315cf92b45590",
        "01a09094-938f-7fd1-a2d3-315cf92b455g",
        " 01a09094-938f-7fd1-a2d3-315cf92b4559",
        "01a09094-938f-7fd1-a2d3-315cf92b4559\n",
        "01a09094_938f_7fd1_a2d3_315cf92b4559",
    ] {
        assert!(!is_thread_id(bad), "{bad:?}");
    }
}
