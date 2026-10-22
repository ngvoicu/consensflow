//! What a failure note says of a window's screen and exit, in the words it says
//! them: the quote, its bounds, an empty screen, a host that said nothing, and
//! a secret on the screen.

use serde_json::json;

use super::*;

/// A screen that names no secret: a model the account cannot use.
const PI: [&str; 2] = [
    "Model gpt-9 is not available on your plan.",
    "Run /model to choose another one.",
];

/// What a note says in place of a quote of lines that name a secret.
const LEFT_OUT: &str = "its last screen lines named a secret, so they are left out";

fn shown(code: Option<u32>, signal: Option<&str>, tail: Option<&[&str]>) -> Shown {
    Shown {
        code,
        signal: signal.map(str::to_owned),
        tail: tail.map(|lines| lines.iter().map(|line| (*line).to_owned()).collect()),
        background: Vec::new(),
        survivor: None,
        pasted: false,
    }
}

#[test]
fn a_window_that_closed_says_its_code_and_what_its_screen_ended_with() {
    assert_eq!(
        shown(Some(3), None, Some(&PI)).after("@zeus's window closed"),
        "@zeus's window closed (exit code 3); its screen ended with: \
         \"Model gpt-9 is not available on your plan. / Run /model to choose another one.\""
    );
}

#[test]
fn a_window_that_never_showed_its_message_has_a_screen_and_no_code() {
    assert_eq!(
        shown(None, None, Some(&PI)).after("the window never showed its first message"),
        "the window never showed its first message; its screen ended with: \
         \"Model gpt-9 is not available on your plan. / Run /model to choose another one.\""
    );
}

/// Every credential both reviews of 2026-10-16 found the masker let through, and every
/// diagnostic they found it hid: where a line names a secret-like word, the note quotes
/// none of the lines, and says so; how the program ended is said all the same.
#[test]
fn a_screen_that_names_a_secret_is_quoted_not_at_all_and_its_code_is_said() {
    let screens: [&[&str]; 13] = [
        // Round 1.
        &[r#"DB_PASSWORD="alpha bravo charlie""#],
        &[r#"DB_PASSWORD="correct horse battery staple""#],
        &["DB_PASSWORD='red green blue'"],
        // Round 2: what the quoted rule left.
        &["password: a b"],
        &["password: correct horse battery staple"],
        &["password: 'it''s a secret'"],
        &["password: 'alpha''bravo charlie'"],
        &[r#"PASSWORD="alpha bravo\"#],
        // A value that goes on over a line keeps no name on the line it ends on.
        &[r#"DB_PASSWORD="alpha"#, r#"bravo charlie""#],
        // Round 2: what the quoted rule hid that names one.
        &[r#"Invalid token: "expired""#],
        &[r#"private_key_path: "/tmp/id_rsa""#],
        &[r#"SyntaxError: unexpected token: "}""#],
        // An identifier, a header, and the word in a sentence above the last line.
        &["Authorization: Bearer abc", "exited", "bye"],
    ];
    for screen in screens {
        let said = shown(Some(1), None, Some(screen)).after("@zeus's window closed");
        assert_eq!(
            said,
            format!("@zeus's window closed (exit code 1); {LEFT_OUT}"),
            "{screen:?}"
        );
    }
    // A Windows crash code, and no code at all.
    assert_eq!(
        shown(Some(0xC000_0005), None, Some(&["API_KEY=x"])).after("gone"),
        format!("gone (exit code 0xC0000005); {LEFT_OUT}")
    );
    assert_eq!(
        shown(None, None, Some(&["No API key found"])).after("gone"),
        format!("gone; {LEFT_OUT}")
    );
}

/// A line is looked at for the words in any case, as a word or inside a name, and every
/// line the host sent is, not only those the quote would keep.
#[test]
fn the_words_are_found_in_any_case_inside_a_name_and_on_any_line_the_host_sent() {
    for line in [
        "PASSWD=x",
        "passphrase",
        "client_secret",
        "AccessToken",
        "apiKey",
        "api key",
        "API-KEY",
        "PrivateKey",
        "private key",
        "credentials.json",
        "OAuth",
        "BEARER",
    ] {
        assert!(names_a_secret(line), "{line}");
    }
    let mut tail = vec!["export DB_PASSWORD=x".to_owned()];
    tail.extend((1..=10).map(|number| format!("line {number}")));
    let said = Shown {
        tail: Some(tail),
        ..Shown::default()
    }
    .after("gone");
    assert_eq!(said, format!("gone; {LEFT_OUT}"));
}

/// The third review's: blanks between the words of a key's name hide nothing (a look that
/// allowed one let `API    key: abcdefghijklmno` through, and the quote, which makes the
/// blanks one, showed the value whole); the names of a secret given in the forms a
/// screen writes them in are found; and a word a secret's name is a piece of is no
/// secret, its screen quoted.
#[test]
fn the_third_reviews_lines_are_left_out_or_quoted_by_their_words() {
    for screen in [
        "API    key: abcdefghijklmno",
        "DB_PASSWORD=x",
        "OAuth: refresh failed",
        "Authorization: Basic xyz",
        "apiKey = abc",
        "x-api-key: abc",
    ] {
        assert_eq!(
            shown(Some(1), None, Some(&[screen])).after("gone"),
            format!("gone (exit code 1); {LEFT_OUT}"),
            "{screen}"
        );
    }
    for screen in [
        "TypeError: author is undefined at src/author.ts:42",
        "Author identity unknown",
        "panicked at src/author.rs:7",
        "the secretary of the build",
    ] {
        assert_eq!(
            shown(Some(1), None, Some(&[screen])).after("gone"),
            format!("gone (exit code 1); its screen ended with: \"{screen}\""),
            "{screen}"
        );
    }
}

/// The fourth reviews': a password's name in a name of any case or split, and API keys in
/// the plural, named nothing, and the masker misses short words, so each value was quoted
/// whole. They are left out; `secretary`, `tokenizer` and `author`, which hold a secret's
/// word as a piece, are quoted still.
#[test]
fn the_fourth_reviews_lines_are_left_out_and_the_pieces_of_words_quoted() {
    for screen in [
        "DB_PWD=red green blue",
        "API keys: abcdefghijklmno",
        "MYSQL_PWD=\"red green blue\"",
        "PassWord=\"alpha bravo charlie\"",
        "dbPassword: alpha bravo charlie",
    ] {
        assert_eq!(
            shown(Some(1), None, Some(&[screen])).after("gone"),
            format!("gone (exit code 1); {LEFT_OUT}"),
            "{screen}"
        );
    }
    for screen in [
        "the secretary of the build",
        "the tokenizer failed",
        "TypeError: author is undefined",
    ] {
        assert_eq!(
            shown(Some(1), None, Some(&[screen])).after("gone"),
            format!("gone (exit code 1); its screen ended with: \"{screen}\""),
            "{screen}"
        );
    }
}

/// A program the host could not end is said in place of how it ended: it may still run.
/// Nothing is said to wait for it, for nothing does (the host's registry of such programs
/// is gone, round 5).
#[test]
fn a_program_the_host_could_not_end_is_said_and_its_screen_after_it() {
    let survived = Shown {
        survivor: Some(4242),
        ..shown(Some(1), None, Some(&["cleaning up"]))
    };
    assert_eq!(
        survived.after("@zeus's window closed"),
        "@zeus's window closed, but its program (process 4242) could not be ended and may \
         still run; its screen ended with: \"cleaning up\""
    );
    let unnumbered = Shown {
        survivor: Some(0),
        ..Shown::default()
    };
    assert_eq!(
        unnumbered.after("gone"),
        "gone, but its program could not be ended and may still run"
    );
    assert!(unnumbered.is_known());
}

/// A screen with no such word is quoted as before, the reviews' diagnostics among them,
/// masked as the masker always has (a `key` name's long value is hidden: `key_file`'s).
#[test]
fn a_screen_that_names_no_secret_is_quoted_as_before() {
    for (screen, after) in [
        (r#"key = "Escape""#, r#""key = 'Escape'""#),
        (
            r#"const monkey = "banana";"#,
            r#""const monkey = 'banana';""#,
        ),
        (
            r#"key_file: "/home/alice/.ssh/id_ed25519""#,
            r#""key_file: '[masked]'" (1 key- or token-like string masked)"#,
        ),
        ("codex: command not found", r#""codex: command not found""#),
    ] {
        assert_eq!(
            shown(Some(127), None, Some(&[screen])).after("gone"),
            format!("gone (exit code 127); its screen ended with: {after}"),
            "{screen}"
        );
    }
}

#[test]
fn a_program_a_signal_ended_is_said_by_the_signal_and_not_the_code_it_was_given() {
    assert_eq!(
        shown(Some(1), Some("Hangup: 1"), Some(&["bye"])).after("@zeus's window closed"),
        "@zeus's window closed (ended by signal Hangup: 1); its screen ended with: \"bye\""
    );
}

#[test]
fn a_windows_status_is_written_as_windows_writes_it_and_a_program_s_own_code_as_it_is() {
    assert_eq!(
        shown(Some(0xC000_013A), None, None).after("@zeus's window closed"),
        "@zeus's window closed (exit code 0xC000013A)"
    );
    assert_eq!(
        shown(Some(0x8000_0003), None, None).after("gone"),
        "gone (exit code 0x80000003)"
    );
    assert_eq!(
        shown(Some(0x7FFF_FFFF), None, None).after("gone"),
        "gone (exit code 2147483647)"
    );
}

#[test]
fn what_else_is_said_goes_between_how_the_program_ended_and_its_screen() {
    let besides = " with background work still running: sleep 9";
    assert_eq!(
        shown(Some(3), None, Some(&["panicked"])).after_with("@zeus's window closed", besides),
        "@zeus's window closed (exit code 3) with background work still running: sleep 9; \
         its screen ended with: \"panicked\""
    );
    // A host that said nothing leaves it as it was said.
    assert_eq!(
        Shown::default().after_with("@zeus's window closed", besides),
        "@zeus's window closed with background work still running: sleep 9"
    );
    assert_eq!(
        shown(Some(3), None, Some(&["panicked"])).after_with("gone", ""),
        shown(Some(3), None, Some(&["panicked"])).after("gone")
    );
}

#[test]
fn a_screen_with_nothing_on_it_says_so_plainly() {
    for tail in [&[][..], &["", "   ", "\t"][..]] {
        assert_eq!(
            shown(Some(0), None, Some(tail)).after("the window closed"),
            "the window closed (exit code 0); its screen was empty"
        );
    }
    assert_eq!(
        shown(None, None, Some(&[])).after("the window closed"),
        "the window closed; its screen was empty"
    );
}

#[test]
fn a_host_that_did_not_say_adds_nothing() {
    let silent = Shown::default();
    assert!(!silent.is_known());
    assert_eq!(silent.after("the window closed"), "the window closed");
    assert!(shown(Some(0), None, None).is_known());
    assert!(
        shown(None, None, Some(&[])).is_known(),
        "an empty screen was said"
    );
    assert_eq!(
        shown(Some(2), None, None).after("the window closed"),
        "the window closed (exit code 2)"
    );
}

#[test]
fn only_the_last_lines_are_quoted() {
    let tail: Vec<String> = (1..=20).map(|number| format!("line {number}")).collect();
    let lines: Vec<&str> = tail.iter().map(String::as_str).collect();
    assert_eq!(
        shown(None, None, Some(&lines)).after("gone"),
        "gone; its screen ended with: \
         \"line 15 / line 16 / line 17 / line 18 / line 19 / line 20\""
    );
}

#[test]
fn a_quote_is_held_to_its_characters_and_the_older_lines_go_first() {
    let long = "w".repeat(190);
    let lines = ["the oldest", long.as_str(), long.as_str(), "newest of them"];
    let said = shown(None, None, Some(&lines)).after("gone");
    let quote = said
        .strip_prefix("gone; its screen ended with: \"")
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("{said}"));
    // The newest and the two long ones, with their separators, are the 400
    // there is room for: the oldest does not fit, and is left out whole.
    assert_eq!(quote, format!("{long} / {long} / newest of them"));
    assert_eq!(quote.chars().count(), QUOTE_CHARS);
}

#[test]
fn a_last_line_longer_than_the_quote_is_cut_with_a_mark() {
    let said = shown(
        None,
        None,
        Some(&["start of a very long line", &"x".repeat(5_000)]),
    )
    .after("gone");
    let quote = said
        .strip_prefix("gone; its screen ended with: \"")
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("{said}"));
    assert_eq!(quote.chars().count(), QUOTE_CHARS);
    assert!(quote.ends_with('…'));
    assert!(
        !quote.contains("start of"),
        "the older line did not fit beside it"
    );
}

#[test]
fn blanks_become_one_and_quotation_marks_become_apostrophes() {
    assert_eq!(
        shown(None, None, Some(&["  model      \"claude\"    ready  "])).after("gone"),
        "gone; its screen ended with: \"model 'claude' ready\""
    );
}

#[test]
fn a_key_looking_string_is_masked_and_the_note_counts_what_it_hid() {
    let said = shown(
        Some(1),
        None,
        Some(&[
            "request with sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv was refused",
            "id 0123456789abcdef0123456789abcdef in the log",
        ]),
    )
    .after("the window closed");
    assert_eq!(
        said,
        "the window closed (exit code 1); its screen ended with: \
         \"request with [masked] was refused / id [masked] in the log\" \
         (2 key- or token-like strings masked)"
    );
    assert!(!said.contains("AbCdEf") && !said.contains("0123456789abcdef"));
    let one = shown(
        None,
        None,
        Some(&["cloned with ghp_abcdefghijklmnopqrstuvwxyz0123456789"]),
    )
    .after("gone");
    assert!(
        one.ends_with("(1 key- or token-like string masked)"),
        "{one}"
    );
}

#[test]
fn a_secret_in_a_line_the_quote_leaves_out_is_not_counted() {
    let secret = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let mut tail = vec![format!("first {secret}")];
    tail.extend((1..=6).map(|number| format!("line {number}")));
    let lines: Vec<&str> = tail.iter().map(String::as_str).collect();
    let said = shown(None, None, Some(&lines)).after("gone");
    assert!(!said.contains("masked"), "{said}");
    assert!(!said.contains("ghp_"), "{said}");
}

#[test]
fn a_secret_cut_in_the_middle_by_the_bound_is_hidden_before_it_is_cut() {
    let secret = format!("sk-{}", "A1".repeat(30));
    let line = format!("{} {secret}", "x".repeat(380));
    let said = shown(None, None, Some(&[line.as_str()])).after("gone");
    assert!(!said.contains("sk-"), "{said}");
    assert!(!said.contains("A1A1"), "{said}");
}

#[test]
fn an_exit_the_host_sent_is_what_the_engine_keeps() {
    let exit: PaneExit = serde_json::from_value(json!({
        "id": "p1-zeus", "generation": 2, "exitCode": 3, "tail": ["No API key found"],
    }))
    .unwrap();
    assert_eq!(
        Shown::from_exit(exit),
        shown(Some(3), None, Some(&["No API key found"]))
    );
}

#[test]
fn a_snapshot_answer_gives_its_tail_when_it_was_a_success() {
    assert_eq!(
        Shown::from_snapshot(&json!({ "ok": true, "unsent": false, "tail": ["stuck"] })),
        shown(None, None, Some(&["stuck"]))
    );
    assert_eq!(
        Shown::from_snapshot(&json!({ "ok": true, "unsent": false })),
        Shown::default(),
        "a host that gave no tail"
    );
    assert_eq!(
        Shown::from_snapshot(&json!({ "ok": false, "error": "stale", "tail": ["no"] })),
        Shown::default(),
        "a refusal says nothing"
    );
    assert_eq!(
        Shown::from_snapshot(&json!({ "ok": true, "tail": "not a list" })),
        Shown::default()
    );
}

#[test]
fn what_changes_by_itself_in_an_outage_is_folded_and_every_other_number_kept() {
    let key = |words: &str| words_key(words);
    assert_eq!(
        key("Overloaded, retry in 12s"),
        key("Overloaded, retry in 7s")
    );
    assert_eq!(key("retrying in 30 s"), key("retrying in 5 minutes"));
    assert_eq!(key("limit resets at 14:05"), key("limit resets at 9:30:12"));
    assert_eq!(
        key("Overloaded (request_id: req_011CVabcd1234efgh5678)"),
        key("Overloaded (request_id: req_011CWzyxw9876vuts5432)")
    );
    assert_ne!(key("model gpt-5 not found"), key("model gpt-6 not found"));
    assert_ne!(
        key("API Error: 500 Internal error"),
        key("API Error: 502 Internal error")
    );
    assert_eq!(
        key("rate_limit_exceeded_error"),
        "rate_limit_exceeded_error"
    ); // A model's id, a dated one, and a code joined by `_` are the failure's own.
    for kept in [
        "model claude-sonnet-5-5 not found",
        "model claude-sonnet-4-5-20250929 not found",
        "model gpt-5-codex-high not found",
        "ERR_HTTP2_GOAWAY_SESSION",
    ] {
        assert_eq!(key(kept), kept);
    }
    // A completion's, a response's, a hex and a UUID id change by themselves.
    for (one, other) in [
        (
            "id chatcmpl-AbC123dEf456GhI789",
            "id chatcmpl-ZyX987wVu654TsR321",
        ),
        (
            "id resp_68af1c2d3e4f5a6b7c8d9e0f",
            "id resp_11aa22bb33cc44dd55ee66ff",
        ),
        (
            "trace 9f86d081884c7d659a2feaa0c55ad015",
            "trace 2c26b46b68ffc68ff99b453c1d304134",
        ),
        (
            "session 123e4567-e89b-12d3-a456-426614174000",
            "session 9b2f1c3e-0d4a-4b5c-8e6f-7a8b9c0d1e2f",
        ),
    ] {
        assert_eq!(key(one), key(other), "{one}");
    }
}

#[test]
fn what_a_screen_says_is_the_windows_lines_after_the_newest_paste() {
    // A long wake-up's tail and its error above a short wake-up's header, body and
    // error (Calliope's re-check T-300, P2-R1): the newest paste's error alone.
    let pastes = [
        Paste {
            id: 9,
            text: "[ConsensFlow m-9 · note from ConsensFlow]\nWake up".to_owned(),
        },
        Paste {
            id: 8,
            text: "[ConsensFlow m-8 · note from ConsensFlow]\nitem 14 of the board\nitem 15 of the board"
                .to_owned(),
        },
    ];
    let screen = shown(
        None,
        None,
        Some(&[
            "item 14 of the board",
            "item 15 of the board",
            "Error: x",
            "[ConsensFlow m-9 · note from ConsensFlow]",
            "Wake up",
            "Error: x",
            "status: idle",
        ]),
    );
    assert_eq!(
        screen.without_pastes(&pastes).after("failed"),
        "failed; its screen ended with: \"Error: x / status: idle\""
    );
}

#[test]
fn a_footers_counters_are_folded_and_keep_one_failure_one() {
    let key = |words: &str| words_key(words);
    assert_eq!(
        key("Error: x / 12% context left"),
        key("Error: x / 9% context left")
    );
    assert_eq!(key("Error: x / $0.03 spent"), key("Error: x / $0.11 spent"));
    assert_eq!(key("Error: x / 1.2k tokens"), key("Error: x / 34k tokens"));
    assert_eq!(
        key("Error: x / 12,345 tokens"),
        key("Error: x / 13,001 tokens")
    );
    assert_ne!(key("API Error: 500"), key("API Error: 502"));
}
