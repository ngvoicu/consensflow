//! A worker's window through its life: closed mid-task and resumed on its
//! conversation, paused in place, never opened, or never showing its first
//! message (`describe('the dispatcher')`).

use std::cell::RefCell;
use std::rc::Rc;

use cf_engine::testing::Context;
use serde_json::{json, Value};

use crate::chiefs::with_codex;
use crate::fixtures::{assert_match, native_of, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher"];

/// The native session a participant's conversation runs in.
fn native(context: &Context, project: i64, handle: &str) -> String {
    let id = context.id(project, handle);
    let conversation = context.ledger.borrow().current_conversation(id);
    conversation.unwrap().unwrap().native_session.unwrap()
}

/// What the last launch prepared was for and resumed.
fn resumed(context: &Context) -> (Value, Value) {
    let launch = context.adapter.prepared().pop().unwrap();
    (
        launch["participant"]["handle"].clone(),
        launch["resume"].clone(),
    )
}

#[test]
fn pauses_the_task_and_tells_the_requester_when_the_worker_window_closes_mid_task_and_resumes_it_on_its_conversation(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = native(&context, project.id, "zeus");
    context.exit("zeus");
    let task = context.task(project.id, 1);
    assert_eq!(
        (task.task.state.as_str(), task.task.assignee.as_deref()),
        ("paused", Some("zeus"))
    );
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(note.recipient, "chief");
    assert_match(
        &note.body,
        r#"^T-1 is paused: @zeus's window closed\. Resume it with: cf task resume T-1 "…""#,
    );
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "paused",
        "nothing happens on its own"
    );
    context.resume_task(project.id, 1, "Carry on");
    context.pass().unwrap();
    assert_eq!(resumed(&context), (json!("zeus"), json!(session)));
    let launch = context.adapter.prepared().pop().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"T-1 · task from @chief\]\nResumed: Carry on$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    held_to(
        context.close(),
        SUITES,
        "pauses the task and tells the requester when the worker window closes mid-task, and resumes it on its conversation",
    );
}

/// A worker's window that ends by itself in the middle of its task (its harness
/// crashed, or exited) is paused with how it ended and what its screen showed last,
/// as far as the host said: the field report of 2026-10-15 (B2, a Devin worker on
/// Windows) had only "window closed", and nothing to tell its cause by.
#[test]
fn a_worker_window_that_ends_mid_task_says_how_it_ended_and_what_it_showed_in_the_pause_note() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.host.exits_showing(
        Some(0xC000_0005),
        None,
        Some(&[
            "Reading src/parser.rs",
            "thread 'main' panicked at src/tui.rs:88",
        ]),
    );
    context.exit("zeus");

    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "paused");
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed (exit code 0xC0000005); its screen ended with: \
         \"Reading src/parser.rs / thread 'main' panicked at src/tui.rs:88\". \
         Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
    );
}

/// Kluris T-153 (T-823): a Claude Code worker's window closed with exit code 143 after the
/// project's own tests killed its `cargo test`, and the pause note said only "exit code 143".
/// No stop and no close of ConsensFlow's was under way, so the note says the signal and that it
/// did not come from ConsensFlow.
#[cfg(unix)]
#[test]
fn a_worker_window_that_exited_with_a_signals_status_says_so_and_that_consensflow_sent_it_no_kill()
{
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .host
        .exits_showing(Some(143), None, Some(&["Running the workspace tests"]));
    context.exit("zeus");

    let task = context.task(project.id, 1);
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed (exit code 143, consistent with SIGTERM; \
         ConsensFlow had sent it no kill); \
         its screen ended with: \"Running the workspace tests\". \
         Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
    );
}

/// The pause note the ledger holds for a worker whose window ended with exit code `code`, its
/// screen `screen` and its record's last tool call `call`.
fn paused_note(
    code: u32,
    screen: &[&str],
    call: Option<std::sync::Arc<cf_harness::records::LastCall>>,
) -> String {
    paused_note_on(code, screen, call, None)
}

/// [`paused_note`] for a window whose conversation runs in the harness's session `session`, as
/// Claude Code's does (the id its quit line prints), where one is given.
fn paused_note_on(
    code: u32,
    screen: &[&str],
    call: Option<std::sync::Arc<cf_harness::records::LastCall>>,
    session: Option<&str>,
) -> String {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.with("zeus", |agent| agent.last_call = call);
    if let Some(session) = session {
        let id = context.id(project.id, "zeus");
        let conversation = context
            .ledger
            .borrow()
            .current_conversation(id)
            .unwrap()
            .expect("its conversation");
        context
            .ledger
            .borrow_mut()
            .bind_conversation(conversation.id, session)
            .unwrap();
    }
    context.host.exits_showing(Some(code), None, Some(screen));
    context.exit("zeus");
    let task = context.task(project.id, 1);
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    note.body.clone()
}

/// What the ledger's note ends with after the screen: the resume.
const RESUME: &str =
    "Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation.";

/// The same window's note said "its last screen lines named a secret, so they are left out", and
/// the chief never learned the cause (T-823, kluris T-153): it says what the window was
/// running, the program of the last tool call its record shows and not a word of what it was
/// given, and the screen it showed where nothing on it names a secret.
#[test]
fn a_worker_window_that_closed_says_what_it_was_running_and_the_screen_where_no_row_names_a_secret()
{
    let call = cf_harness::records::LastCall::shell(
        "Bash",
        "cargo fmt --check && cargo test --workspace -- --nocapture",
    );
    assert_eq!(
        paused_note(
            3,
            &["Running the workspace tests", "test result: FAILED", "bye"],
            Some(call.clone())
        ),
        format!(
            "T-1 is paused: @zeus's window closed (exit code 3); \
             its last tool call was Bash: \"cargo \u{2026}\"; \
             its screen ended with: \"Running the workspace tests / test result: FAILED / bye\". {RESUME}"
        )
    );
    // One row that names a secret, and the whole screen is withheld, as the base's rule has it.
    assert_eq!(
        paused_note(
            3,
            &[
                "Running the workspace tests",
                "test result: FAILED",
                "export TEST_TOKEN=abc",
                "cargo test exited",
                "bye",
            ],
            Some(call)
        ),
        format!(
            "T-1 is paused: @zeus's window closed (exit code 3); \
             its last tool call was Bash: \"cargo \u{2026}\"; \
             its last screen lines named a secret, so they are left out. {RESUME}"
        )
    );
}

/// The reviews of `a26e0848e` and `860010398` put passwords in a real ledger note with screens
/// the per-row rules quoted. The note the ledger holds says none of them, for the first
/// review's three and the second's three, and says the screen was left out; so for rows that
/// may be a key with no word to say so; so for what the last tool call carries, wrapped in a
/// program the list names or not, and for a base64 value the masker once let through.
#[test]
fn a_pause_note_the_ledger_holds_says_none_of_a_password_the_screen_or_the_last_call_carried() {
    let named = format!(
        "T-1 is paused: @zeus's window closed (exit code 1); its last screen lines named a \
         secret, so they are left out. {RESUME}"
    );
    for (screen, value) in [
        // The first review (T-1216).
        (
            vec!["Password:", "Enter the value below", "red green blue"],
            "red green blue",
        ),
        (
            vec!["PASSWORD: >-", "  correct horse", "  battery staple"],
            "battery staple",
        ),
        (
            vec!["password: correct horse", "battery staple", "alpha bravo"],
            "battery staple",
        ),
        // The second (T-1222): a sentence that trails off, an assignment wrapped by hand, a
        // token wrapped over rows of one short word.
        (
            vec![
                "The password consists of these words",
                "correct",
                "horse",
                "battery",
                "staple",
                "connection failed",
            ],
            "battery",
        ),
        (
            vec![
                "PASSWORD=correct",
                "horse",
                "battery staple",
                "connection failed",
            ],
            "battery staple",
        ),
        (
            vec![
                "API_KEY=sk-abcdefghijkl",
                "mnopqrstuvwx",
                "xyz",
                "connection failed",
            ],
            "mnopqrstuvwx",
        ),
    ] {
        let note = paused_note(1, &screen, None);
        assert_eq!(note, named, "{screen:?}");
        assert!(!note.contains(value), "{screen:?}");
    }

    // Rows that may be a key, with no name and no delimiter in view.
    let key = format!(
        "T-1 is paused: @zeus's window closed (exit code 1); its last screen lines held what \
         may be a key, so they are left out. {RESUME}"
    );
    for (screen, value) in [
        (vec!["YQ==", "Yg=="], "YQ=="),
        (vec!["MIIEvQIBADANBgkq", "hkiG9w0BAQEFAASC"], "hkiG9w0B"),
        (vec!["hkiG9w0BAQEFAASC"], "hkiG9w0B"),
        (
            vec!["abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuv"],
            "abcdefghijkl",
        ),
    ] {
        let note = paused_note(1, &screen, None);
        assert_eq!(note, key, "{screen:?}");
        assert!(!note.contains(value), "{screen:?}");
    }

    // What the last tool call carries: a program the list names is named and nothing after it;
    // one it does not is the tool's name alone.
    for (command, said) in [
        (
            "npm exec -- docker login -p redgreenblue",
            "Bash: \"npm \u{2026}\"",
        ),
        (
            "git -C /w config http.extraHeader X-Session:redgreenblue",
            "Bash: \"git \u{2026}\"",
        ),
        (
            "node login.js alice redgreenblue",
            "Bash: \"node \u{2026}\"",
        ),
        ("curl -u alice:redgreenblue https://example.test/", "Bash"),
        ("TOKEN=redgreenblue cargo test", "Bash"),
    ] {
        let note = paused_note(
            1,
            &["running", "all fine"],
            Some(cf_harness::records::LastCall::shell("Bash", command)),
        );
        assert_eq!(
            note,
            format!(
                "T-1 is paused: @zeus's window closed (exit code 1); its last tool call was {said}; \
                 its screen ended with: \"running / all fine\". {RESUME}"
            ),
            "{command}"
        );
        assert!(
            !note.contains("redgreenblue") && !note.contains("alice") && !note.contains("docker"),
            "{command}"
        );
    }

    // A base64 value with no digit, on a row that names nothing, is masked.
    let note = paused_note(
        1,
        &[
            "running",
            "value YWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJj end",
        ],
        None,
    );
    assert!(!note.contains("YWJjYWJj"), "{note}");
}

/// The note the ledger holds for a screen withheld for a secret it names.
fn named_note() -> String {
    format!(
        "T-1 is paused: @zeus's window closed (exit code 1); its last screen lines named a \
         secret, so they are left out. {RESUME}"
    )
}

/// The note the ledger holds for a screen withheld for what may be a key.
fn key_note() -> String {
    format!(
        "T-1 is paused: @zeus's window closed (exit code 1); its last screen lines held what \
         may be a key, so they are left out. {RESUME}"
    )
}

/// The re-check of `4aeb48564` (T-1224) read these out of the ledger's notes: a token the
/// masker took a piece of, a name the window's wrap split over rows, values in groups, a key
/// between bars, and a padding row beside a body that has no shape of its own. Each is a
/// screen the note withholds whole now; nothing of its value reaches the ledger.
#[test]
fn a_pause_note_the_ledger_holds_says_none_of_a_secret_split_grouped_framed_or_padded() {
    for (screen, value, named) in [
        // A recognized token prefix, whatever follows it.
        (
            vec!["sk-abcdefghijkl mnopqrstuvwx yz1234"],
            "mnopqrstuvwx",
            true,
        ),
        (
            vec!["sk-abcd efgh ijkl mnop qrst uvwx yz12 3456"],
            "efgh ijkl",
            true,
        ),
        // The same URL, written in percent signs.
        (
            vec!["https%3A%2F%2Falice%3Aswordfish%40example.test"],
            "swordfish",
            true,
        ),
        // A name split over the physical rows of the window.
        (
            vec!["{\"api_", "key\": \"red", "green blue\"}"],
            "green blue",
            true,
        ),
        // Values in groups.
        (
            vec!["YWJj-ZGVm-ZGhp-amts-bWpv-cHFy-cHRu-dndv"],
            "ZGVm-ZGhp",
            false,
        ),
        (
            vec!["deadbeef cafebabe feedface abcdefab deadbeef cafebabe feedface abcdefab"],
            "cafebabe",
            false,
        ),
        // The two-row key crop in a frame.
        (
            vec!["│ MIIEvQIBADANBgkq │", "│ hkiG9w0BAQEFAASC │"],
            "hkiG9w0B",
            false,
        ),
        // A body of small letters in rows narrower than twelve.
        (
            vec![
                "abcdefghijk",
                "lmnopqrstuv",
                "wxyzabcdefg",
                "hijklmnopqr",
                "stuv",
            ],
            "lmnopqrstuv",
            false,
        ),
        // The same body in rows of differing widths.
        (
            vec![
                "abcdefghij",
                "klmnopqrstu",
                "vwxyzabcdefg",
                "hijklmnop",
                "qrs",
            ],
            "klmnopqrstu",
            false,
        ),
        // A padding row beside a body of small letters.
        (vec!["abcdefghijklmnopqrstuvw", "="], "abcdefghij", false),
    ] {
        let note = paused_note(1, &screen, None);
        assert_eq!(
            note,
            if named { named_note() } else { key_note() },
            "{screen:?}"
        );
        assert!(!note.contains(value), "{screen:?}");
    }
}

/// The note the ledger holds for a screen that is quoted, `quote` between its marks, with
/// `masked` strings hidden in it.
fn quoted_note(quote: &str, masked: usize) -> String {
    let hid = match masked {
        0 => String::new(),
        1 => " (1 key- or token-like string masked)".to_owned(),
        count => format!(" ({count} key- or token-like strings masked)"),
    };
    format!(
        "T-1 is paused: @zeus's window closed (exit code 1); its screen ended with: \
         \"{quote}\"{hid}. {RESUME}"
    )
}

/// The cases of `cases` whose pause note, as the ledger holds it, is not the whole-screen
/// withholding `named` says (a secret named, else what may be a key), or has a piece of its
/// value in it: one line for each, so that a red run lists every shape and not the first.
fn not_withheld(cases: &[(&[&str], &str)], named: bool) -> Vec<String> {
    let mut failed = Vec::new();
    for (screen, value) in cases {
        // The screen alone, and between rows that name nothing.
        for rows in [
            screen.to_vec(),
            [&["pad one"][..], screen, &["pad two"][..]].concat(),
        ] {
            let note = paused_note(1, &rows, None);
            let wanted = if named { named_note() } else { key_note() };
            if note != wanted || note.contains(value) {
                failed.push(format!("{rows:?}: {note}"));
            }
        }
    }
    failed
}

/// T-1227 found these shapes of a value in groups still quoted, and main quoted them too: groups
/// parted by `_`, `:` or `.`, hex parted by a tab or two blanks.
#[test]
fn a_pause_note_the_ledger_holds_says_none_of_a_value_in_groups_parted_by_any_one_character_or_any_blanks(
) {
    let failed = not_withheld(
        &[
            (&["abcd_efgh_ijkl_mnop"], "efgh_ijkl"),
            (&["abcd:efgh:ijkl:mnop"], "efgh:ijkl"),
            (&["abcd.efgh.ijkl.mnop"], "efgh.ijkl"),
            (&["deadbeef\tcafebabe\tfeedface\tabcdefab"], "cafebabe"),
            (&["deadbeef  cafebabe  feedface  abcdefab"], "cafebabe"),
            (
                &["deadbeef", "cafebabe", "feedface", "abcdefab"],
                "cafebabe",
            ),
        ],
        false,
    );
    assert!(failed.is_empty(), "{failed:#?}");
}

/// T-1227: four words of unequal length in a row of their own are a passphrase as much as four
/// of one length.
#[test]
fn a_pause_note_the_ledger_holds_says_none_of_four_words_in_a_row_of_their_own() {
    let failed = not_withheld(
        &[
            (&["abcd efghi jklmno pqrstuv"], "efghi"),
            (&["correct horse battery staple"], "horse battery"),
        ],
        false,
    );
    assert!(failed.is_empty(), "{failed:#?}");
}

/// T-1227: a short body with a `/` in it, alone or above another, is a key's body as much as
/// one with a digit in it.
#[test]
fn a_pause_note_the_ledger_holds_says_none_of_a_short_body_with_a_slash_in_it() {
    let failed = not_withheld(
        &[
            (&["abcd/efghijkl"], "efghijkl"),
            (&["abcd/efghijkl", "mnopqrst/uvw"], "mnopqrst"),
        ],
        false,
    );
    assert!(failed.is_empty(), "{failed:#?}");
}

/// T-1227: the beginning of a token of a known kind, followed by fewer than four letters or
/// digits, or by punctuation, and the rest of it in groups.
#[test]
fn a_pause_note_the_ledger_holds_says_none_of_a_token_prefix_cut_short_or_followed_by_punctuation()
{
    let failed = not_withheld(
        &[
            (&["sk-abc defg hijk lmno"], "defg hijk"),
            (&["sk-_abc defg hijk lmno"], "defg hijk"),
            (&["ghp_ab cdef ghij klmn"], "cdef ghij"),
            (&["glpat-.ab cdef ghij klmn"], "cdef ghij"),
        ],
        true,
    );
    assert!(failed.is_empty(), "{failed:#?}");
}

/// T-1227: a URL's user with no password is a token as often as a name (`https://TOKEN@host`):
/// it is masked where it stands, and the screen is withheld where the window's wrap cut the
/// URL in two or its characters are written in percent signs.
#[test]
fn a_pause_note_the_ledger_holds_says_none_of_the_user_of_a_url_that_has_no_password() {
    let mut failed = Vec::new();
    for (row, quoted) in [
        (
            "cloned from https://swordfish@vault",
            "cloned from https://[masked]@vault",
        ),
        (
            "https://swordfish@vault.example.test/x.git",
            "https://[masked]@vault.example.test/x.git",
        ),
    ] {
        let note = paused_note(1, &[row], None);
        if note != quoted_note(quoted, 1) {
            failed.push(format!("{row}: {note}"));
        }
    }
    // A name on ssh is a name: the account of `git clone`.
    let note = paused_note(1, &["cloned from ssh://git@vault/repo"], None);
    if note != quoted_note("cloned from ssh://git@vault/repo", 0) {
        failed.push(format!("ssh: {note}"));
    }
    // The wrap cut the URL: the masker sees no URL in either row.
    failed.extend(not_withheld(
        &[
            (&["https://swordfi", "sh@vault"], "swordfi"),
            (&["https://alice:swo", "rdfish@vault"], "rdfish"),
            (&["https%3A%2F%2Fswordfish%40vault"], "swordfish"),
        ],
        true,
    ));
    assert!(failed.is_empty(), "{failed:#?}");
}

/// T-1227: percent signs nested (`%25` for the sign), and a value that is nothing but them.
#[test]
fn a_pause_note_the_ledger_holds_says_none_of_percent_signs_nested_or_alone() {
    let mut failed = not_withheld(
        &[
            (&["api%255Fkey%253Dredgreenblue"], "redgreenblue"),
            (&["sk%252Dabcdefgh%2520ijklmnop"], "ijklmnop"),
            (&["%2570assword%25253Dhunter22"], "hunter22"),
        ],
        true,
    );
    failed.extend(not_withheld(
        &[(&["cobalt%2Dviolet%2Dindigo"], "violet")],
        false,
    ));
    assert!(failed.is_empty(), "{failed:#?}");
}

/// T-1227: `Authentication failed` names a secret as `token` does.
#[test]
fn a_pause_note_the_ledger_holds_says_none_of_what_follows_an_authentication_word() {
    let failed = not_withheld(
        &[
            (&["Authentication failed", "redgreenblue"], "redgreenblue"),
            (
                &["authenticate with the registry", "redgreenblue"],
                "redgreenblue",
            ),
            (&["Unauthenticated: redgreenblue"], "redgreenblue"),
        ],
        true,
    );
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A Claude Code window that quit printed `claude --resume <its session>`: that id is not a
/// secret, and the screen is quoted around it. Any other id in the screen still withholds it
/// whole, and so does the same id on a window whose session ConsensFlow does not know.
#[test]
fn a_pause_note_the_ledger_holds_quotes_the_screen_of_a_claude_window_that_quit_with_its_own_resume_line(
) {
    const OWN: &str = "123e4567-e89b-12d3-a456-426614174000";
    const OTHER: &str = "9f2c1d3e-7a4b-4c5d-8e6f-0a1b2c3d4e5f";
    let resume = format!("Resume this conversation with: claude --resume {OWN}");
    let mut failed = Vec::new();
    // Its own: quoted, and the id is masked as any long id is.
    let note = paused_note_on(1, &["Goodbye!", &resume], None, Some(OWN));
    if note
        != quoted_note(
            "Goodbye! / Resume this conversation with: claude --resume [masked]",
            1,
        )
    {
        failed.push(format!("its own: {note}"));
    }
    // As a harness may print it in capitals.
    let loud = resume.to_uppercase();
    let note = paused_note_on(1, &["Goodbye!", &loud], None, Some(OWN));
    if !note.contains("its screen ended with") {
        failed.push(format!("its own, in capitals: {note}"));
    }
    // Another id withholds the screen, beside the window's own or alone; and so does the
    // window's own where ConsensFlow does not know it, or where it is another window's.
    let other = format!("claude --resume {OTHER}");
    for (rows, session) in [
        (vec!["Goodbye!", other.as_str()], Some(OWN)),
        (vec![resume.as_str(), other.as_str()], Some(OWN)),
        (vec!["Goodbye!", other.as_str()], None),
        (vec!["Goodbye!", resume.as_str()], None),
        (vec!["Goodbye!", resume.as_str()], Some(OTHER)),
    ] {
        let note = paused_note_on(1, &rows, None, session);
        if note != key_note() || note.contains("a456") || note.contains("8e6f") {
            failed.push(format!("{rows:?} {session:?}: {note}"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// What the new rules leave alone, through the same path: the screens of an ordinary day are
/// quoted in the note the ledger holds, as before (cargo's test names, a Rust error, git's log,
/// a connection error, a URL an account clones from, a URL written in percent signs, a quit
/// line with the window's own id on it).
#[test]
fn a_pause_note_the_ledger_holds_still_quotes_the_screens_of_an_ordinary_day() {
    let mut failed = Vec::new();
    for rows in [
        vec![
            "test windows::a_host_shut_down_with_windows_open_tells_the_exit ... ok",
            "test result: ok. 238 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
        ],
        vec![
            "error[E0308]: mismatched types",
            " --> crates/cf-engine/src/shown.rs:258:20",
            "expected `Vec<&str>`, found `Vec<String>`",
        ],
        vec![
            "3e25960a1 A short body with a point. and the rest",
            "error: failed to connect to 127.0.0.1:8080: Connection refused (os error 61)",
        ],
        vec![
            "cloned from ssh://git@vault/repo",
            "file:///My%20Folder/a%2Fb.txt",
        ],
        vec!["Compiling cf-engine v3.0.7-alpha.3 (/Users/me/consensflow/crates/cf-engine)"],
    ] {
        let note = paused_note(1, &rows, None);
        if !note.contains("its screen ended with") || note.contains("[masked]") {
            failed.push(format!("{rows:?}: {note}"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The joint review T-1322, blocker 1: the window's own session id is exempt from the rule of a
/// value in groups only. Taken out of the rows before every check, as it was, it made `https://<id>:swo`
/// / `rdfish@vault` two rows with no URL in either, and the password in them was quoted. Every
/// other check looks at the rows as they were drawn.
#[test]
fn a_pause_note_the_ledger_holds_says_none_of_a_credential_the_windows_own_id_sits_beside() {
    const OWN: &str = "01234567-89ab-4def-8123-456789abcdef";
    let url = format!("https://{OWN}:swo");
    let token = format!("sk-{OWN}");
    let basic = format!("Basic {OWN}");
    let slot = format!("password: {OWN}");
    let mut failed = Vec::new();
    for (rows, value) in [
        // The user of a URL is the id, the password cut in two by the window's wrap.
        (vec![url.as_str(), "rdfish@vault"], "rdfish"),
        (
            vec!["pad one", url.as_str(), "rdfish@vault", "pad two"],
            "rdfish",
        ),
        // The id as the body of a token, of a Basic credential, and in a password's slot.
        (vec![token.as_str()], "-89ab"),
        (vec![basic.as_str()], "89ab"),
        (vec![slot.as_str()], "89ab"),
    ] {
        let note = paused_note_on(1, &rows, None, Some(OWN));
        if note != named_note() || note.contains(value) {
            failed.push(format!("{rows:?}: {note}"));
        }
    }
    // The id alone, as a harness prints it when it quits, still passes.
    let resume = format!("claude --resume {OWN}");
    let note = paused_note_on(1, &["Goodbye!", &resume], None, Some(OWN));
    if !note.contains("its screen ended with") {
        failed.push(format!("its own: {note}"));
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The files a tool worked on are never said in a pause note (T-1224, finding 2): a token split
/// by blanks, a key's body below the masker's size, and credentials written in percent signs
/// went to the ledger in the paths a note quoted.
#[test]
fn a_file_the_last_tool_call_worked_on_is_never_said_in_the_pause_note() {
    for (path, value) in [
        (
            "/tmp/sk-abcd efgh ijkl mnop qrst uvwx yz12 3456.txt",
            "efgh ijkl",
        ),
        ("/tmp/hkiG9w0BAQEFAASC.txt", "hkiG9w0B"),
        (
            "/tmp/https%3A%2F%2Falice%3Aswordfish%40example.test.json",
            "swordfish",
        ),
        ("crates/cf-engine/src/shown.rs", "shown.rs"),
    ] {
        for tool in ["Read", "Edit", "Write"] {
            let note = paused_note(
                1,
                &["running", "all fine"],
                Some(cf_harness::records::LastCall::on(tool, path)),
            );
            assert_eq!(
                note,
                format!(
                    "T-1 is paused: @zeus's window closed (exit code 1); its last tool call was {tool}; \
                     its screen ended with: \"running / all fine\". {RESUME}"
                ),
                "{tool} {path}"
            );
            assert!(!note.contains(value), "{tool} {path}");
        }
    }
}

/// A window's exit reads its conversation's record with no window open, to name the last tool
/// call. A participant whose window was asked for meanwhile, up, on its way or given up, is no
/// business of that exit: it settles nothing more (the guard of `in_flight`'s `launches`),
/// and no pause note says a window closed under a task that another window now has (the
/// review of `a26e0848e`, P3: the fake answered at once, and the guard had no case).
#[test]
fn a_window_that_exits_while_its_record_is_read_and_is_replaced_before_the_read_ends_pauses_nothing(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let session = tiers.assignee(1);
    let first = context.host.last(&session).unwrap().pane;
    let native = native_of(&context, tiers.id(&session)).expect("its conversation");
    context.adapter.with("zeus", |agent| {
        agent.last_call = Some(cf_harness::records::LastCall::shell("Bash", "cargo test"));
    });
    let reading = context
        .adapter
        .last_call_holds
        .hold(move |asked| asked[0] == native.as_str());
    context
        .host
        .exits_showing(Some(1), None, Some(&["Running the workspace tests"]));
    // The window exits; its note waits for the record, which is held.
    let exit = context.begin_exit(&session);
    // The human opens the session's window again meanwhile: a replacement comes up.
    context.open_window(tiers.project.id, &session).unwrap();
    context.pass().unwrap();
    let second = context.host.last(&session).unwrap().pane;
    assert_ne!(second, first, "a replacement window came up");
    reading.open();
    drop(exit);
    context.pass().unwrap();

    let task = tiers.task(1);
    assert_ne!(
        task.task.state, "paused",
        "the replaced window's exit paused nothing"
    );
    assert!(
        task.messages
            .iter()
            .all(|m| !m.body.contains("window closed")),
        "{:?}",
        task.messages.iter().map(|m| &m.body).collect::<Vec<_>>()
    );
}

/// A screen that names a secret puts none of its lines in the pause note, which goes to
/// the board, the chief's window and the log: a passphrase of words is no token, and
/// both reviews of 2026-10-16 found the masker let one syntax after another through.
/// How the window ended is said all the same.
#[test]
fn a_screen_that_names_a_secret_puts_none_of_its_lines_in_the_pause_note() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.host.exits_showing(
        Some(1),
        None,
        Some(&[
            "password: correct horse battery staple",
            "export API_KEY='red green blue'",
            "panicked at src/main.rs:4",
        ]),
    );
    context.exit("zeus");

    let task = context.task(project.id, 1);
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed (exit code 1); its last screen lines named a \
         secret, so they are left out. \
         Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
    );
}

/// A window whose program the host could not end in its bound: the pause note says so in
/// place of how it ended, and that it may still run: a resume opens its window beside it,
/// which nothing refuses.
#[test]
fn a_worker_window_whose_program_could_not_be_ended_says_so_in_its_pause_note() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .host
        .exits_showing(None, None, Some(&["cleaning up"]));
    context.host.exits_with_a_survivor(4242);
    context.exit("zeus");

    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "paused");
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed, but its program (process 4242) could not be \
         ended and may still run; its screen ended with: \"cleaning up\". Resume it with: cf \
         task resume T-1 \"…\"; its window comes back on its own conversation."
    );
}

/// Whether each launch of `handle` was told the program of its window before was
/// proven gone, in the order prepared.
fn told_gone(context: &Context, handle: &str) -> Vec<bool> {
    context
        .adapter
        .prepared()
        .iter()
        .zip(context.adapter.previous_gone())
        .filter(|(launch, _)| launch["participant"]["handle"] == handle)
        .map(|(_, gone)| gone)
        .collect()
}

/// A window opened again on its conversation is told whether the program of the
/// window before it is gone (`Launch::previous_gone`), which a Claude window
/// takes the record's length for: only where the host said how it ended and named
/// no program it could not end. T-442's reviews: one that may still run would
/// write past the length taken, as the new process's.
#[test]
fn a_window_opened_again_is_told_the_program_before_it_is_gone_only_where_the_host_proved_it() {
    let reopened = |says: &dyn Fn(&Context)| {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.give(project.id, "zeus", "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        says(&context);
        context.exit("zeus");
        context.resume_task(project.id, 1, "Carry on");
        context.pass().unwrap();
        told_gone(&context, "zeus")
    };
    let ended = |context: &Context| context.host.exits_showing(Some(0), None, None);
    assert_eq!(reopened(&ended), [false, true], "ended, and gone");
    let outlived = |context: &Context| {
        ended(context);
        context.host.exits_with_a_survivor(4242);
    };
    assert_eq!(reopened(&outlived), [false, false], "may still run");
    assert_eq!(
        reopened(&|_: &Context| {}),
        [false, false],
        "a host that said nothing of the end"
    );
}

/// The host's proof outlives the daemon that heard it (T-526, 2026-10-25): a worker's
/// window closed with its program proven gone, an update restarted the daemon, and the
/// chief's reopen opened the conversation again under the new daemon, which never saw the
/// close. Told nothing, Claude Code's reader kept the notification the closed process had
/// queued and taken, and read the reopen's answered turn as still under way for four
/// hours. The new daemon is told what the old one proved, and nothing it did not: a
/// program the host could not end, or a host that said nothing, proves nothing after a
/// restart either.
#[test]
fn a_window_opened_again_after_a_restart_is_told_the_program_before_it_is_gone_where_the_daemon_before_proved_it(
) {
    let reopened = |says: &dyn Fn(&Context)| {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.give(project.id, "zeus", "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        says(&context);
        context.exit("zeus");
        // The app is updated: its daemon stops with no window open, and a new one starts.
        context.ledger.borrow_mut().suspend_for_restart().unwrap();
        let after = context.make();
        after.settle_after_restart().unwrap();
        after.resume_project(project.id).unwrap();
        context.resume_task(project.id, 1, "Carry on");
        after.pass().unwrap();
        after.pass().unwrap();
        told_gone(&context, "zeus")
    };
    let ended = |context: &Context| context.host.exits_showing(Some(0), None, None);
    assert_eq!(reopened(&ended), [false, true], "ended, and gone");
    let outlived = |context: &Context| {
        ended(context);
        context.host.exits_with_a_survivor(4242);
    };
    assert_eq!(reopened(&outlived), [false, false], "may still run");
    assert_eq!(
        reopened(&|_: &Context| {}),
        [false, false],
        "a host that said nothing of the end"
    );
}

/// The proof the ledger keeps is spent as the next window opens: a daemon started while
/// that window's program may still run (its end never heard) is told nothing of the
/// program before it.
#[test]
fn the_proof_a_restart_finds_is_spent_by_the_window_that_opened_after_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.host.exits_showing(Some(0), None, None);
    context.exit("zeus");
    context.resume_task(project.id, 1, "Carry on");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(told_gone(&context, "zeus"), [false, true]);

    // The daemon stops with that window open, and no word of its end ever comes.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project.id).unwrap();
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(told_gone(&context, "zeus"), [false, true, false]);
}

/// The engine's own close settles a window before the host says how it ended: the
/// host's word, when it comes, is what the next launch is told.
#[test]
fn the_hosts_word_on_a_window_the_engine_closed_itself_is_what_its_next_launch_is_told() {
    let closed_then = |word: bool| {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.give(project.id, "zeus", "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        context.host.exits_showing(Some(0), None, None);
        context.host.hold_exits.set(true);
        context.close_project(project.id).unwrap();
        context.host.hold_exits.set(false);
        if word {
            context.exit("zeus");
        }
        // The Resume brings back T-1, which the Close paused.
        context.resume_project(project.id).unwrap();
        context.pass().unwrap();
        context.pass().unwrap();
        context.pass().unwrap();
        told_gone(&context, "zeus")
    };
    assert_eq!(closed_then(true), [false, true]);
    assert_eq!(closed_then(false), [false, false]);
}

/// The same, with a restart between the host's word and the next launch: the word the
/// host gave after the engine's own close is kept for the daemon after it too.
#[test]
fn the_hosts_word_on_a_window_the_engine_closed_itself_is_kept_for_the_daemon_after_a_restart() {
    let closed_then = |word: bool| {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.give(project.id, "zeus", "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        context.host.exits_showing(Some(0), None, None);
        context.host.hold_exits.set(true);
        context.close_project(project.id).unwrap();
        context.host.hold_exits.set(false);
        if word {
            context.exit("zeus");
        }
        context.ledger.borrow_mut().suspend_for_restart().unwrap();
        let after = context.make();
        after.settle_after_restart().unwrap();
        // The Resume brings back T-1, which the Close paused.
        after.resume_project(project.id).unwrap();
        after.pass().unwrap();
        after.pass().unwrap();
        after.pass().unwrap();
        told_gone(&context, "zeus")
    };
    assert_eq!(closed_then(true), [false, true]);
    assert_eq!(closed_then(false), [false, false]);
}

/// What the close ended goes between how the window ended and its screen.
#[test]
fn a_worker_window_that_ends_mid_task_with_work_running_says_both() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.host.exits_showing(Some(1), None, Some(&["Killed"]));
    context.host.exits_leaving(&["npm test -- --watch"]);
    context.exit("zeus");

    let task = context.task(project.id, 1);
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_match(
        &note.body,
        r#"^T-1 is paused: @zeus's window closed \(exit code 1\) with background work running, which its close ended: npm …; its screen ended with: "Killed"\. Resume it with"#,
    );
}

#[test]
fn brings_a_tell_to_a_task_whose_window_is_gone_on_that_windows_own_conversation_still_paused() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = native(&context, project.id, "zeus");
    context.exit("zeus");
    assert_eq!(context.task(project.id, 1).task.state, "paused");
    let told = context.tell(project.id, "zeus", 1, "Which grammar did you start from?");
    context.pass().unwrap();
    assert_eq!(
        resumed(&context),
        (json!("zeus"), json!(session)),
        "the same conversation"
    );
    let launch = context.adapter.prepared().pop().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"^\[ConsensFlow m-\d+ · T-1 · question from @chief\]\nWhich grammar did you start from\?\n\nT-1 is paused for this\.",
    );
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "paused",
        "the task waits for the chief"
    );
    assert_ne!(context.message(told.id).state, "queued");
    held_to(
        context.close(),
        SUITES,
        "brings a tell to a task whose window is gone on that window's own conversation, still paused",
    );
}

#[test]
fn pauses_a_working_task_in_its_open_window() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let launches = context.adapter.prepared().len();
    context.pause_task(project.id, 1);
    context.pass().unwrap();
    context.pass().unwrap();
    let pane = context.host.last("zeus").unwrap().pane;
    assert_eq!(
        context.host.inputs(),
        [json!({ "id": pane.id, "generation": pane.generation, "bytes": [27] })],
        "Escape, once"
    );
    assert!(context.host.killed().is_empty(), "the window stays");
    // Still working three seconds later (a harness that ignored the key while it thought): again, up to three times.
    context.adapter.busy("zeus");
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        2,
        "pressed again while the window still works"
    );
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 3, "and then no more");
    // Its turn ends after the third round, before the look that would close it:
    // paid at rest, and the window stays.
    context.advance(2_000);
    context.adapter.answer("zeus", "Parser done");
    context.advance(1_100);
    context.pass().unwrap();
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 3);
    assert!(
        context.host.killed().is_empty(),
        "the window stopped: it stays"
    );
    let zeus = context.id(project.id, "zeus");
    assert_eq!(context.dispatcher.unstopped(zeus), None);
    assert_eq!(
        context.task(project.id, 1).task.state,
        "paused",
        "said after the pause: not a result"
    );
    context.resume_task(project.id, 1, "Add the tests too");
    context.pass().unwrap();
    assert_eq!(context.adapter.prepared().len(), launches, "no new window");
    let last = context.adapter.agent("zeus").items.pop().unwrap();
    assert_match(
        &last.text,
        r"T-1 · task from @chief\]\nResumed: Add the tests too$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser and tests done");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");
    // Not held to Node's recording: its stop is pressed in rounds (`stops.rs`).
}

#[test]
fn fails_the_task_when_the_worker_window_cannot_open() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.host.refuse.set(true);
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "failed");
    assert_match(
        task.messages[0].reason.as_deref().unwrap(),
        "the window did not open: the host would not open it",
    );
    held_to(
        context.close(),
        SUITES,
        "fails the task when the worker window cannot open",
    );
}

#[test]
fn forgets_the_files_a_launch_wrote_when_its_window_never_opens() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.host.refuse.set(true);
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "failed");
    let launched = context.adapter.prepared().pop().unwrap();
    let forgotten = || context.launch_files.forgotten.borrow().clone();
    assert!(
        forgotten().contains(&launched["launchId"].as_str().unwrap().to_owned()),
        "the host refused the window"
    );

    let written = Rc::new(RefCell::new(String::new()));
    let seen = Rc::clone(&written);
    *context.adapter.prepare.borrow_mut() = Some(Rc::new(move |asked| {
        *seen.borrow_mut() = asked["launchId"].as_str().unwrap().to_owned();
        Err("the settings could not be written".to_owned())
    }));
    context.give(project.id, "zeus", "Lexer");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 2).task.state, "failed");
    assert!(
        forgotten().contains(&written.take()),
        "the adapter failed after writing them"
    );
    held_to(
        context.close(),
        SUITES,
        "forgets the files a launch wrote when its window never opens: refused by the host, or its adapter failed",
    );
}

#[test]
fn fails_a_launch_whose_first_message_never_arrives() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    // The window opens, but its record never shows the brief.
    let fake = Rc::downgrade(&context.adapter);
    *context.adapter.after_prepare.borrow_mut() = Some(Rc::new(move |handle| {
        if let Some(fake) = fake.upgrade() {
            fake.with(handle, |agent| agent.items.clear());
        }
    }));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.advance(121_000);
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "failed");
    assert_eq!(
        context.host.killed().last().unwrap().id,
        context.host.last("zeus").unwrap().pane.id
    );
    held_to(
        context.close(),
        SUITES,
        "fails a launch whose first message never arrives",
    );
}

#[test]
fn fails_the_first_message_at_once_when_the_harness_cannot_take_it_after_the_window_opens() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    *context.adapter.started.borrow_mut() =
        Some(Rc::new(|| Err("the server never answered".to_owned())));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "failed");
    assert_match(
        task.messages[0].reason.as_deref().unwrap(),
        "the window could not take its first message \\(its cause is in ConsensFlow's log\\)",
    );
    assert_eq!(
        context.host.killed().last().unwrap().id,
        context.host.last("zeus").unwrap().pane.id
    );
    held_to(
        context.close(),
        SUITES,
        "fails the first message at once when the harness cannot take it after the window opens",
    );
}

/// What `cf codex-session` writes on its terminal when it could not open Codex because the npm
/// shim has no Node to run on (`ConsensFlow could not open Codex: …`, the person's own words).
fn codex_could_not_open(shim: &str) -> String {
    format!(
        "ConsensFlow could not open Codex: {shim} is an npm shim that runs on Node, and ConsensFlow finds no Node for it: none is beside it, and none is on the PATH ConsensFlow runs with. Make the harness's Node visible to ConsensFlow, or install the harness's own build instead of the npm one."
    )
}

/// `text` as a terminal of `columns` columns draws it: hard wrapped, a row of its own each.
fn hard_wrapped(text: &str, columns: usize) -> Vec<String> {
    text.chars()
        .collect::<Vec<_>>()
        .chunks(columns)
        .map(|row| row.iter().collect())
        .collect()
}

/// T-1328, the second reading of T-1322: a Codex window whose program (`cf codex-session`) could not
/// open Codex names the shim it tried on its screen, and a screen note quoted it at every width,
/// `hunter22` and all. The window ends with the status of why (72, no Node), which the engine reads:
/// the harness never ran, and the note says what failed and what to do, from the status, and quotes
/// nothing of the screen. A Codex window that ends with any other status, or another harness's with
/// this one, is quoted as before.
#[test]
fn a_codex_window_that_could_not_open_codex_says_why_from_how_it_ended_and_quotes_no_screen() {
    let shim = r"C:\tools\hunter22\codex.cmd";
    let words = codex_could_not_open(shim);
    let said = "Node.js was not found for Codex; make the harness's Node visible to ConsensFlow, or install the harness's own build instead of the npm one";
    let mut failed = Vec::new();
    for columns in [80, 120, 160] {
        let screen = hard_wrapped(&words, columns);
        let rows: Vec<&str> = screen.iter().map(String::as_str).collect();
        let context = with_codex();
        let project = context
            .open_project(json!({
                "directory": "/work/app",
                "name": "app",
                "chief": { "harness": "claude-code", "model": "claude-opus-5" },
                "staff": [{ "agent": "diana", "harness": "codex", "roles": ["worker"], "tier": "standard" }],
            }))
            .unwrap()
            .expect("the project")
            .id;
        context.pool_task(project, "worker", "Write the parser");
        context.pass().unwrap();
        context.pass().unwrap();
        let session = context.task(project, 1).task.assignee.expect("its session");
        context.host.exits_showing(Some(72), None, Some(&rows));
        context.exit(&session);
        let note = context
            .task(project, 1)
            .messages
            .iter()
            .find(|message| message.kind == "note")
            .map(|message| message.body.clone())
            .unwrap_or_default();
        if !note.contains(&format!("(exit code 72); {said}")) {
            failed.push(format!("{columns} columns, not said: {note}"));
        }
        for word in [
            "hunter22",
            "tools",
            "codex.cmd",
            "ConsensFlow could not open",
        ] {
            if note.contains(word) {
                failed.push(format!("{columns} columns: `{word}` is in {note}"));
            }
        }
        context.close();
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A Codex window a signal ended is not one that could not start Codex, whatever code the host
/// also named: the status of why is read only where no signal ended the program.
#[test]
fn a_status_of_why_is_not_read_of_a_window_a_signal_ended() {
    let context = with_codex();
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "model": "claude-opus-5" },
            "staff": [{ "agent": "diana", "harness": "codex", "roles": ["worker"], "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project")
        .id;
    context.pool_task(project, "worker", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = context.task(project, 1).task.assignee.expect("its session");
    context.host.exits_showing(
        Some(72),
        Some("SIGTERM"),
        Some(&["Running the workspace tests"]),
    );
    context.exit(&session);
    let note = context
        .task(project, 1)
        .messages
        .iter()
        .find(|message| message.kind == "note")
        .map(|message| message.body.clone())
        .unwrap_or_default();
    assert!(!note.contains("Node.js was not found"), "{note}");
    assert!(note.contains("Running the workspace tests"), "{note}");
    context.close();
}

/// The same status from a window of any other harness is that harness's own: its screen is quoted
/// as it is, and a Codex window's other statuses are Codex's own.
#[test]
fn a_status_of_why_is_read_only_of_a_codex_window() {
    for (harness, code) in [
        ("claude-code", 72),
        ("codex", 1),
        ("codex", 69),
        ("codex", 78),
    ] {
        let context = with_codex();
        let project = context
            .open_project(json!({
                "directory": "/work/app",
                "name": "app",
                "chief": { "harness": "claude-code", "model": "claude-opus-5" },
                "staff": [{ "agent": "diana", "harness": harness, "roles": ["worker"], "tier": "standard" }],
            }))
            .unwrap()
            .expect("the project")
            .id;
        context.pool_task(project, "worker", "Write the parser");
        context.pass().unwrap();
        context.pass().unwrap();
        let session = context.task(project, 1).task.assignee.expect("its session");
        context
            .host
            .exits_showing(Some(code), None, Some(&["Running the workspace tests"]));
        context.exit(&session);
        let note = context
            .task(project, 1)
            .messages
            .iter()
            .find(|message| message.kind == "note")
            .map(|message| message.body.clone())
            .unwrap_or_default();
        assert!(
            note.contains(&format!(
                "(exit code {code}); its screen ended with: \"Running the workspace tests\""
            )),
            "{harness} {code}: {note}"
        );
        assert!(!note.contains("Node.js was not found"), "{note}");
        context.close();
    }
}
