//! `quotable`: the words of a program are screened whole and cut after; a reason with no words
//! of a program in it is only masked.

use super::*;

/// What a read of Pi that ended says before the words of Pi.
const ENDED: &str = "pi --mode rpc ended before it answered (exit 1)";

/// A reason of Pi's read, with `said` after the mark.
fn pi_said(said: &str) -> String {
    format!("{ENDED}{IT_SAID}{said}")
}

/// What `quotable` keeps of Pi's words when they are withheld: the description, the code and the
/// sentence.
fn withheld() -> String {
    pi_said(WORDS_WITHHELD)
}

#[test]
fn a_key_with_blanks_in_it_withholds_the_words_of_pi_whole() {
    let said = quotable(&pi_said(
        "extension failed: API_KEY: sk-abcd efghijkl mnopqrst",
    ));
    assert_eq!(said, withheld());
    for piece in [
        "sk-abcd",
        "efghijkl",
        "mnopqrst",
        "API_KEY",
        "extension failed",
    ] {
        assert!(!said.contains(piece), "{piece} in {said}");
    }
}

/// The third reading's probe: the first line was withheld and both continuations were kept.
#[test]
fn a_value_that_runs_over_lines_withholds_the_lines_after_it_too() {
    let said = quotable(&pi_said(
        "Error: extension failed\nAPI_KEY: sk-abcd\nefghijkl\nmnopqrst",
    ));
    assert_eq!(said, withheld());
}

/// The third reading's probe: five lines that begin `password: correct`; a cut to the last four
/// before the look took the label and left the password.
#[test]
fn five_lines_that_begin_with_a_password_are_screened_before_the_cut() {
    let said = quotable(&pi_said(
        "password: correct\nhorse\nbattery\nstaple\nprovider unavailable",
    ));
    assert_eq!(said, withheld());
}

#[test]
fn a_label_far_above_the_last_lines_withholds_them_all() {
    let lines: Vec<String> = std::iter::once("token: abc12345".to_owned())
        .chain((1..=20).map(|n| format!("line {n}")))
        .collect();
    assert_eq!(quotable(&pi_said(&lines.join("\n"))), withheld());
}

#[test]
fn a_pretty_printed_error_object_holding_an_api_key_is_withheld() {
    let object = "{\n  \"error\": {\n    \"message\": \"provider failed\",\n    \"apiKey\": \"sk-abcd efghijkl\",\n    \"retries\": 3\n  }\n}";
    assert_eq!(quotable(&pi_said(object)), withheld());
}

/// A name that says its value is a password, a secret or a credential withholds the block
/// whatever the value is, for the masker leaves a short one and a password of short words; a
/// key's or a token's name, whatever it is joined to, withholds it when the value is one the
/// masker takes for a key. Both in any case, on the line or the next.
#[test]
fn a_credential_word_with_a_value_after_it_withholds_the_block_in_any_case() {
    for words in [
        "password: x y",
        "SECRET = 'a b'",
        "\"secret\": \"two words\"",
        "the line before\nPassword=hunter two\nthe line after",
        "DB_PWD=red green blue",
        "PassWord=\"alpha bravo charlie\"",
        "passphrase: a b",
        "credentials = x",
        "Password:\nthe value on the next line",
        "key: abcdefgh1 ijklmnop",
        "KEY=abcdefgh12",
        "ssh_key = abcdefghijklmnop",
        "x-api-key: 0123456789",
        "token: abcdefgh12 b c",
        "Token=abcdef0123",
        "Authorization: Basic YWxhZGRpbjpvcGVuc2VzYW1l",
        "authorization bearer zzzzyyyy1234",
        "Bearer abcdefgh1234 def",
        "bearer: abcd1234efgh",
        "key:\nabcdefgh12",
    ] {
        assert_eq!(quotable(&pi_said(words)), withheld(), "{words}");
    }
}

/// `key` and `token` are words too (the masker's own doc: `token: expired`). A short value after
/// one is a message, as it is in `Invalid key: expected string` and `Unexpected token: }`, and no
/// key the masker could not hide in a note is one.
#[test]
fn a_key_or_a_token_with_a_value_no_key_is_as_short_as_is_a_message_and_is_quoted() {
    for words in [
        "key: abcdefgh ijklmnop",
        "KEY=abc",
        "token: a b c",
        "x-api-key: 1",
        "Bearer abc def",
        "key:\nthe value on the next line",
    ] {
        assert_eq!(quotable(&pi_said(words)), pi_said(words), "{words}");
    }
}

#[test]
fn the_screens_rule_holds_for_words_a_credential_is_in_with_no_word_to_say_so() {
    for words in [
        "request with sk-abcd was refused",
        "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB",
        "deadbeef cafebabe feedface 0badf00d",
        "see https%3A%2F%2Falice%3Aswordfish%40example.test",
        "MIIEvQIBADANBgkq\nhkiG9w0BAQEFAASC",
        // A name the wrap cut in two is whole only at the join of the lines: neither row names a
        // secret alone.
        "request with sk\n-abcdefgh1234 was refused",
        "cannot read pass\nword: hunter two from disk",
        "the access tok\nen: abcdefgh1234 was refused",
    ] {
        assert_eq!(quotable(&pi_said(words)), withheld(), "{words}");
    }
    // The word alone, cut in two or not, is no credential.
    for words in [
        "cannot open pass\nword for storage",
        "the access tok\nen was refused",
    ] {
        assert_eq!(quotable(&pi_said(words)), pi_said(words), "{words}");
    }
}

/// What the user is told when Pi's words hold no credential: the description, the code, and the
/// last lines of what it said, one to a line.
#[test]
fn the_words_of_pi_that_hold_no_credential_are_quoted_as_they_were() {
    let said = "Error: Failed to load extension \"/x/e.ts\": Cannot find module 'left-pad'\nHint: Start without extensions using \"pi -ne\".";
    assert_eq!(quotable(&pi_said(said)), pi_said(said));
    for words in [
        "key not found",
        "the key",
        "keyboard: us",
        "Pi lists no model: no provider is set up",
    ] {
        assert_eq!(quotable(&pi_said(words)), pi_said(words), "{words}");
    }
}

#[test]
fn only_the_last_lines_that_passed_are_quoted_and_a_stack_is_never_quoted() {
    let said: String = (1..=9).map(|n| format!("line {n}\n")).collect();
    assert_eq!(
        quotable(&pi_said(&said)),
        pi_said("line 6\nline 7\nline 8\nline 9")
    );
    let with_frames = "Error: boom\n    at one (a.js:1:1)\n    at two (b.js:2:2)\n";
    assert_eq!(quotable(&pi_said(with_frames)), pi_said("Error: boom"));
    // Nothing but frames and blanks: nothing of Pi's words is said, nor the mark.
    assert_eq!(quotable(&pi_said("    at one (a.js:1:1)\n\n  \n")), ENDED);
    let wide = "é".repeat(2_000);
    let cut = quotable(&pi_said(&wide));
    let words = cut.strip_prefix(&format!("{ENDED}{IT_SAID}")).unwrap();
    assert_eq!(words.chars().count(), QUOTE_CHARS + 1);
    assert!(words.ends_with('…'));
}

/// A frame is never quoted, but it is looked at with the rest (T-1276,
/// `frame-label-continuation`, below): a frame whose path merely names a credential word, as
/// `token-lib/auth` does, withholds nothing (A4-08: a word is no credential), and is set aside from
/// what is quoted.
#[test]
fn a_stack_frame_is_never_quoted_and_a_path_that_names_a_credential_word_withholds_nothing() {
    for path in [
        "/x/node_modules/token-lib/auth/index.js",
        "/x/node_modules/lib/index.js",
    ] {
        let said =
            format!("Error: Failed to load extension \"/x/e.ts\": boom\n    at load ({path}:1:1)");
        assert_eq!(
            quotable(&pi_said(&said)),
            pi_said("Error: Failed to load extension \"/x/e.ts\": boom"),
            "{path}"
        );
    }
}

/// Only a line of the shape of a frame is set aside: prose that begins with `at` is screened,
/// for it may be the label of the value below it.
#[test]
fn a_line_that_begins_with_at_and_is_no_frame_is_screened_with_the_rest() {
    for words in [
        "at the end, token:\nabcdefgh1234",
        "at least one key:\nsk-abcd",
        "at the top, password:\ncorrect horse",
    ] {
        assert_eq!(quotable(&pi_said(words)), withheld(), "{words}");
    }
    // Prose that begins with `at` and names no credential is no frame: it is quoted.
    let prose = "Error: boom\nat startup the extension failed to load";
    assert_eq!(quotable(&pi_said(prose)), pi_said(prose));
    for frame in [
        "at load (/x/e.ts:1:1)",
        "at /x/e.ts:12:34",
        "at async Promise.all (index 0)",
        "at Object.<anonymous> (/x/e.js:3:9)",
        "at new Promise (<anonymous>)",
        "at Array.map (native)",
        "at Module._compile (node:internal/modules/cjs/loader:1554:14)",
    ] {
        let said = quotable(&pi_said(&format!("Error: boom\n{frame}")));
        assert!(
            said == pi_said("Error: boom")
                || said == pi_said("Error: boom\nat async Promise.all (index 0)"),
            "{frame}: {said}"
        );
    }
}

#[test]
fn the_lines_that_are_quoted_are_masked_as_a_note_masks_them() {
    let said = quotable(&pi_said(
        "fetch failed: https://alice:swordfish@example.invalid/x",
    ));
    assert_eq!(
        said,
        pi_said("fetch failed: https://[masked]@example.invalid/x")
    );
}

/// ConsensFlow's own description stays before the mark whatever Pi said, and is masked as a
/// note masks, not screened: it names packages and extensions by name, and a package called
/// `token-provider` is not a credential.
#[test]
fn the_description_before_the_mark_is_kept_and_what_pi_was_loading_is_named() {
    let reason = format!(
        "pi --mode rpc did not answer within 30 s. Pi loads the packages and extensions it has \
         before it lists, and one of them may be waiting for something (packages \
         npm:@scope/token-provider@1.2.3; extensions ./oauth.ts, ./secret=1.ts){IT_SAID}password: correct\nhorse"
    );
    let told = quotable(&reason);
    assert!(
        told.starts_with(
            "pi --mode rpc did not answer within 30 s. Pi loads the packages and extensions it has \
             before it lists, and one of them may be waiting for something (packages \
             npm:@scope/token-provider@1.2.3; extensions ./oauth.ts, ./secret=1.ts)"
        ),
        "{told}"
    );
    assert!(
        told.ends_with(WORDS_WITHHELD) && !told.contains("horse"),
        "{told}"
    );
}

/// The first mark is where the program's words begin: a mark among them, forged to make what
/// follows pass for ConsensFlow's, only makes the block longer.
#[test]
fn a_mark_forged_among_the_words_of_pi_begins_nothing() {
    let reason = pi_said(&format!("fine\n{IT_SAID}password: correct\nhorse"));
    assert_eq!(quotable(&reason), withheld());
    let reason = pi_said(&format!("password: correct\nhorse{IT_SAID}the rest"));
    assert_eq!(quotable(&reason), withheld());
}

/// A reason with no mark is no program's words. It is masked as it was in round 2, and the
/// login, key and billing messages of the other harnesses read as they did (the third reading,
/// F9). The words of the screens' rule, applied to them, withheld every one.
#[test]
fn a_reason_with_no_mark_is_only_masked_so_the_login_and_billing_messages_read_as_they_did() {
    for words in [
        "Invalid API key · Please run /login",
        "its login was refused: Invalid API key",
        "Not logged in. Run `devin auth login` to authenticate.",
        "devin models list --format json: Not logged in. Run `devin auth login` to authenticate.",
        "No Meta credentials were found. Run `muse login`.",
        "API error 401 [request_id=x]: invalid api key (authentication_error)",
        "credential error: authentication failed for the account",
        "deferred_token_held",
        "key: abc",
        "Invalid key: expected string",
    ] {
        assert_eq!(quotable(words), words, "{words}");
    }
    // What the mask hides, it hides in them too: a key a harness echoes.
    assert_eq!(
        quotable("it said sk-live_0123456789abcdefghijklmnop"),
        "it said [masked]"
    );
    // The mask is the weaker rule, and it is all those harnesses' own messages get: it hides the
    // first word of a password and no more. Pi's words, which a program of the human's writes,
    // are what the block rule is for.
    assert_eq!(
        quotable("password: correct horse battery staple"),
        "password: [masked] horse battery staple"
    );
}

#[test]
fn a_reason_with_nothing_is_nothing() {
    assert_eq!(quotable(""), "");
    assert_eq!(quotable(&pi_said("")), ENDED);
}

/// T-1276, `frame-label-continuation`: a line of the shape of a frame was set aside before the
/// look, and a label in it went with it; the three words of its value on the lines below it
/// were quoted. Every line is looked at, a frame among them; a frame is only never quoted.
#[test]
fn a_label_in_a_line_of_the_shape_of_a_frame_withholds_the_lines_of_its_value() {
    for words in [
        "at password: correct (native)\nhorse\nbattery\nstaple",
        "    at password: correct (native)\n    horse\n    battery\n    staple\n",
        "Error: boom\nat password: correct (native)\nhorse\nbattery\nstaple",
        "at token: abcd1234 (a.js:1:2)\nefgh ijkl mnop",
        "at load (<anonymous>)\nat api_key: red12345 (index 0)\ngreen blue",
    ] {
        let said = quotable(&pi_said(words));
        assert_eq!(said, withheld(), "{words:?}");
        for value in [
            "correct",
            "horse",
            "battery",
            "staple",
            "abcd",
            "green blue",
        ] {
            assert!(!said.contains(value), "{value} in {said}");
        }
    }
}

/// T-1276, `wrapped-url-userinfo`: a line end in the middle of a URL's user and password left
/// neither line with both of them, and the half before the end was quoted.
#[test]
fn a_url_whose_user_and_password_a_line_end_splits_withholds_the_block() {
    for words in [
        "FetchError: https://alder:copper\nwillow@host.invalid/catalog",
        "FetchError: https://alder:\ncopperwillow@host.invalid/catalog",
        "FetchError: https://al\nder:copperwillow@host.invalid/catalog",
        "FetchError: https://alder:cop\nperwil\nlow@host.invalid/catalog",
        "FetchError: https://alder:copper\r\nwillow@host.invalid/catalog",
        "FetchError: https://alder:copper\n   willow@host.invalid/catalog",
        "FetchError: https://\nalder:copperwillow@host.invalid/catalog",
        "FetchError: https://alder:copperwillow\n@host.invalid/catalog",
    ] {
        let said = quotable(&pi_said(words));
        assert_eq!(said, withheld(), "{words:?}");
        for value in ["alder", "copper", "willow"] {
            assert!(!said.contains(value), "{value} in {said}");
        }
    }
    // A URL with no password in it, a line end after it, and a host on the next line, are none.
    for words in [
        "FetchError: request to https://catalog.invalid/models failed\nCheck your network.",
        "see https://host.invalid/a\nthen write to the owner",
    ] {
        assert_eq!(quotable(&pi_said(words)), pi_said(words), "{words:?}");
    }
}

/// What a read of Pi puts after the mark when it kept only the end of what Pi said (the label
/// of a value can be in the front it lost): the sentence, and it is told as it stands.
#[test]
fn the_sentence_a_cut_read_put_in_place_of_pis_words_is_told_as_it_stands() {
    assert_eq!(quotable(&withheld()), withheld());
}

/// T-1276, `frame-wrapped-token`: a frame between the two halves of a name or a token the wrap
/// cut in two. Every line is looked at, and so are the lines that are no frame, joined as they
/// are with the frames out of the way: `s` / `at load (native)` / `k-fake…` is `sk-fake…`.
#[test]
fn a_name_or_token_cut_in_two_with_a_frame_between_its_halves_is_found_at_the_join() {
    for words in [
        "s\nat load (native)\nk-fakeCloverOak12345",
        "passw\n    at load (/x/e.ts:1:1)\nord: invented-otter-river",
        "Error: boom:\ngh\nat async Promise.all (index 0)\np_FAKEAspenMaple1234567890123456",
    ] {
        let said = quotable(&pi_said(words));
        assert_eq!(said, withheld(), "{words:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// Pi's harmless failure hints (A4-08, the Limits of the decision HMD-13): a word that names a
// credential is no credential. A block is withheld for a value or a shape, not for a word.
// ---------------------------------------------------------------------------------------------

/// What alpha.3 withheld though it held no credential, by the decision's own list, and Pi 1.5.0's
/// own wordings of a login that fails (`dist/core/auth-guidance.js`, `agent-session.js`,
/// `auth-storage.js`, `model-runtime.js`).
#[test]
fn the_failures_of_pi_that_name_a_credential_and_hold_none_are_quoted_as_they_were() {
    for said in [
        // The decision's list.
        "Invalid key: expected string",
        "OAuth login expired. Run /login",
        "OPENAI_API_KEY is not set",
        "Error: Cannot find module 'oauth-helper'\nRequire stack:\n- /home/h/.pi/agent/extensions/provider.ts",
        "Unexpected token: }",
        // Pi 1.5.0's own.
        "No API key found for openrouter.\nUse /login to log into a provider via OAuth or API key. See:\n/x/docs/providers.md\n/x/docs/models.md",
        "Authentication failed for \"openai-codex\". Credentials may have expired or network is unavailable. Run '/login openai-codex' to re-authenticate.",
        "Invalid auth.json credential for provider \"openai-codex\"",
        "Provider is not configured: openrouter",
        "Failed to load token from /home/h/.pi/agent/auth.json: ENOENT",
        // A path or a package named for a credential word, and a frame in one.
        "Error: Failed to load extension \"/home/h/.pi/agent/extensions/oauth-provider.ts\": boom\nHint: Start without extensions using \"pi -ne\".",
        "Error: Failed to load extension \"/x/node_modules/@scope/token-provider/index.ts\": boom",
    ] {
        assert_eq!(quotable(&pi_said(said)), pi_said(said), "{said}");
    }
    // A frame is never quoted, whatever its path is called; it is no reason to withhold the rest.
    let said = "Error: Failed to load extension \"/x/e.ts\": boom\n    at load (/x/node_modules/token-lib/auth/index.js:1:1)";
    assert_eq!(
        quotable(&pi_said(said)),
        pi_said("Error: Failed to load extension \"/x/e.ts\": boom")
    );
}

const SETUPS: &str = include_str!("../../../../cf-harness/tests/fixtures/pi-homes/scenarios.json");
const RECORDED: &str =
    include_str!("../../../../cf-harness/tests/fixtures/pi-homes/answers-mac.json");

/// The words the recorded Pi 1.5.0 gave for the setup that ended on a failed extension.
fn recorded_failure() -> String {
    let all: serde_json::Value = serde_json::from_str(RECORDED).unwrap();
    all["scenarios"]["extension-fails"]["with"]["stderr"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The recorded setups the live suite reads (`tests/fixtures/pi-homes`): ten of them, each read
/// with the extensions Pi loads and without. What Pi says when it fails to load one is its words
/// about what it was loading, and its advice. The names in each setup (its files and its providers)
/// are what Pi would name, and none of them is a credential: `auth.json`, `models.json`, a package
/// called `pi-test-provider`, an extension, `provider-cache.json`.
#[test]
fn what_pi_names_of_the_recorded_setups_it_fails_in_is_quoted_with_its_advice() {
    let recorded = recorded_failure();
    assert!(
        recorded.contains("Failed to load extension") && recorded.contains("Hint: Start without"),
        "{recorded}"
    );
    // The recorded words themselves.
    assert_eq!(
        quotable(&pi_said(recorded.trim())),
        pi_said(recorded.trim())
    );
    let setups: serde_json::Value = serde_json::from_str(SETUPS).unwrap();
    let answers: serde_json::Value = serde_json::from_str(RECORDED).unwrap();
    let setups = setups["scenarios"].as_array().unwrap();
    assert_eq!(setups.len(), 10, "the ten recorded setups");
    let mut checked = 0;
    for setup in setups {
        let name = setup["name"].as_str().unwrap();
        for read in ["with", "without"] {
            let answer = &answers["scenarios"][name][read];
            assert!(!answer.is_null(), "{name} {read}");
            // The words of a Pi that fails in this setup: each file it holds is what Pi was
            // loading, and each provider it lists is one that may fail.
            let mut words: Vec<String> = setup["files"]
                .as_object()
                .unwrap()
                .keys()
                .map(|file| {
                    recorded
                        .replace("extensions/broken.ts", file)
                        .trim()
                        .to_owned()
                })
                .collect();
            if let Some(providers) = answer["providers"].as_object() {
                words.extend(providers.keys().map(|provider| {
                    format!(
                        "Authentication failed for \"{provider}\". Run '/login {provider}' to re-authenticate."
                    )
                }));
            }
            words.push(format!(
                "Error: Failed to load extension \"<agent>/{name}\": boom"
            ));
            for said in words {
                assert_eq!(
                    quotable(&pi_said(&said)),
                    pi_said(&said),
                    "{name} ({read}): {said}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 50, "{checked} words looked at");
}
