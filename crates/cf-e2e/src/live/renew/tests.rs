use std::time::Duration;

use super::*;

/// What a login holds that must never be said.
const SECRET: &str = "sk-ant-oat01-SECRET-0123456789";

fn machine() -> TestMachine {
    TestMachine::signalled(Some("zeewin2")).unwrap()
}

#[test]
fn a_run_is_on_a_test_machine_only_where_the_runner_says_which() {
    // The Windows runner gives the command it runs this variable, with the machine's name, and
    // nothing else does: a Mac has no runner, so no variable, and no renewal (T-1067).
    assert_eq!(TEST_MACHINE, "CONSENSFLOW_TEST_MACHINE");
    assert_eq!(machine().name(), "zeewin2");
    assert_eq!(
        TestMachine::signalled(Some(" zeewin ")).unwrap().name(),
        "zeewin"
    );
    assert_eq!(TestMachine::signalled(None), None);
    assert_eq!(TestMachine::signalled(Some("")), None);
    assert_eq!(TestMachine::signalled(Some("   ")), None);
}

#[test]
fn a_time_is_said_in_utc_and_a_login_that_holds_none_says_so() {
    assert_eq!(time(Some(1_800_000_000_000)), "2027-01-15T08:00:00Z");
    assert_eq!(time(None), "at no time that can be read");
    assert_eq!(time(Some(i64::MAX)), "at no time that can be read");
}

fn renewed(before: Option<i64>, after: Option<i64>) -> Renewed {
    Renewed {
        file: "~/.claude/.credentials.json".to_owned(),
        login: "Claude Code's login".to_owned(),
        before,
        after,
        took: Duration::from_secs(12),
        ended: None,
    }
}

#[test]
fn what_a_renewal_came_to_names_the_file_and_when_its_login_ended_before_and_after() {
    let moved = renewed(Some(1_800_000_000_000), Some(1_800_028_800_000));
    assert!(moved.moved());
    assert_eq!(
        moved.said(&machine()),
        "the kit ran the harness once as the owner on zeewin2 to renew Claude Code's login: \
         ~/.claude/.credentials.json ended 2027-01-15T08:00:00Z, and ends 2027-01-15T16:00:00Z (12 s)"
    );
    let stayed = renewed(Some(1_800_000_000_000), Some(1_800_000_000_000));
    assert!(!stayed.moved());
    assert!(
        stayed.said(&machine()).contains("it did not move"),
        "{}",
        stayed.said(&machine())
    );
    // A login that ends sooner, or is gone, did not move forward either.
    assert!(!renewed(Some(2), Some(1)).moved());
    assert!(!renewed(Some(1), None).moved());
    assert!(!renewed(None, Some(1)).moved());
}

#[test]
fn a_renewal_that_ran_and_failed_says_so_and_how() {
    let renewal = Renewal::new(
        machine(),
        "Claude Code",
        "only when five minutes of it are left",
        || Ok(None),
    );
    let said = renewal.failed("it ended 1 (saying nothing)");
    assert_eq!(
        said,
        "On zeewin2 the kit ran Claude Code once as the owner to renew it (T-1067), and the renewal \
         ran and failed: it ended 1 (saying nothing)"
    );
    // One that moved the login, but not far enough, says when it ends now.
    let now = 1_799_996_400_000;
    let short = renewal.ended(
        &renewed(Some(1_800_000_000_000), Some(1_800_003_600_000)),
        now,
    );
    assert!(
        short.contains("the login now ends 2027-01-15T09:00:00Z, which is still short"),
        "{short}"
    );
    // One that moved nothing says how long the login has left and that the harness renews it
    // itself once it has ended, when the kit asks it to, so that the wait is understood.
    let same = renewal.ended(
        &renewed(Some(1_800_000_000_000), Some(1_800_000_000_000)),
        now,
    );
    assert!(
        same.contains("the login still ends 2027-01-15T08:00:00Z (in 1 h): Claude Code renews a token only when five minutes of it are left"),
        "{same}"
    );
    assert!(
        same.contains("run again once it has ended, when the kit runs Claude Code and it renews its login itself"),
        "{same}"
    );
    assert!(same.contains("the renewal ran and failed"), "{same}");
    // One that has ended and was not renewed has a refresh token that no longer works.
    let gone = renewal.ended(
        &renewed(Some(1_800_000_000_000), Some(1_800_000_000_000)),
        1_800_000_100_000,
    );
    assert!(
        gone.contains("the login has still ended (2027-01-15T08:00:00Z): Claude Code could not renew it, so its refresh token may be used up or revoked"),
        "{gone}"
    );
    assert!(!gone.contains("run again once it has ended"), "{gone}");
    assert!([&short, &same, &gone]
        .iter()
        .all(|said| !said.contains(SECRET)));
}

#[test]
fn a_renewal_runs_the_harness_once_and_reads_the_login_again() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    let ran = Arc::new(AtomicUsize::new(0));
    let keeps = Arc::new(Mutex::new(format!("ends 100 {SECRET}")));
    let (counted, written) = (ran.clone(), keeps.clone());
    let renewal = Renewal::new(
        machine(),
        "Pi",
        "only once the entry has ended",
        move || {
            counted.fetch_add(1, Ordering::SeqCst);
            *written.lock().unwrap() = format!("ends 900 {SECRET}");
            Ok(None)
        },
    );
    let ends = |text: &str| text.split(' ').nth(1).and_then(|ms| ms.parse::<i64>().ok());
    let read = || Ok(keeps.lock().unwrap().clone());
    let kept = Kept {
        file: "~/.pi/agent/auth.json".to_owned(),
        login: "Pi's `openai` login".to_owned(),
        read: &read,
        expiry: &ends,
    };
    let (fresh, renewed) = renewal.renew(&kept, &format!("ends 100 {SECRET}")).unwrap();
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert_eq!(fresh, format!("ends 900 {SECRET}"));
    assert_eq!((renewed.before, renewed.after), (Some(100), Some(900)));
    assert!(renewed.moved());
    // What is said of it has the times and the file, and never the login.
    assert!(!renewed.said(&machine()).contains(SECRET));
}

#[test]
fn a_harness_that_could_not_renew_is_a_renewal_that_failed_and_the_login_is_not_read_again() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let looked = Arc::new(AtomicUsize::new(0));
    let counted = looked.clone();
    let renewal = Renewal::new(machine(), "Pi", "only once the entry has ended", || {
        Err(Error::Daemon("it ended 1 (boom)".to_owned()))
    });
    let read = move || {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(String::new())
    };
    let kept = Kept {
        file: "~/.pi/agent/auth.json".to_owned(),
        login: "Pi's `openai` login".to_owned(),
        read: &read,
        expiry: &|_| None,
    };
    let said = renewal.renew(&kept, "").unwrap_err();
    assert!(
        said.contains("the renewal ran and failed: it ended 1 (boom)"),
        "{said}"
    );
    assert_eq!(looked.load(Ordering::SeqCst), 0);
}

#[test]
fn claude_code_is_asked_for_one_word_in_its_print_mode_on_the_runs_model_and_nothing_else() {
    let words = claude_words("claude-sonnet-5-5");
    assert_eq!(&words[..4], ["-p", PROMPT, "--model", "claude-sonnet-5-5"]);
    for flag in [
        "--no-session-persistence",
        "--strict-mcp-config",
        "--no-chrome",
        "--disable-slash-commands",
    ] {
        assert!(words.iter().any(|word| word == flag), "{flag}: {words:?}");
    }
    // No tools at all: the last flag takes the empty list.
    assert_eq!(&words[words.len() - 2..], ["--tools", ""]);
    assert_nothing_that_logs_in_or_changes_the_owners_choices(&words);
}

#[test]
fn pi_is_asked_for_one_word_in_its_print_mode_on_the_runs_model_and_nothing_else() {
    let words = pi_words("openai/gpt-6-luna");
    assert_eq!(&words[..4], ["-p", PROMPT, "--model", "openai/gpt-6-luna"]);
    for flag in [
        "--no-session",
        "--no-tools",
        "--no-extensions",
        "--no-mcp",
        "--no-skills",
        "--no-context-files",
        "--no-prompt-templates",
    ] {
        assert!(words.iter().any(|word| word == flag), "{flag}: {words:?}");
    }
    assert_nothing_that_logs_in_or_changes_the_owners_choices(&words);
}

/// A renewal never logs in or out, never changes the model or its effort as the owner chose them,
/// and never spends anything but the call (no API key, no credits).
fn assert_nothing_that_logs_in_or_changes_the_owners_choices(words: &[String]) {
    for word in words {
        for forbidden in ["login", "logout", "/model", "/effort", "api-key", "credit"] {
            assert!(!word.contains(forbidden), "{word} is in {words:?}");
        }
    }
    // The prompt is the one word's, and the only word that is not a flag or what a flag takes.
    assert_eq!(PROMPT, "Reply with the single word ok.");
}

#[cfg(unix)]
mod running {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::*;

    /// A program that stands in for a harness: a shell script in `folder` that writes down its words
    /// and the folder it was started in at `log`, then runs `body`, then ends with `status`.
    pub(in crate::live) fn stand_in(folder: &Path, log: &Path, body: &str, status: i32) -> PathBuf {
        let program = folder.join("stand-in");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > '{log}'\npwd >> '{log}'\n{body}\nexit {status}\n",
            log = log.display()
        );
        fs::write(&program, script).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        program
    }

    #[test]
    fn the_harness_runs_once_in_a_folder_of_a_run_that_goes_with_it() {
        let scratch = tempfile::tempdir().unwrap();
        let log = scratch.path().join("log");
        let program = stand_in(scratch.path(), &log, "", 0);
        one_shot(
            &program,
            &["-p".to_owned(), "one word".to_owned(), String::new()],
        )
        .unwrap();
        let written = fs::read_to_string(&log).unwrap();
        let (words, folder) = written.split_once('\n').unwrap();
        assert_eq!(words, "-p one word ");
        // The folder is named as a folder of a run, so that the kit's watches of the owner's setup
        // find anything the harness keeps of it; and it is gone when the harness is.
        assert!(
            folder.contains("consensflow-integration-renew-"),
            "{folder}"
        );
        assert!(!Path::new(folder.trim()).exists(), "{folder}");
    }

    #[test]
    fn a_harness_that_ended_badly_says_how_in_what_it_said_and_not_what_it_printed() {
        let scratch = tempfile::tempdir().unwrap();
        let log = scratch.path().join("log");
        let printing = format!("echo {SECRET}; echo 'Not   logged in' >&2");
        let program = stand_in(scratch.path(), &log, &printing, 3);
        // It ran and ended badly: how, not an error (the renewal is judged by the login it left).
        let said = one_shot(&program, &[]).unwrap().unwrap();
        assert!(said.contains("it ended 3 (Not logged in)"), "{said}");
        assert!(!said.contains(SECRET), "{said}");
        let silent = stand_in(scratch.path(), &log, "", 1);
        let said = one_shot(&silent, &[]).unwrap().unwrap();
        assert!(said.contains("it ended 1 (saying nothing)"), "{said}");
        // One that ended well says nothing; one that is no program is the error.
        let well = stand_in(scratch.path(), &log, "", 0);
        assert_eq!(one_shot(&well, &[]).unwrap(), None);
        assert!(one_shot(&scratch.path().join("none"), &[]).is_err());
    }

    #[test]
    fn a_harness_that_goes_on_past_its_limit_is_ended_and_the_renewal_has_failed() {
        let scratch = tempfile::tempdir().unwrap();
        let log = scratch.path().join("log");
        let program = stand_in(scratch.path(), &log, "sleep 30", 0);
        let started = Instant::now();
        let said = one_shot_within(&program, &[], Duration::from_secs(1), &[])
            .unwrap()
            .unwrap();
        assert!(said.contains("it was ended after 1 s"), "{said}");
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(LIMIT, Duration::from_secs(180));
    }
}

#[cfg(unix)]
pub(in crate::live) use running::stand_in;

#[test]
fn a_harness_that_ended_badly_after_it_renewed_its_login_has_renewed_it_and_one_that_did_not_has_not(
) {
    use std::sync::{Arc, Mutex};

    // Claude Code on the owner's own config, on zeewin2 (2026-11-01), ended 1 on an API error of
    // Sonnet 5.5's safeguards, twice in two runs, on a login that needed no renewal, so it was not
    // seen to renew and then fail. If it renews before such a call, the login moved and the
    // renewal is the login's; if it left the login where it was, it has not renewed, whatever it
    // said.
    let keeps = Arc::new(Mutex::new(format!("ends 100 {SECRET}")));
    let written = keeps.clone();
    let renewal = Renewal::new(
        machine(),
        "Claude Code",
        "when five minutes are left",
        move || {
            *written.lock().unwrap() = format!("ends 900 {SECRET}");
            Ok(Some("it ended 1 (API Error)".to_owned()))
        },
    );
    let ends = |text: &str| text.split(' ').nth(1).and_then(|ms| ms.parse::<i64>().ok());
    let read = || Ok(keeps.lock().unwrap().clone());
    let kept = Kept {
        file: "~/.claude/.credentials.json".to_owned(),
        login: "Claude Code's login".to_owned(),
        read: &read,
        expiry: &ends,
    };
    let (_, renewed) = renewal.renew(&kept, &format!("ends 100 {SECRET}")).unwrap();
    assert!(renewed.moved());
    assert_eq!(renewed.ended.as_deref(), Some("it ended 1 (API Error)"));
    let said = renewed.said(&machine());
    assert!(
        said.contains("the harness ended badly after renewing it: it ended 1 (API Error)"),
        "{said}"
    );
    assert!(!said.contains(SECRET), "{said}");
    // The same ending with the login where it was: the renewal failed, as the harness said.
    let still = Renewal::new(
        machine(),
        "Claude Code",
        "when five minutes are left",
        || Ok(Some("it ended 1 (API Error)".to_owned())),
    );
    let fixed = || Ok(format!("ends 100 {SECRET}"));
    let kept = Kept {
        read: &fixed,
        ..kept
    };
    let said = still
        .renew(&kept, &format!("ends 100 {SECRET}"))
        .unwrap_err();
    assert!(
        said.contains("the renewal ran and failed: it ended 1 (API Error)"),
        "{said}"
    );
}
