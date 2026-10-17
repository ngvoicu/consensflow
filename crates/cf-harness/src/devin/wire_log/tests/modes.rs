//! The permission mode the shown conversation is in, as the log says it: in the
//! configuration of the conversation the window shows, and in each change of it.
//! A session Devin resumes after a quit cut its first turn comes up in `normal`,
//! whatever its flag says, and `/bypass` changes it (Devin 3000.11.92, 2026-10-15).

use super::*;

/// The configuration of `session` the window shows, its mode `mode`.
fn configured(session: &str, mode: &str) -> String {
    line(&json!({ "sessionId": session, "update": {
        "sessionUpdate": "config_option_update",
        "configOptions": [
            { "id": "mode", "currentValue": mode },
            { "id": "model", "currentValue": "swe-2-high" },
        ],
    } }))
}

/// A change of `session`'s mode to `mode`.
fn changed(session: &str, mode: &str) -> String {
    line(&json!({ "sessionId": session,
        "update": { "sessionUpdate": "current_mode_update", "currentModeId": mode } }))
}

#[test]
fn the_mode_is_the_last_the_log_said_of_the_shown_conversation() {
    let log = Log::new();
    assert_eq!(log.said().mode, None, "an empty log says none");
    log.write(&configured("mild-coin", "normal"));
    let said = log.said();
    assert_eq!(
        (said.shown.as_deref(), said.mode.as_deref()),
        (Some("mild-coin"), Some("normal"))
    );
    log.append(&changed("mild-coin", "bypass"));
    assert_eq!(log.said().mode.as_deref(), Some("bypass"));
    // Read again with nothing new, it says the same.
    assert_eq!(log.said().mode.as_deref(), Some("bypass"));
}

#[test]
fn another_conversations_mode_is_none_of_the_shown_ones_and_a_new_one_is_in_its_own() {
    let log = Log::new();
    log.write(&configured("mild-coin", "bypass"));
    log.append(&changed("fresh-leaf", "normal"));
    assert_eq!(log.said().mode.as_deref(), Some("bypass"));
    // The window shows another conversation: its configuration names no value of the
    // mode, and what the old one was in is not this one's.
    log.append(&shows("fresh-leaf"));
    let said = log.said();
    assert_eq!(
        (said.shown.as_deref(), said.mode.as_deref()),
        (Some("fresh-leaf"), None)
    );
    log.append(&changed("fresh-leaf", "normal"));
    assert_eq!(log.said().mode.as_deref(), Some("normal"));
}

#[test]
fn a_log_replaced_by_one_as_long_says_the_mode_the_new_one_says_and_no_mode_of_the_old_one() {
    // A configuration rewritten at its size from bypass to normal was read as bypass,
    // and the window as ready (Astra and Sol on the B1 fix). Each rewrite keeps the
    // time of the one before, a second before the look, as Windows' clock gives a log
    // rewritten within its tick: only its bytes tell it (main's Windows CI, run
    // 38942619741).
    let log = Log::new();
    let tick = EPOCH_MS - 1_000;
    log.write_at(&configured("mild-coin", "bypass"), tick);
    assert_eq!(log.said().mode.as_deref(), Some("bypass"));
    let normal = configured("mild-coin", "normal");
    assert_eq!(normal.len(), configured("mild-coin", "bypass").len());
    log.write_at(&normal, tick);
    assert_eq!(log.said().mode.as_deref(), Some("normal"));
    // Replaced by one that says no mode: none, and not the one before.
    log.write_at(&shows("mild-coin"), tick);
    let said = log.said();
    assert_eq!(
        (said.shown.as_deref(), said.mode.as_deref()),
        (Some("mild-coin"), None)
    );
    // Emptied, it says no mode either, and what it says next is read whole.
    log.write_at(&configured("mild-coin", "normal"), tick);
    assert_eq!(log.said().mode.as_deref(), Some("normal"));
    log.write_at("", tick);
    assert_eq!(log.said().mode, None);
    log.write_at(&configured("mild-coin", "bypass"), tick);
    assert_eq!(log.said().mode.as_deref(), Some("bypass"));
}

#[test]
fn a_log_last_written_well_before_the_look_that_read_it_is_not_read_again_while_its_time_stays() {
    // Its time and size as the last look found them, and that time older than the look
    // by more than a file time's step: what it holds is not hashed again at each look.
    // Rewritten so at its size, it is the log read before (as git takes a clean entry).
    let log = Log::new();
    let long_ago = EPOCH_MS - 60 * MINUTE;
    log.write_at(&configured("mild-coin", "bypass"), long_ago);
    assert_eq!(log.said().mode.as_deref(), Some("bypass"));
    log.write_at(&configured("mild-coin", "normal"), long_ago);
    assert_eq!(log.said().mode.as_deref(), Some("bypass"));
    // A time of its own, and it is read again.
    log.write_at(&configured("mild-coin", "normal"), long_ago + 1);
    assert_eq!(log.said().mode.as_deref(), Some("normal"));
}

/// Makes the log of `log` written to later than a look found it, whatever the file
/// system's clock: what a rewrite is.
fn written_later(log: &Log) {
    let file = fs::File::options()
        .write(true)
        .open(log.wire.path())
        .unwrap();
    let written = file.metadata().unwrap().modified().unwrap();
    file.set_modified(written + std::time::Duration::from_secs(5))
        .unwrap();
}

#[test]
fn a_log_rewritten_past_its_first_kilobyte_and_grown_says_the_mode_the_rewrite_says() {
    // The first kilobyte the same, a later configuration rewritten from bypass to normal
    // at its size, and a record more after it: read on from where the old log ended, it
    // said bypass, and the window was ready in normal (Astra and Sol on the B1 fix, their
    // second look).
    let log = Log::new();
    let first = says(&"x".repeat(2_048));
    log.write(&format!("{first}{}", configured("mild-coin", "bypass")));
    assert_eq!(log.said().mode.as_deref(), Some("bypass"));
    log.write(&format!(
        "{first}{}{}",
        configured("mild-coin", "normal"),
        says("more")
    ));
    written_later(&log);
    assert_eq!(log.said().mode.as_deref(), Some("normal"));
    // Appended to and nothing rewritten, it is read on, and says the same.
    log.append(&says("and more"));
    written_later(&log);
    assert_eq!(log.said().mode.as_deref(), Some("normal"));
}

#[test]
fn a_log_another_file_was_renamed_over_is_read_from_its_start_whatever_its_size_and_time() {
    // Another file of the same size, written to at the same time, settled when the look
    // found it: only which file it is tells it from the one read.
    let log = Log::new();
    let long_ago = EPOCH_MS - 60 * MINUTE;
    log.write_at(&configured("mild-coin", "bypass"), long_ago);
    assert_eq!(log.said().mode.as_deref(), Some("bypass"));
    let written = fs::metadata(log.wire.path()).unwrap().modified().unwrap();
    let other = log.wire.path().with_file_name("other.jsonl");
    fs::write(&other, configured("mild-coin", "normal")).unwrap();
    fs::File::options()
        .write(true)
        .open(&other)
        .unwrap()
        .set_modified(written)
        .unwrap();
    fs::rename(&other, log.wire.path()).unwrap();
    assert_eq!(log.said().mode.as_deref(), Some("normal"));
}

#[test]
fn a_configuration_that_gives_no_value_of_the_mode_keeps_the_one_said_before() {
    let log = Log::new();
    log.write(&configured("mild-coin", "normal"));
    log.append(&shows("mild-coin"));
    assert_eq!(log.said().mode.as_deref(), Some("normal"));
}
