//! A screen that names a secret or holds what may be a key, put in a note (T-823, the reviews
//! of `a26e0848e` and `860010398`): for every shape of a value the reviews tried or the chief
//! named, no part of it reaches the note, because the whole screen is withheld, and a screen
//! that holds neither is quoted as it always was.

use crate::shown::Shown;

/// What a screen withheld for a secret it names is told in.
const NAMED: &str =
    "gone (exit code 1); its last screen lines named a secret, so they are left out";

/// What a screen withheld for rows that may be a key is told in.
const KEY: &str =
    "gone (exit code 1); its last screen lines held what may be a key, so they are left out";

/// What the note says of a window that ended with `screen` on it.
fn note(screen: &[&str]) -> String {
    Shown {
        code: Some(1),
        tail: Some(screen.iter().map(|row| (*row).to_owned()).collect()),
        ..Shown::default()
    }
    .after("gone")
}

/// `rows` between rows that name nothing, so that a screen withheld whole is told from one
/// that is quoted around them.
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

/// Screens whose secret is `value`: the note of each is the sentence, whole, and holds none of it.
fn assert_withheld(screens: &[&[&str]], sentence: &str, value: &str) {
    for screen in screens {
        for rows in [screen.to_vec(), padded(screen)] {
            let said = note(&rows);
            assert_eq!(said, sentence, "{rows:?}");
            assert!(!leaks(&said, value), "{rows:?}");
        }
    }
}

/// The screens the review of `860010398` put a password in the ledger with: a sentence that
/// trails off, an assignment of one word wrapped by hand, and a token wrapped over rows of one
/// short word each.
#[test]
fn the_screens_the_second_review_put_in_the_ledger_withhold_the_whole_screen() {
    assert_withheld(
        &[
            &[
                "The password consists of these words",
                "correct",
                "horse",
                "battery",
                "staple",
                "connection failed",
            ],
            &[
                "PASSWORD=correct",
                "horse",
                "battery staple",
                "connection failed",
            ],
            &[
                "API_KEY=sk-abcdefghijkl",
                "mnopqrstuvwx",
                "xyz",
                "connection failed",
            ],
        ],
        NAMED,
        "battery staple xyz mnopqrstuvwx",
    );
}

/// The screens the first review put a password in the ledger with: a label with its value
/// below, a YAML block scalar with a modifier, a value hard-wrapped over rows.
#[test]
fn the_screens_the_first_review_put_in_the_ledger_withhold_the_whole_screen() {
    assert_withheld(
        &[
            &["Password:", "Enter the value below", "red green blue"],
            &["PASSWORD: >-", "  correct horse", "  battery staple"],
            &["password: correct horse", "battery staple", "alpha bravo"],
        ],
        NAMED,
        "red green blue battery staple alpha bravo",
    );
}

/// Every shape of a value, whatever its extent: the screen is withheld, so none of it is told.
#[test]
fn a_row_that_names_a_secret_withholds_the_screen_whatever_follows_it() {
    for opener in [
        "|", "|-", "|+", ">", ">-", ">+", ">2", "{", "[", "(", "\\", "<<EOF", "&anchor", "abc\\",
    ] {
        for name in ["password", "secret", "api_key", "token", "DB_PASSWORD"] {
            let row = format!("{name}: {opener}");
            for following in [
                ["  correct horse", "  battery staple", "  alpha bravo"],
                ["correct horse", "battery staple", "alpha bravo"],
            ] {
                let mut rows = vec![row.as_str()];
                rows.extend(following);
                assert_withheld(&[&rows], NAMED, "battery staple");
            }
        }
    }
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
        "Using the secret flag \\",
        "see the token list [",
        "No API key found for the selected model. Use /login to log into a provider.",
        "export DB_PASSWORD=hunter2hunter2",
        "API_KEY=\"abcdef",
        "token: 'abc",
        "password: hunter2",
        "password: correct horse battery staple",
        "Authorization: Bearer abcdefgh",
        "echo \"my password hunter2",
        "password: \"abc\" def ghi",
        "\"api_key\": \"abc123abc123\",",
        "-----BEGIN RSA PRIVATE KEY-----",
        "-----END PGP PRIVATE KEY BLOCK-----",
    ] {
        assert_withheld(
            &[&["start", label, "hunter2hunter2 and more", "end"]],
            NAMED,
            "hunter2hunter2",
        );
    }
    // The beginning of a block with no end in view, an end with no beginning, two beginnings.
    assert_withheld(
        &[
            &[
                "ok",
                "-----BEGIN PGP PRIVATE KEY BLOCK-----",
                "bGluZSBvbmU=",
                "bGluZSB0d28=",
            ],
            &["x", "y", "-----END RSA PRIVATE KEY-----", "done"],
            &[
                "-----BEGIN PGP PRIVATE KEY BLOCK-----",
                "ok",
                "-----BEGIN PGP PRIVATE KEY BLOCK-----",
                "-----END PGP PRIVATE KEY BLOCK-----",
            ],
        ],
        NAMED,
        "bGluZSBvbmU= bGluZSB0d28=",
    );
}

/// A key with nothing to say so: no delimiter, no name, rows as narrow as the window was.
#[test]
fn rows_of_base64_and_nothing_else_withhold_the_screen() {
    // The review's: two short endings, a crop of a PEM body in rows of sixteen, a run of
    // forty-eight small letters.
    let lowercase = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuv";
    assert_eq!(lowercase.len(), 48);
    let hash = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
    let cases: Vec<(&str, Vec<&str>)> = vec![
        ("two short endings", vec!["YQ==", "Yg=="]),
        (
            "a crop of a PEM body",
            vec!["MIIEvQIBADANBgkq", "hkiG9w0BAQEFAASC"],
        ),
        ("a run of small letters", vec![lowercase]),
        (
            "a run of small letters in narrow rows",
            vec!["abcdefghijklmnop", "qrstuvwxyzabcdef", "ghijklmnopqrstuv"],
        ),
        ("a hash", vec![hash]),
        (
            "a body, a short ending",
            vec!["ZXhhbXBsZWV4YW1wbGVleGFtcGxlZXhh", "bXBsZQ=="],
        ),
        (
            "a body of slashes and pluses",
            vec!["a+b/c+d/e+f/g+h/", "i+j/k+l/m+n/o+p/"],
        ),
        (
            "a body with digits",
            vec!["hkiG9w0BAQEFAASC", "MIIEvQIBADANBgkq", "AQ"],
        ),
        (
            "the padding alone after a body",
            vec!["MIIEvQIBADANBgkq", "=="],
        ),
        // Each reason by itself: digits among small letters, a capital after a small letter.
        (
            "letters and digits, no capital",
            vec!["abc123def456", "ghi789jkl012"],
        ),
        (
            "capitals after small letters, no digit",
            vec!["abcDEFghiJKLmno", "pqrSTUvwxYZAbcd"],
        ),
    ];
    for (label, rows) in &cases {
        for screen in [rows.clone(), padded(rows)] {
            let said = note(&screen);
            assert_eq!(said, KEY, "{label}: {screen:?}");
            for row in rows.iter().filter(|row| row.len() >= 8) {
                assert!(!leaks(&said, row), "{label}");
            }
        }
    }
}

/// What is not a key is quoted as it always was: words, paths, a single short row, numbers.
#[test]
fn a_screen_with_no_secret_and_no_key_on_it_is_quoted_as_it_always_was() {
    for screen in [
        vec![
            "compiling",
            "error[E0308]: mismatched types",
            "  --> src/lib.rs:4",
        ],
        vec!["running 3 tests", "ok", "done"],
        vec!["Compiling", "Finished", "Running"],
        vec!["YQ==", "next row says words"],
        vec!["ok", "YQ=="],
        vec!["/Users/gabrielvoicu/Projects/ngvoicu/consensflow-trees/failure-why"],
        vec!["12", "345", "6789"],
        vec!["connection", "refused"],
        vec!["src/lib.rs", "src/main.rs"],
        // A rule of equals signs across the screen is chrome, not a key's padding.
        vec![
            "========================================================================",
            "Summary",
            "ok",
        ],
        // Sentences with a digit in each are not rows of base64: they have blanks in them.
        vec![
            "compiled 3 crates in 4 seconds",
            "finished 2 tests in 1 second",
        ],
        // Two encoded-looking rows with prose between them are two rows, not a run.
        vec!["YQ==", "some words here", "Yg=="],
    ] {
        let said = note(&screen);
        assert!(said.contains("its screen ended with"), "{screen:?}: {said}");
        assert!(!said.contains("left out"), "{screen:?}: {said}");
    }
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
            "git clone https://alice:swordfish@github.test/x/y.git",
            "swordfish",
        ),
        ("$ curl -u alice:swordfish https://x.test/", "swordfish"),
    ] {
        let said = note(&["start", row, "end"]);
        assert!(!leaks(&said, value), "{row}: {said}");
    }
}

/// The review of `a26e0848e` tried a screen for every way a secret can be shown, and its probe
/// failed nine of twenty-one; the re-check failed one of them and twelve more. These are those
/// ways and the ones the chief named, each a screen and the pieces of the secret it holds: none
/// of them may reach the note. (The probes were not left on the board; the cases are written from
/// their reports.)
#[test]
fn the_twenty_four_screens_of_the_reviews_leak_nothing() {
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
            "a key in rows of sixteen",
            vec![
                "ok".into(),
                "MIIEvQIBADANBgkq".into(),
                "hkiG9w0BAQEFAASC".into(),
                "done".into(),
            ],
            vec!["MIIEvQIB", "hkiG9w0B"],
        ),
        (
            "a key's short endings",
            vec!["ok".into(), "YQ==".into(), "Yg==".into(), "done".into()],
            vec!["YQ==", "Yg=="],
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
        (
            "a sentence that trails off",
            vec![
                "The password consists of these words".into(),
                "correct".into(),
                "horse".into(),
                "battery".into(),
                "staple".into(),
                "connection failed".into(),
            ],
            vec!["battery", "staple"],
        ),
        (
            "an assignment wrapped by hand",
            vec![
                "PASSWORD=correct".into(),
                "horse".into(),
                "battery staple".into(),
                "connection failed".into(),
            ],
            vec!["battery staple", "horse"],
        ),
        (
            "a token wrapped in short rows",
            vec![
                "API_KEY=sk-abcdefghijkl".into(),
                "mnopqrstuvwx".into(),
                "xyz".into(),
                "connection failed".into(),
            ],
            vec!["mnopqrstuvwx"],
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
    ];
    assert!(cases.len() >= 24);
    for (label, rows, pieces) in &cases {
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let said = note(&rows);
        for piece in pieces {
            assert!(!said.contains(piece), "{label}: {piece} in {said}");
        }
    }
}
