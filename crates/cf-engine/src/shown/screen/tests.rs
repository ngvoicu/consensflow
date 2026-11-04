//! A screen that names a secret, put in a note (T-823, the review of `a26e0848e`): for every
//! shape of a value the review tried or the chief named, no part of it reaches the note, and
//! the screens the extent of which is known are still quoted for the rest of their rows.

use crate::shown::Shown;

/// The sentence a screen withheld whole is told in.
const WHOLE: &str =
    "gone (exit code 1); its last screen lines named a secret, so they are left out";

/// What the note says of a window that ended with `screen` on it.
fn note(screen: &[&str]) -> String {
    Shown {
        code: Some(1),
        tail: Some(screen.iter().map(|row| (*row).to_owned()).collect()),
        ..Shown::default()
    }
    .after("gone")
}

/// `rows` between rows that name nothing, so that a screen withheld whole is told from one every
/// row of which a row's reach took out (the review of `a26e0848e`: a short screen could not
/// tell a verdict of "unknown" from a verdict of "prose", and plants that took the first for
/// the second were missed).
fn padded<'a>(rows: &[&'a str]) -> Vec<&'a str> {
    let mut screen = vec!["pad one", "pad two", "pad three"];
    screen.extend_from_slice(rows);
    screen.extend(["pad four", "pad five", "pad six"]);
    screen
}

/// Whether any six characters in a row of `value` are in `note`.
fn leaks(note: &str, value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    chars
        .windows(6.min(chars.len()))
        .any(|piece| note.contains(&piece.iter().collect::<String>()))
}

/// Screens whose secret is `value`, and the note of each must hold none of it.
fn assert_withheld_whole(screens: &[&[&str]], value: &str) {
    for screen in screens {
        let said = note(&padded(screen));
        assert_eq!(said, WHOLE, "{screen:?}");
        assert!(!leaks(&said, value), "{screen:?}");
    }
}

/// The review's first three screens, which put a password in the ledger.
#[test]
fn a_label_with_its_value_below_a_yaml_block_and_a_hard_wrapped_value_withhold_the_screen() {
    // `Password:` and its value two rows down, an instruction between.
    assert_withheld_whole(
        &[&["Password:", "Enter the value below", "red green blue"]],
        "red green blue",
    );
    // A folded scalar with a modifier, `>-`, whose third row `runs_on` never saw.
    assert_withheld_whole(
        &[&["PASSWORD: >-", "  correct horse", "  battery staple"]],
        "battery staple",
    );
    // A value of several words broken by hand over three rows.
    assert_withheld_whole(
        &[&["password: correct horse", "battery staple", "alpha bravo"]],
        "battery staple",
    );
}

/// The block scalars, with every modifier, and the other marks that open a value.
#[test]
fn every_block_scalar_and_every_opener_withholds_the_screen() {
    for opener in [
        "|",
        "|-",
        "|+",
        ">",
        ">-",
        ">+",
        ">2",
        "|2-",
        "|1",
        ">-  # a comment",
        "{",
        "[",
        "(",
        "\\",
        "<<EOF",
        "&anchor",
        "!!binary |",
        // A value of one word that ends in a backslash.
        "abc\\",
    ] {
        for name in ["password", "secret", "api_key", "token", "DB_PASSWORD"] {
            let row = format!("{name}: {opener}");
            // The rows that follow are indented, as a block's are, or are not: a screen is
            // not bound to show a block as YAML does.
            for following in [
                ["  correct horse", "  battery staple", "  alpha bravo"],
                ["correct horse", "battery staple", "alpha bravo"],
            ] {
                let mut rows = vec!["start", "ok", row.as_str()];
                rows.extend(following);
                rows.push("end");
                let said = note(&padded(&rows));
                assert_eq!(said, WHOLE, "{row} / {following:?}");
                assert!(!said.contains("battery"), "{row}");
            }
        }
    }
}

/// A label and its prompts, whatever ends them.
#[test]
fn a_row_that_is_a_label_or_ends_at_the_name_withholds_the_screen() {
    for label in [
        "Password:",
        "password =",
        "API_KEY=",
        "Enter password",
        "Enter your password:",
        "Invalid API key",
        "The password is",
        "Your new token is:",
        "secret ->",
        "token=>",
        "Bearer",
        // A row that ends on a mark that opens, and is not the name.
        "Using the secret flag \\",
        "see the token list [",
    ] {
        assert_withheld_whole(
            &[&["start", "ok", label, "", "hunter2hunter2 and more", "end"]],
            "hunter2hunter2",
        );
    }
}

/// A value wrapped over rows, quoted or not.
#[test]
fn a_value_that_goes_on_over_rows_withholds_the_screen() {
    assert_withheld_whole(
        &[
            &[
                "start",
                "ok",
                "API_KEY=\"abcdef",
                "ghijkl",
                "mnopqr\"",
                "end",
            ],
            &["start", "ok", "token: 'abc", "def", "ghi'", "end"],
            // A quote that closes on the row and a value the next row goes on with, deeper.
            &[
                "start",
                "ok",
                "password: hunter2",
                "  continued here",
                "end",
            ],
            // An unquoted value of several words.
            &[
                "start",
                "ok",
                "password: correct horse battery staple",
                "end",
            ],
            &["start", "ok", "Authorization: Bearer abcdefgh", "end"],
            // The rest of a row after a quote that closes.
            &["start", "ok", "password: \"abc\" def ghi", "end"],
            // A sentence that opens a quote on the row and does not close it.
            &[
                "start",
                "ok",
                "echo \"my password hunter2",
                "and more words",
                "end",
            ],
        ],
        "battery staple",
    );
}

/// A private key's block: whole, cut at its top, cut at its foot, and cut at both.
#[test]
fn a_private_keys_block_is_taken_out_whole_and_one_with_a_delimiter_missing_withholds_the_screen() {
    // Whole: out, with its neighbours, and the rest is quoted.
    let said = note(&[
        "building",
        "ok",
        "-----BEGIN OPENSSH PRIVATE KEY-----",
        "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ",
        "AAAAAAABAAAAMwAAAAtzc2gtZWQyNTUxOQAAACD",
        "-----END OPENSSH PRIVATE KEY-----",
        "ok",
        "finished",
    ]);
    assert_eq!(
        said,
        "gone (exit code 1); its screen ended with: \"finished\" (5 lines that named or lay beside a secret were left out)"
    );
    assert!(!said.contains("b3Blbn") && !said.contains("AAAAAAAB"));
    // A delimiter missing: the screen is withheld, and none of the body is told.
    for screen in [
        // Two beginnings and one end.
        &[
            "-----BEGIN RSA PRIVATE KEY-----",
            "ok",
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----END RSA PRIVATE KEY-----",
        ][..],
        // Its top is above the screen.
        &[
            "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ",
            "AAAAAAABAAAAMwAAAAtzc2gtZWQyNTUxOQAAACD",
            "-----END OPENSSH PRIVATE KEY-----",
            "done",
        ][..],
        // Its foot is below the screen.
        &[
            "ok",
            "-----BEGIN RSA PRIVATE KEY-----",
            "MIIEowIBAAKCAQEAxampleexampleexampleexample",
            "ZXhhbXBsZWV4YW1wbGVleGFtcGxlZXhhbXBsZQ",
        ][..],
        // No delimiter in view: the middle of a block, rows of base64 and nothing else.
        &[
            "ok",
            "MIIEowIBAAKCAQEAxampleexampleexampleexample",
            "ZXhhbXBsZWV4YW1wbGVleGFtcGxlZXhhbXBsZQ",
            "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=",
            "done",
        ][..],
    ] {
        let said = note(&padded(screen));
        assert_eq!(said, WHOLE, "{screen:?}");
        assert!(!said.contains("MIIEow") && !said.contains("ZXhhbX") && !said.contains("b3Blbn"));
    }
}

/// Values the extent of which is the row: quoted, `.env`, JSON, YAML. They are out with their
/// neighbours, and the rest of the screen is quoted through the masker.
#[test]
fn an_assignment_whose_value_is_on_its_row_leaves_out_itself_and_its_neighbours_only() {
    let screen = [
        "start",
        "ok",
        "DB_HOST=localhost",
        "DB_PASSWORD=hunter2hunter2",
        "DEBUG=true",
        "cargo test exited",
        "bye",
    ];
    let said = note(&screen);
    assert_eq!(
        said,
        "gone (exit code 1); its screen ended with: \"ok / cargo test exited / bye\" (3 lines that named or lay beside a secret were left out)"
    );
    assert!(!said.contains("hunter2"));
    for row in [
        r#""api_key": "abc123abc123","#,
        "export API_KEY='red green blue'",
        r#"DB_PASSWORD="alpha bravo charlie""#,
        "password: 'it''s a secret'",
        "x-api-key: abcdefghijklmno",
        "PassWord=\"alpha bravo charlie\"",
    ] {
        let said = note(&["start", "ok", row, "end of it", "bye", "fin"]);
        assert!(
            !said.contains("alpha") && !said.contains("abc123") && !said.contains("red green"),
            "{row}: {said}"
        );
        assert!(said.contains("\"start / bye / fin\""), "{row}: {said}");
    }
}

/// Prose that names a secret and says all it has to on its row, a test's name among it.
#[test]
fn prose_that_is_whole_on_its_row_leaves_out_the_row_and_two_each_way() {
    let said = note(&[
        "a",
        "b",
        "c",
        "test brain::auth::rejects_expired_token ... ok",
        "d",
        "e",
        "f",
        "g",
        "h",
    ]);
    assert_eq!(
        said,
        "gone (exit code 1); its screen ended with: \"f / g / h\" (3 lines that named or lay beside a secret were left out)"
    );
    // The screen of Pi with no login, and the errors of the others, say what failed.
    for words in [
        "No API key found for the selected model. Use /login to log into a provider.",
        "Invalid API key · Please run /login",
        "Authentication failed: the credentials were refused by the server",
    ] {
        let said = note(&["", words, ""]);
        assert_eq!(said, WHOLE, "{words}");
    }
}

/// Nothing named, nothing changed: a screen with no secret on it is quoted whole.
#[test]
fn a_screen_with_no_secret_on_it_is_quoted_as_it_always_was() {
    let said = note(&[
        "compiling",
        "error[E0308]: mismatched types",
        "  --> src/lib.rs:4",
    ]);
    assert_eq!(
        said,
        "gone (exit code 1); its screen ended with: \"compiling / error[E0308]: mismatched types / --> src/lib.rs:4\""
    );
}

/// Values with no name before them, on rows that name nothing: the masker's.
#[test]
fn a_value_with_no_name_before_it_is_masked_where_it_stands() {
    for (row, value) in [
        (
            concat!("token ", "sk-ant", "-api03-AbCdEfGhIjKlMnOp"),
            "AbCdEfGhIj",
        ),
        (
            concat!("got ", "ghp", "_abcdefghijklmnopqrstuvwxyz0123456789"),
            "abcdefghijklmnop",
        ),
        (
            concat!(
                "jwt ey",
                "JhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.sig-nature_1"
            ),
            "JhbGciOiJ",
        ),
        (
            "value YWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJj end",
            "YWJjYWJj",
        ),
        (
            "git clone https://alice:swordfish@github.test/x/y.git",
            "swordfish",
        ),
    ] {
        let said = note(&["start", row, "end"]);
        assert!(!leaks(&said, value), "{row}: {said}");
    }
}

/// The review of `a26e0848e` tried a screen for every way a secret can be shown, and its probe
/// failed nine of twenty-one. These are those ways, each a screen and the pieces of the secret
/// it holds: none of them may reach the note, and the delimiterless key's fragment is among
/// them. (The probe itself was not left on the board; the cases are written from its report.)
#[test]
fn the_twenty_four_screens_of_the_review_leak_nothing() {
    // Pieces of secrets are made in two (`concat!`) where their shape is a token's.
    let sk = concat!("sk-ant", "-api03-Zx9QpLmNoPqRsTuVwXyZaBcDeF");
    let ghp = concat!("ghp", "_abcdefghijklmnopqrstuvwxyz0123456789");
    let jwt = concat!(
        "ey",
        "JhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r"
    );
    let b64 = "YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXpBQkNERUZHSElKS0xN";
    let b64_no_digit = "YWJj".repeat(12);
    let overlap = "Zm9vYmFyMTIzNDU2Nzg5MGFiY2RlZg+/abcdefghijklmnopqr/AA==";
    let cases: Vec<(&str, Vec<String>, Vec<&str>)> = vec![
        (
            "sk- token",
            vec!["start".into(), format!("key was {sk}"), "end".into()],
            vec!["Zx9QpLmNoP"],
        ),
        (
            "ghp_ token",
            vec!["start".into(), format!("pushed with {ghp}"), "end".into()],
            vec!["abcdefghijklmn"],
        ),
        (
            "a JWT",
            vec!["start".into(), format!("session {jwt}"), "end".into()],
            vec!["JhbGciOiJ", "dBjftJeZ4"],
        ),
        (
            "base64 with digits",
            vec!["start".into(), format!("blob {b64}"), "end".into()],
            vec!["YWJjZGVm", "SElKS0xN"],
        ),
        (
            "base64 with no digit",
            vec!["start".into(), format!("blob {b64_no_digit}"), "end".into()],
            vec!["YWJjYWJj"],
        ),
        (
            "base64 overlapping a run",
            vec!["start".into(), format!("blob {overlap}"), "end".into()],
            vec!["abcdefghijklmnopqr", "obar123", "AA=="],
        ),
        (
            "Authorization header",
            vec![
                "start".into(),
                "Authorization: Bearer abcdefghijkl123456".into(),
                "end".into(),
            ],
            vec!["abcdefghijkl"],
        ),
        (
            "header split from its token",
            vec![
                "start".into(),
                "Authorization: Bearer".into(),
                "abcdefghijkl123456".into(),
                "end".into(),
            ],
            vec!["abcdefghijkl"],
        ),
        (
            "value wrapped over three rows",
            vec![
                "start".into(),
                "API_KEY=\"abcdef".into(),
                "ghijklmnop".into(),
                "qrstuv\"".into(),
                "end".into(),
            ],
            vec!["ghijklmnop", "qrstuv"],
        ),
        (
            "name and value two rows down",
            vec![
                "start".into(),
                "Password:".into(),
                "Enter the value below".into(),
                "red green blue".into(),
                "end".into(),
            ],
            vec!["red green"],
        ),
        (
            ".env dump",
            vec![
                "start".into(),
                "DB_HOST=localhost".into(),
                "DB_PASSWORD=hunter2hunter2".into(),
                "API_TOKEN=abcd1234abcd".into(),
                "end".into(),
            ],
            vec!["hunter2hunter2", "abcd1234"],
        ),
        (
            "curl -H",
            vec![
                "start".into(),
                "curl -H \"Authorization: Bearer abcdefghijkl123456\" https://x.test".into(),
                "end".into(),
            ],
            vec!["abcdefghijkl"],
        ),
        (
            "curl -u",
            vec![
                "start".into(),
                "$ curl -u alice:swordfish https://x.test/".into(),
                "end".into(),
            ],
            vec!["swordfish"],
        ),
        (
            "URL with credentials",
            vec![
                "start".into(),
                "git clone https://alice:swordfish@github.test/x/y.git".into(),
                "end".into(),
            ],
            vec!["swordfish"],
        ),
        (
            "JSON api_key",
            vec![
                "{".into(),
                "  \"api_key\": \"sekret-value-1234\",".into(),
                "  \"name\": \"x\"".into(),
                "}".into(),
            ],
            vec!["sekret-value"],
        ),
        (
            "YAML folded scalar",
            vec![
                "start".into(),
                "PASSWORD: >-".into(),
                "  correct horse".into(),
                "  battery staple".into(),
                "end".into(),
            ],
            vec!["battery staple"],
        ),
        (
            "hard-wrapped value",
            vec![
                "start".into(),
                "password: correct horse".into(),
                "battery staple".into(),
                "alpha bravo".into(),
                "end".into(),
            ],
            vec!["battery staple", "alpha bravo"],
        ),
        (
            "private key cut at its top",
            vec![
                "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ".into(),
                "AAAAAAABAAAAMwAAAAtzc2gtZWQyNTUxOQAAACD".into(),
                "-----END OPENSSH PRIVATE KEY-----".into(),
                "done".into(),
            ],
            vec!["b3BlbnNz", "AAAAAAABAAAA"],
        ),
        (
            "private key cut at its foot",
            vec![
                "ok".into(),
                "-----BEGIN RSA PRIVATE KEY-----".into(),
                "MIIEowIBAAKCAQEAxampleexampleexampleexample".into(),
                "ZXhhbXBsZWV4YW1wbGVleGFtcGxlZXhhbXBsZQ".into(),
            ],
            vec!["MIIEowIB", "ZXhhbXBs"],
        ),
        (
            "a key with no delimiter in view",
            vec![
                "ok".into(),
                "MIIEowIBAAKCAQEAxampleexampleexampleexample".into(),
                "ZXhhbXBsZWV4YW1wbGVleGFtcGxlZXhhbXBsZQ".into(),
                "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=".into(),
                "done".into(),
            ],
            vec!["MIIEowIB", "ZXhhbXBs", "QUJDREVG"],
        ),
        (
            "Pi without a login",
            vec![
                "Pi".into(),
                "No API key found for the selected model. Use /login to log into a provider."
                    .into(),
                "".into(),
            ],
            vec![],
        ),
        (
            "Claude's invalid key",
            vec![
                "Claude Code".into(),
                "Invalid API key · Please run /login".into(),
            ],
            vec![],
        ),
        (
            "Muse's credential error",
            vec![
                "muse".into(),
                "credential error: authentication failed for the account".into(),
                "".into(),
            ],
            vec![],
        ),
        (
            "a prompt with the value below",
            vec![
                "start".into(),
                "The password is".into(),
                "correct horse".into(),
                "battery staple".into(),
                "end".into(),
            ],
            vec!["correct horse", "battery staple"],
        ),
    ];
    assert!(cases.len() >= 21);
    for (label, rows, pieces) in &cases {
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let said = note(&rows);
        for piece in pieces {
            assert!(!said.contains(piece), "{label}: {piece} in {said}");
        }
    }
}

/// A token wrapped by hand over rows of one word each: the rows after the first are out as long
/// as they are one word.
#[test]
fn the_rows_of_a_token_wrapped_by_hand_are_left_out_as_long_as_they_are_one_word() {
    let said = note(&[
        "start",
        "ok",
        "API_KEY=abcdefgh",
        "ijklmnopqr",
        "stuvwxyzab",
        "cdefghijkl",
        "end",
        "fin",
    ]);
    for piece in ["ijklmn", "stuvwx", "cdefgh"] {
        assert!(!said.contains(piece), "{said}");
    }
    assert!(said.contains("\"end / fin\""), "{said}");
}
