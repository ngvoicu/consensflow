//! What every live test is held to, and what they share of saying and waiting.

use std::cell::Cell;
use std::fs;
use std::time::Duration;

use serde_json::json;

use super::*;

/// The 1-based lines of the `#[test]`s of `source` that no `#[ignore` stands
/// beside, above or below (the two attributes are a pair of lines, in either
/// order), and that no `#[cfg(test)]` before them says is a unit test of the
/// file's own helpers: those run nothing live.
fn unignored(source: &str) -> Vec<usize> {
    let lines: Vec<&str> = source.lines().map(str::trim).collect();
    let live = lines
        .iter()
        .position(|line| *line == "#[cfg(test)]")
        .unwrap_or(lines.len());
    (0..live)
        .filter(|at| lines[*at] == "#[test]")
        .filter(|at| {
            let beside =
                |line: Option<&&str>| line.is_some_and(|line| line.starts_with("#[ignore"));
            !(beside(lines.get(at + 1))
                || beside(at.checked_sub(1).and_then(|before| lines.get(before))))
        })
        .map(|at| at + 1)
        .collect()
}

/// A run of the workspace's tests that found a live test not ignored would
/// spend the machine's quota, on every developer's machine that has the
/// harnesses, and fail on every one that has not.
#[test]
fn every_live_test_is_ignored_unless_asked_for() {
    let folder = crate::checkout::path("crates/cf-e2e/tests");
    let mut live_files = 0;
    for entry in fs::read_dir(&folder).unwrap() {
        let file = entry.unwrap().path();
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        if !name.starts_with("live_") || !name.ends_with(".rs") {
            continue;
        }
        live_files += 1;
        let source = fs::read_to_string(&file).unwrap();
        let live = source.split("#[cfg(test)]").next().unwrap();
        assert!(live.contains("#[test]"), "{name} holds no live test");
        assert_eq!(
            unignored(&source),
            Vec::<usize>::new(),
            "{name}: a live test is ignored unless asked for (the lines of those that are not)"
        );
    }
    assert!(live_files > 0, "no live test file in {}", folder.display());
}

/// The live tests whose windows run agents of their own, not the models of
/// [`agents`]: the catalog's agents named (`live_agent.rs`), and the catalog's
/// image designer (`live_designer.rs`).
const OWN_AGENTS: [&str; 2] = ["live_agent.rs", "live_designer.rs"];

/// A run that does not begin with `Log::begin` takes a `--models` that no window
/// could be asked for as far as its first window, and says nowhere what its
/// harnesses ran on; one of agents of its own that does not begin with
/// `Log::own_agents` takes a `--models` it never reads without a word.
#[test]
fn every_live_run_on_the_shared_models_begins_by_reading_them_and_the_others_do_not() {
    let folder = crate::checkout::path("crates/cf-e2e/tests");
    let mut begun = 0;
    for entry in fs::read_dir(&folder).unwrap() {
        let file = entry.unwrap().path();
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        if !name.starts_with("live_") || !name.ends_with(".rs") {
            continue;
        }
        let source = fs::read_to_string(&file).unwrap();
        let live = source.split("#[cfg(test)]").next().unwrap();
        if OWN_AGENTS.contains(&name.as_str()) {
            assert!(
                !live.contains("Log::begin("),
                "{name} runs agents of its own"
            );
            assert!(
                live.contains("Log::own_agents(") && !live.contains("Log::new("),
                "{name}: its run does not begin with Log::own_agents, which refuses --models"
            );
            continue;
        }
        begun += 1;
        assert!(
            live.contains("Log::begin("),
            "{name}: its run does not begin with Log::begin"
        );
        assert!(
            !live.contains("Log::new("),
            "{name}: a run of it is a Log::new, which reads no --models"
        );
    }
    assert!(begun > 0, "no live test file in {}", folder.display());
}

#[test]
fn a_run_of_agents_of_its_own_given_models_says_it_reads_none_and_what_to_ask_instead() {
    assert_eq!(
        not_asked_for_models("live agent", "use --agent", None, None),
        Ok(())
    );
    assert_eq!(
        not_asked_for_models(
            "live agent",
            "use --agent",
            Some("claude=claude-opus-5-5"),
            None
        ),
        Err(
            "live agent runs agents of its own, each on the model and effort it has, and reads \
             no --models or --claude-model: --models claude=claude-opus-5-5 would be run on \
             nothing; use --agent"
                .to_owned()
        )
    );
    let claude = not_asked_for_models("live designer", "it runs pygmalion", None, Some("x"));
    assert!(
        claude.as_ref().is_err_and(|refused| refused
            .contains("--claude-model x would be run on nothing; it runs pygmalion")),
        "{claude:?}"
    );
    // The run that is given neither begins.
    assert!(Log::own_agents("live agent", "use --agent").is_ok());
}

#[test]
fn a_test_that_is_not_ignored_is_found_wherever_it_is_in_the_file_before_its_unit_tests() {
    let source = "\
#[test]
#[ignore = \"opt-in\"]
fn one() {}

#[ignore]
#[test]
fn two() {}

#[test]
fn three() {}

#[ignore]
fn not_a_test() {}

#[test]

#[ignore]
fn four_the_attributes_are_not_a_pair() {}

#[cfg(test)]
mod tests {
    #[test]
    fn a_unit_test_of_the_files_own_helpers() {}
}
";
    assert_eq!(unignored(source), [9, 15]);
    assert_eq!(unignored(""), Vec::<usize>::new());
}

#[test]
fn a_live_run_keeps_what_it_made_when_the_variable_says_anything_but_nothing_or_zero() {
    assert!(!kept(None));
    assert!(!kept(Some("")));
    assert!(!kept(Some("0")));
    assert!(kept(Some("1")));
    assert!(kept(Some("yes")));
}

#[test]
fn a_flag_sets_the_variable_of_its_name_in_capitals_with_underscores_for_dashes() {
    assert_eq!(variable("harness"), HARNESS);
    assert_eq!(variable("steps"), STEPS);
    assert_eq!(variable("keep"), KEEP);
    assert_eq!(variable("input-box"), "CONSENSFLOW_LIVE_INPUT_BOX");
    assert!(HARNESS.starts_with(PREFIX) && KEEP.starts_with(PREFIX) && STEPS.starts_with(PREFIX));
}

#[test]
fn a_flag_given_more_than_once_or_with_commas_is_a_list_in_the_order_given() {
    assert_eq!(words("claude,devin"), ["claude", "devin"]);
    assert_eq!(words(" claude , ,devin,"), ["claude", "devin"]);
    assert_eq!(words("8000"), ["8000"]);
    assert!(words("").is_empty());
    assert!(words(" , ").is_empty());
    // What the run was not given is nothing.
    assert!(list("flag-that-no-test-is-given").is_empty());
    assert_eq!(option("flag-that-no-test-is-given"), None);
}

#[test]
fn what_cannot_be_read_says_so_and_what_can_is_indented_json() {
    assert_eq!(pretty(Ok(json!({ "a": 1 }))), "{\n  \"a\": 1\n}");
    let failed = pretty(Err(Error::Daemon("no answer".to_owned())));
    assert_eq!(failed, "(could not be read: no answer)");
}

#[test]
fn only_the_warnings_and_errors_of_a_log_are_kept_and_at_most_so_many() {
    let log = "t info start\nt warn slow pass\nt error it broke\nt info done\n";
    assert_eq!(warnings(log), ["t warn slow pass", "t error it broke"]);
    assert!(warnings("t info start\n").is_empty());
    let many: String = (0..30).map(|at| format!("t warn {at}\n")).collect();
    assert_eq!(warnings(&many).len(), WARNINGS);
}

#[test]
fn what_the_product_answered_fits_a_line_cut_where_it_is_too_long() {
    assert_eq!(cut("a\n  b\t c", 10), "a b c");
    assert_eq!(cut("abcdef", 3), "abc…");
    assert_eq!(cut("abc", 3), "abc");
    // Counted in characters, not bytes.
    assert_eq!(cut("ééé", 2), "éé…");
    // A JSON string as the text it is; anything else as JSON writes it.
    assert_eq!(text(&json!("a b")), "a b");
    assert_eq!(text(&json!(7)), "7");
    assert_eq!(text(&json!(null)), "null");
    assert_eq!(text(&json!({ "a": 1 })), r#"{"a":1}"#);
}

#[test]
fn what_a_window_printed_last_is_the_last_eight_lines_with_something_in_them_on_one_line() {
    assert_eq!(last_lines(""), "");
    assert_eq!(last_lines("  one \n\n   \ntwo"), "one ⏎ two");
    let nine: String = (1..=9).map(|at| format!("line {at}\n")).collect();
    assert_eq!(
        last_lines(&nine),
        "line 2 ⏎ line 3 ⏎ line 4 ⏎ line 5 ⏎ line 6 ⏎ line 7 ⏎ line 8 ⏎ line 9"
    );
}

#[test]
fn a_span_of_milliseconds_is_seconds_with_one_decimal() {
    assert_eq!(seconds(0), "0.0");
    assert_eq!(seconds(12_340), "12.3");
    assert_eq!(seconds(-1_500), "-1.5");
    // A span of time is said the same way.
    assert_eq!(secs(Duration::ZERO), "0.0");
    assert_eq!(secs(Duration::from_millis(12_340)), "12.3");
    assert_eq!(secs(Duration::from_secs(301)), "301.0");
}

#[test]
fn a_run_says_how_long_it_has_gone_on_and_a_scope_of_it_goes_on_as_long() {
    let log = Log::new("live test");
    let scoped = log.scoped("claude");
    std::thread::sleep(Duration::from_millis(20));
    assert!(log.elapsed() >= Duration::from_millis(20));
    // One run, whatever the scope that says what it is doing.
    assert!(scoped.elapsed() >= Duration::from_millis(20));
    assert!(scoped.elapsed() <= log.elapsed());
}

#[test]
fn a_wait_ends_with_the_first_thing_it_finds_and_with_nothing_when_the_time_runs_out() {
    let looks = Cell::new(0);
    let found = until(Duration::from_secs(10), Duration::from_millis(1), || {
        looks.set(looks.get() + 1);
        (looks.get() == 3).then_some("there")
    });
    assert_eq!((found, looks.get()), (Some("there"), 3));
    let nothing: Option<()> = until(Duration::from_millis(30), Duration::from_millis(5), || None);
    assert_eq!(nothing, None);
    // It looks once even where it is given no time.
    let once = Cell::new(0);
    until(Duration::ZERO, Duration::from_millis(1), || {
        once.set(once.get() + 1);
        None::<()>
    });
    assert_eq!(once.get(), 1);
}

#[test]
fn a_harness_that_cannot_be_found_or_run_has_an_unknown_version_and_says_why() {
    let nowhere = Installed::at(Harness::Pi, "/no/such/folder/pi", "/home/me");
    assert_eq!(said_version(Ok(nowhere)), "unknown");
    let refused = Error::Daemon("could not find pi: log in by running `pi` once".to_owned());
    assert_eq!(
        said_version(Err(refused)),
        "unknown (could not find pi: log in by running `pi` once)"
    );
}

#[test]
fn a_harness_found_is_said_with_its_program_its_version_and_where_its_login_is_read_from() {
    let install = Installed::at(Harness::Pi, "/usr/local/bin/pi", "/home/me");
    assert_eq!(
        found_line(&install, "1.2.3"),
        "pi is /usr/local/bin/pi (1.2.3); its login is read from /home/me"
    );
    let codex = Installed::at(Harness::Codex, "/opt/codex", "/home/me/.codex");
    assert_eq!(
        found_line(&codex, "unknown"),
        "codex is /opt/codex (unknown); its login is read from /home/me/.codex"
    );
}
