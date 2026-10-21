//! What the words of a long run ask for, and what they are refused for.

use super::*;

fn of(line: &str) -> Result<Options> {
    let words: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
    Options::of(&words)
}

#[test]
fn a_long_run_with_no_words_is_two_hours_of_every_harness_under_a_claude_chief_with_every_disruption(
) {
    let options = of("").unwrap();
    assert_eq!(options.chief, "claude");
    assert_eq!(
        options.staff,
        ["claude", "codex", "pi", "opencode", "devin"]
    );
    assert_eq!(options.budget, Duration::from_secs(2 * 3600));
    assert_eq!(options.every, Duration::from_secs(480));
    assert_eq!(options.look, Duration::from_secs(10));
    assert_eq!(options.quiet, Duration::from_secs(600));
    assert_eq!(options.wake_minutes, 20);
    assert_eq!(options.settings, Settings::default());
    assert_eq!(options.disruptions, Disruption::ALL);
    assert_eq!(options.ask, Ask::Real);
    assert_eq!(options.reports, checkout::path("evals/reports/long"));
    assert_eq!(
        (options.effort.as_str(), options.staff_effort.as_str()),
        ("high", "medium")
    );
    assert_eq!(options.chief_effort(), Some("high"));
}

#[test]
fn each_disruption_is_a_flag_some_named_play_those_alone_and_quiet_plays_none() {
    assert_eq!(
        of("--suspend --pause").unwrap().disruptions,
        [Disruption::Pause, Disruption::Suspend]
    );
    assert!(of("--quiet").unwrap().disruptions.is_empty());
    assert_eq!(
        of("--quiet --kill").unwrap_err().to_string(),
        "--quiet plays no disruption, and names none"
    );
}

#[test]
fn the_staff_the_budget_and_the_times_are_taken_as_given_and_what_is_none_is_refused() {
    let options = of(
        "--chief devin --staff devin,claude --hours 0.25 --every-min 1.5 --look-sec 3 --idle-min 2 --dialog-min 1 --settle-sec 5",
    )
    .unwrap();
    assert_eq!(options.staff, ["devin", "claude"]);
    assert_eq!(options.budget, Duration::from_secs(900));
    assert_eq!(options.every, Duration::from_secs(90));
    assert_eq!(options.settings.idle, 120_000);
    assert_eq!(options.settings.dialog, 60_000);
    assert_eq!(options.settings.settle, 5_000);
    assert_eq!(options.chief_effort(), None);
    for (line, said) in [
        ("--chief kimi", "no such eval harness: kimi"),
        ("--staff claude,kimi", "no such eval harness: kimi"),
        ("--staff ,", "--staff names at least one harness"),
        ("--hours 0", "--hours takes a number above 0"),
        ("--every-min soon", "--every-min takes a number above 0"),
        (
            "--switch-to pi --staff claude",
            "--switch-to names a harness of the staff, not pi",
        ),
        ("--ask both", "--ask is real or stand-in, not both"),
    ] {
        let refused = of(line).unwrap_err();
        assert!(matches!(refused, Error::Usage(_)), "{line}");
        assert_eq!(refused.to_string(), said, "{line}");
    }
}

#[test]
fn the_chief_is_switched_to_the_harness_named_else_the_first_of_the_staff_that_is_another() {
    let staff =
        |names: &[&str]| -> Vec<String> { names.iter().map(|name| (*name).to_owned()).collect() };
    let options = of("--chief claude").unwrap();
    assert_eq!(
        options.switch_target(&staff(&["claude", "codex", "devin"])),
        Some("codex".to_owned())
    );
    assert_eq!(
        options.switch_target(&staff(&["claude"])),
        Some("claude".to_owned())
    );
    let named = of("--chief claude --staff claude,devin --switch-to claude").unwrap();
    assert_eq!(
        named.switch_target(&staff(&["claude", "devin"])),
        Some("claude".to_owned())
    );
    // A harness named that the machine has not: the next one there is.
    let gone = of("--chief claude --staff claude,devin --switch-to devin").unwrap();
    assert_eq!(
        gone.switch_target(&staff(&["claude"])),
        Some("claude".to_owned())
    );
}
