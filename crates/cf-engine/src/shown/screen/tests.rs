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
        // Rows narrower than twelve, with what is left of the body on the last.
        (
            "a body of small letters in rows of eleven",
            vec![
                "abcdefghijk",
                "lmnopqrstuv",
                "wxyzabcdefg",
                "hijklmnopqr",
                "stuv",
            ],
        ),
        // Widths that differ, as a wrapped screen's rows do not (the groups of one length are
        // the groups rule's): three rows or more of eight, and the last what is left.
        (
            "a body of small letters in rows of ten, eleven, twelve and nine",
            vec![
                "abcdefghij",
                "klmnopqrstu",
                "vwxyzabcdefg",
                "hijklmnop",
                "qrs",
            ],
        ),
        // Two rows, the first thirty-five long and the second what is left.
        (
            "a body of small letters in two rows",
            vec!["abcdefghijklmnopqrstuvwxyzabcdefghi", "jklmn"],
        ),
        (
            "a body of small letters in rows of eight",
            vec!["abcdefgh", "ijklmnop", "qrstuvwx", "yzabcdef", "ghijklmn"],
        ),
        (
            "a body of small letters in rows of twenty and a short ending",
            vec!["abcdefghijklmnopqrst", "uvwxyzabcdefghijklmn", "opqr"],
        ),
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
        // A short body, below any size of its own, and the padding alone after it: the padding
        // row is the second row that looks encoded.
        ("a short body and its padding alone", vec!["0QAAAA", "=="]),
        // A key cropped to one row: a narrow body, and one with a symbol in it.
        ("one narrow row of a key's body", vec!["hkiG9w0BAQEFAASC"]),
        ("one narrow row with a plus", vec!["abcdefghij+klm"]),
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

// ---- The re-check of `4aeb48564` (T-1224): split, framed and grouped secrets, and the padding row.

/// Every kind of token the masker knows by its beginning, with a few characters of its body: the
/// screen is withheld whatever follows, and so with the body in groups or wrapped over rows.
#[test]
fn a_token_prefix_withholds_the_screen_whatever_follows_it() {
    // The re-check's: the masker took `sk-abcdefghijkl` and left the rest of the row.
    assert_withheld(
        &[
            &["sk-abcdefghijkl mnopqrstuvwx yz1234"],
            &["sk-abcd efgh ijkl mnop qrst uvwx yz12 3456"],
        ],
        NAMED,
        "mnopqrstuvwx",
    );
    // Each kind, short, in the middle of a row, framed.
    for token in [
        "sk-abcdefgh",
        "pk_live_abcdefgh",
        "rk_test_abcd",
        "ghp_abcdefgh",
        "gho_abcdefgh",
        "ghu_abcdefgh",
        "ghs_abcdefgh",
        "ghr_abcdefgh",
        "github_pat_abcdefgh",
        "glpat-abcdefgh",
        "xoxb-abcdefgh",
        "xoxp-abcdefgh",
        "AKIAABCDEFGH",
        "ASIAABCDEFGH",
        "AIzaabcdefgh",
        "eyJhbGciOiJIUzI1",
        "npm_abcdefghijkl",
        "hf_abcdefghijkl",
        "ya29.abcdefghijkl",
        "SG.abcdefgh",
    ] {
        for row in [
            token.to_owned(),
            format!("cloned with {token} and then"),
            format!("│ {token} │"),
        ] {
            let said = note(&[row.as_str()]);
            assert_eq!(said, NAMED, "{row}");
        }
    }
    // The prefix at the end of a row, its body on the next: whole at the join.
    for rows in [
        vec!["gone to the disk", "sk-abcdefgh"],
        vec!["the key is sk-", "abcdefgh"],
        vec!["token ghp_", "abcdefgh", "next"],
        vec!["s", "k-abcdefgh"],
        vec!["see AKIA", "ABCDEFGH"],
        vec!["Basic", "YTpi"],
    ] {
        assert_eq!(note(&rows), NAMED, "{rows:?}");
    }
    // `Basic` with the base64 of `a:b`; a word after it is no credential.
    assert_eq!(note(&["Basic YTpi"]), NAMED);
    assert!(note(&["Basic setup"]).contains("its screen ended with"));
    // Names that begin like a kind, and are not one.
    for row in [
        "npm_config_cache=/tmp/npm",
        "hf_hub_download(repo)",
        "SG.Assert(x)",
        "a task-based disk-usage check",
        "ASIAN markets",
        "pk_id of the users table is skipped",
        "constraint pk_users_id failed",
        "Basic HTTP server",
        "Basic JSON output",
    ] {
        let said = note(&[row]);
        assert!(said.contains("its screen ended with"), "{row}: {said}");
    }
}

/// A name the window's wrap cut in two is whole at the join of the rows, as is a framed one.
#[test]
fn a_name_the_window_split_over_rows_is_found_at_the_join() {
    // The re-check's: each row alone names nothing, and the value was quoted.
    assert_withheld(
        &[
            &["{\"api_", "key\": \"red", "green blue\"}"],
            &["{\"api_", "key\": \"red", "green blue\"}", "}"],
        ],
        NAMED,
        "green blue",
    );
    for (rows, value) in [
        (vec!["pass", "word: correct horse"], "correct horse"),
        (vec!["export MYSQL_PW", "D=hunter22 now"], "hunter22"),
        (
            vec!["Author", "ization: Bear", "er abcd1234 gone"],
            "abcd1234",
        ),
        (vec!["the private", "key is below"], "below"),
        (vec!["an API", "key set to blue-sky-one"], "blue-sky-one"),
        (vec!["a credent", "ial of red green blue"], "red green blue"),
        (
            vec!["│ {\"api_ │", "│ key\": \"red │", "│ green blue\"} │"],
            "green blue",
        ),
        (
            vec!["{\"api_", "", "key\": \"red", "green blue\"}"],
            "green blue",
        ),
    ] {
        assert_withheld(&[&rows], NAMED, value);
    }
    // The join of the rows makes a word of two, and a row alone is a word still.
    assert_eq!(note(&["auth", "or", "next"]), NAMED);
    // A screen whose rows only end and begin like words is quoted.
    for rows in [
        vec!["the cargo test", "ran in a minute"],
        vec!["see the author", "list for details"],
        vec!["the sec", "ond run"],
    ] {
        assert!(
            note(&rows).contains("its screen ended with"),
            "{rows:?}: {}",
            note(&rows)
        );
    }
}

/// What a program draws round its output is no part of a row: the bars and the box edges at its
/// ends, and the blanks beside them.
#[test]
fn the_frame_a_program_draws_round_its_rows_is_no_part_of_them() {
    // The re-check's: the two-row key crop, each row between bars.
    for (left, right) in [
        ("│", "│"),
        ("┃", "┃"),
        ("║", "║"),
        ("|", "|"),
        ("│ ", " │"),
        ("| ", "   |"),
        ("│", ""),
        ("", "│"),
    ] {
        let rows = [
            format!("{left} MIIEvQIBADANBgkq {right}"),
            format!("{left} hkiG9w0BAQEFAASC {right}"),
        ];
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        assert_eq!(note(&rows), KEY, "{rows:?}");
    }
    // In a box, with its corners and rules.
    assert_eq!(
        note(&[
            "┌──────────────────┐",
            "│ MIIEvQIBADANBgkq │",
            "│ hkiG9w0BAQEFAASC │",
            "└──────────────────┘",
        ]),
        KEY
    );
    assert_eq!(
        note(&[
            "╭──────────────────╮",
            "║ YQ==             ║",
            "║ Yg==             ║",
            "╰──────────────────╯",
        ]),
        KEY
    );
    // A row between a box's own edges.
    assert_eq!(
        note(&["├─ MIIEvQIBADANBgkq ─┤", "├─ hkiG9w0BAQEFAASC ─┤"]),
        KEY
    );
    // A short row between bars, and the padding.
    assert_eq!(note(&["│ abcdefghijklmnopqrstuvw │", "│ = │"]), KEY);
    // A framed screen with nothing in it is quoted, its frame and all.
    for rows in [
        vec!["┌────┐", "│ ok │", "└────┘"],
        vec!["| src/lib.rs | 3 +++ |", "| 1 file changed |"],
        vec!["╭───────╮", "│ ready │", "╰───────╯"],
    ] {
        let said = note(&rows);
        assert!(said.contains("its screen ended with"), "{rows:?}: {said}");
    }
}

/// A value in groups, parted by one blank or one `-`, whatever the groups hold.
#[test]
fn a_value_in_groups_withholds_the_screen() {
    for (label, rows) in [
        (
            "the re-check's: base64 groups parted by dashes",
            vec!["YWJj-ZGVm-ZGhp-amts-bWpv-cHFy-cHRu-dndv"],
        ),
        (
            "the re-check's: hex groups parted by blanks",
            vec!["deadbeef cafebabe feedface abcdefab deadbeef cafebabe feedface abcdefab"],
        ),
        (
            "the re-check's: letters in groups of four",
            vec!["abcd efgh ijkl mnop qrst uvwx yz12 3456"],
        ),
        ("four groups, the fewest", vec!["abcd efgh ijkl mnop"]),
        ("digits", vec!["1234-5678-9012-3456"]),
        ("underscores", vec!["abcd_efgh_ijkl_mnop"]),
        ("colons", vec!["abcd:efgh:ijkl:mnop"]),
        ("dots", vec!["abcd.efgh.ijkl.mnop"]),
        ("slashes in the groups", vec!["ab/c d/ef g/hi j/kl m/no"]),
        ("hex words", vec!["dead beef cafe babe"]),
        (
            "hex groups of differing lengths",
            vec!["deadbeef cafe feedface abcdef"],
        ),
        (
            "groups with a plus, of differing lengths",
            vec!["ab+cd e+fghi jk+lmn o+pqr"],
        ),
        (
            "groups of differing lengths that each have letters and digits",
            vec!["a1b2c3 d4e5f6g7 h8i9 j0k1l2"],
        ),
        (
            "groups of differing lengths that each have a capital after a small letter",
            vec!["aBcDe fGhIjKl mNoPq rStUvWxY"],
        ),
        (
            "the groups in the middle of a row",
            vec!["key part YWJj ZGVm ZGhp amts end"],
        ),
        ("groups wrapped over rows", vec!["abcd efgh", "ijkl mnop"]),
        ("a group to a row", vec!["abcd", "efgh", "ijkl", "mnop"]),
        (
            "groups in a frame",
            vec!["│ YWJj-ZGVm-ZGhp-amts │", "│ bWpv-cHFy-cHRu-dndv │"],
        ),
        (
            "the groups of a full IPv6 address",
            vec!["2001:0db8:85a3:0000:0000:8a2e:0370:7334"],
        ),
    ] {
        assert_eq!(note(&rows), KEY, "{label}: {rows:?}");
    }
    // Not a run: three groups, two blanks, a word of three letters between, groups of
    // differing lengths that are words, and the lists a screen shows.
    for rows in [
        vec!["abcd efgh ijkl"],
        vec!["abcd efgh ijkl and more"],
        vec!["abcd  efgh  ijkl  mnop"],
        vec!["abcd efgh ijk lmno pqrs"],
        vec!["the and for are not"],
        vec!["Compiling consensflow engine library for target platform"],
        vec!["the quick brown foxes jumped over lazy dogs"],
        vec!["PATH=/usr/bin HOME=/root LANG=C TERM=xterm"],
        vec!["src/foo tests/bar docs/baz crates/qux"],
        vec!["twelve that looks encoded"],
        vec!["   Compiling cf-engine v3.0.7-alpha.2 (/Users/me/consensflow/crates/cf-engine)"],
    ] {
        let said = note(&rows);
        assert!(said.contains("its screen ended with"), "{rows:?}: {said}");
    }
    // The cost, stated: four words of one length, and a list of camel-case names, are a run.
    for rows in [
        vec!["this will work like that"],
        vec!["ParseFailure LexerMismatch SyntaxError TypeMismatch"],
    ] {
        assert_eq!(note(&rows), KEY, "{rows:?}");
    }
}

/// A row of one `=` or two is a key's last row, whatever stands beside it and whatever the row
/// before it is: a body of small letters has no shape of its own.
#[test]
fn a_lone_padding_row_withholds_the_screen_whatever_stands_beside_it() {
    // The re-check's: the concatenation is base64, and the note printed the 23-character body.
    for (label, rows) in [
        ("the re-check's", vec!["abcdefghijklmnopqrstuvw", "="]),
        ("two signs", vec!["abcdefghijklmnopqrstuvw", "=="]),
        ("a short body", vec!["abc", "="]),
        ("padding first", vec!["=", "some words"]),
        ("padding last", vec!["still working", "=="]),
        ("padding alone", vec!["="]),
        ("in a frame", vec!["│ abcdefghijklmnopqrstuvw │", "│ == │"]),
    ] {
        assert_eq!(note(&rows), KEY, "{label}: {rows:?}");
        let between = padded(&rows);
        assert_eq!(note(&between), KEY, "{label}: {between:?}");
    }
    // A longer row of `=` is a rule, which is chrome, as is a framed one.
    for rows in [
        vec!["Summary", "=======", "ok"],
        vec!["Summary", "===", "ok"],
        vec!["╞═══════╡", "ok"],
        vec!["│ ═══ │", "ok"],
    ] {
        let said = note(&rows);
        assert!(said.contains("its screen ended with"), "{rows:?}: {said}");
    }
}

/// The screens of an ordinary day: which are withheld, and so which are not. The re-check
/// (T-1224) tried eleven and four were withheld: a Rust syntax error saying "unexpected token",
/// a bare full hash, a bare identifier and an encoded-looking row. The same four are withheld
/// here, and no other of these.
#[test]
fn the_ordinary_screens_stay_visible_and_the_few_that_are_withheld_are_counted() {
    // (what it is, its rows, whether the note withholds it)
    let cases: Vec<(&str, Vec<&str>, bool)> = vec![
        (
            "cargo build",
            vec![
                "   Compiling cf-proto v3.0.7-alpha.2 (/Users/me/consensflow/crates/cf-proto)",
                "   Compiling cf-engine v3.0.7-alpha.2 (/Users/me/consensflow/crates/cf-engine)",
                "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 41.20s",
                "     Running unittests src/lib.rs (target/debug/deps/cf_engine-1a2b3c4d5e6f7a8b)",
            ],
            false,
        ),
        (
            "cargo test",
            vec![
                "running 238 tests",
                "test shown::tests::only_the_last_lines_are_quoted ... ok",
                "test shown::screen::tests::a_value_in_groups_withholds_the_screen ... ok",
                "test result: ok. 238 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.18s",
            ],
            false,
        ),
        (
            "a Rust type error with a long type path",
            vec![
                "error[E0308]: mismatched types",
                "   --> crates/cf-engine/src/shown.rs:258:20",
                "    |",
                "258 |         let rows: Vec<&str> = tail.iter().map(undecorated).collect::<Vec<String>>();",
                "    |                 ----------   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Vec<&str>`, found `Vec<String>`",
                "    = note: expected struct `Vec<&str>`",
                "               found struct `Vec<std::string::String>`",
            ],
            false,
        ),
        (
            "a type path alone",
            vec!["std::collections::hash::map::HashMap<alloc::string::String, alloc::vec::Vec<core::option::Option<u8>>>"],
            false,
        ),
        (
            "git log",
            vec![
                "commit 4aeb48564ce31f17a2e92e5c39e4fbe15a51bb37",
                "Author: Gabriel Voicu <gabriel@example.test>",
                "Date:   Wed Nov 4 08:00:00 2026 +0200",
                "",
                "    A short body and the padding alone after it is the case that decides the lone padding row",
                "",
                "commit 3eb253eeb0123456789abcdef0123456789abcde",
                "Author: Gabriel Voicu <gabriel@example.test>",
            ],
            false,
        ),
        (
            "git log --oneline",
            vec![
                "4aeb48564 A short body and the padding alone after it is the case that decides",
                "3eb253eeb A key cropped to one row withholds the screen too: a lone row of twelve that looks encoded",
                "187f81d89 A sentence with a digit in it is not a row of base64",
            ],
            false,
        ),
        (
            "a connection error",
            vec![
                "error: failed to connect to 127.0.0.1:8080: Connection refused (os error 61)",
                "caused by: tcp connect error",
            ],
            false,
        ),
        (
            "a database connection error",
            vec!["Error: connect ECONNREFUSED 127.0.0.1:5432"],
            false,
        ),
        (
            "a panic",
            vec![
                "thread 'main' panicked at src/main.rs:12:5:",
                "called `Option::unwrap()` on a `None` value",
                "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
            ],
            false,
        ),
        (
            "a Python traceback",
            vec![
                "Traceback (most recent call last):",
                "  File \"app.py\", line 12, in <module>",
                "    main()",
                "ValueError: invalid literal for int() with base 10: 'abc'",
            ],
            false,
        ),
        (
            "an npm install",
            vec![
                "added 312 packages, and audited 313 packages in 9s",
                "found 0 vulnerabilities",
            ],
            false,
        ),
        (
            "ls -la",
            vec![
                "total 24",
                "-rw-r--r--  1 me  staff  1234 Nov  4 08:00 shown.rs",
                "drwxr-xr-x  5 me  staff   160 Nov  4 08:00 tests",
            ],
            false,
        ),
        (
            "a file diff stat in a frame",
            vec![
                "┌─────────────────────────┬─────┐",
                "│ crates/cf-engine/src/shown.rs │ 89 │",
                "└─────────────────────────┴─────┘",
            ],
            false,
        ),
        (
            "Pi's model not available",
            vec![
                "Model gpt-9 is not available on your plan.",
                "Run /model to choose another one.",
            ],
            false,
        ),
        (
            "a window that printed nothing but a prompt",
            vec!["$ "],
            false,
        ),
        // The withheld.
        (
            "a Rust syntax error saying unexpected token",
            vec![
                "error: unexpected token: `;`",
                " --> src/main.rs:3:14",
            ],
            true,
        ),
        (
            "a bare full hash",
            vec!["4aeb48564ce31f17a2e92e5c39e4fbe15a51bb37"],
            true,
        ),
        ("a bare identifier", vec!["ParseFailure"], true),
        ("an encoded-looking row", vec!["hkiG9w0BAQEFAASC"], true),
    ];
    let mut withheld = 0;
    for (label, rows, expected) in &cases {
        let said = note(rows);
        let was = said.contains("so they are left out");
        assert_eq!(was, *expected, "{label}: {said}");
        withheld += usize::from(was);
    }
    assert_eq!((withheld, cases.len()), (4, 19));
}

/// Text in percent signs says nothing to the masker (no `://`, no `@`) nor to a test of words:
/// what it spells is looked at too (the re-check's boundary, and the obvious next shape of its
/// credential URL).
#[test]
fn text_in_percent_signs_is_looked_at_as_what_it_spells() {
    assert_withheld(
        &[
            &["https%3A%2F%2Falice%3Aswordfish%40example.test"],
            &["https%3a%2f%2falice%3aswordfish%40example.test"],
            &["https%3A%2F%2Falice%3Aswo", "rdfish%40example.test"],
            &["GET /cb?u=https%3A%2F%2Falice%3Aswordfish%40example.test HTTP/1.1"],
        ],
        NAMED,
        "swordfish",
    );
    for (rows, value) in [
        (vec!["api%5Fkey%3Dred%20green%20blue"], "green blue"),
        (vec!["password%3Dhunter22"], "hunter22"),
        (vec!["sk%2Dabcdefgh"], "abcdefgh"),
        (vec!["x=%2D%2Dtoken%3Dabc123"], "abc123"),
    ] {
        assert_withheld(&[&rows], NAMED, value);
    }
    // Percent signs that spell nothing secret, and ones that spell nothing at all.
    for rows in [
        vec!["file:///My%20Folder/a%2Fb.txt"],
        vec!["GET /a%20b?q=100%25 HTTP/1.1"],
        vec!["caf%C3%A9 au lait"],
        vec!["50% done", "100%", "%zz and %4"],
    ] {
        let said = note(&rows);
        assert!(said.contains("its screen ended with"), "{rows:?}: {said}");
    }
}
