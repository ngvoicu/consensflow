//! What the words of a run ask for, and what they are refused for.

use super::*;

fn of(line: &str) -> Result<Options> {
    let words: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
    Options::of(&words)
}

#[test]
fn a_run_with_no_words_is_six_decisions_with_a_claude_chief_and_a_claude_staff() {
    let options = of("").unwrap();
    assert_eq!(options.scenario.id, "six-decisions");
    assert_eq!(options.chief, "claude");
    assert_eq!(options.staff, ["claude"]);
    assert_eq!(options.arm, Arm::Card);
    assert_eq!(options.repeat, 1);
    assert_eq!(options.timeout, Duration::from_secs(40 * 60));
    assert_eq!(
        (options.effort.as_str(), options.staff_effort.as_str()),
        ("high", "medium")
    );
    assert_eq!(options.claude_staff_model, "claude-haiku-4-5-20251001");
    assert_eq!(
        options.pi_staff_model,
        "openrouter/deepseek/deepseek-v4.1-flash"
    );
    assert!(!options.gate && options.model.is_none() && options.switch_to.is_none());
    // The reports are kept where the summary reads them, unless the words say another place.
    assert_eq!(options.reports, cf_e2e::checkout::path("evals/reports"));
    assert_eq!(
        of("--reports /tmp/kept").unwrap().reports,
        PathBuf::from("/tmp/kept")
    );
}

#[test]
fn the_staff_is_the_harnesses_named_with_their_spaces_taken_off_and_the_chief_alone_when_none_is() {
    let options = of("--scenario round-trip --chief codex --staff claude,pi").unwrap();
    assert_eq!(options.scenario.id, "round-trip");
    assert_eq!(options.staff, ["claude", "pi"]);
    assert_eq!(of("--chief pi").unwrap().staff, ["pi"]);
    let words = vec!["--staff".to_owned(), " claude , codex ".to_owned()];
    assert_eq!(Options::of(&words).unwrap().staff, ["claude", "codex"]);
}

#[test]
fn the_chief_has_an_effort_where_its_harness_has_a_switch_for_one() {
    for (chief, effort) in [
        ("claude", Some("high")),
        ("codex", Some("high")),
        ("pi", Some("high")),
        ("opencode", None),
        ("devin", None),
    ] {
        assert_eq!(
            of(&format!("--chief {chief}")).unwrap().chief_effort(),
            effort,
            "{chief}"
        );
    }
    assert_eq!(of("--effort low").unwrap().chief_effort(), Some("low"));
}

#[test]
fn a_harness_is_in_the_run_as_the_chief_or_on_the_staff() {
    let options = of("--chief codex --staff claude,pi").unwrap();
    for (name, in_run) in [
        ("codex", true),
        ("claude", true),
        ("pi", true),
        ("devin", false),
    ] {
        assert_eq!(options.in_run(name), in_run, "{name}");
    }
    assert_eq!(
        options.staff_models(),
        [
            ("claude", "claude-haiku-4-5-20251001"),
            ("pi", "openrouter/deepseek/deepseek-v4.1-flash")
        ]
    );
}

#[test]
fn an_arm_a_scenario_or_a_harness_that_is_none_is_refused_by_name() {
    assert_eq!(
        of("--arm both").unwrap_err().to_string(),
        "no such arm: both"
    );
    let scenario = of("--scenario nope").unwrap_err().to_string();
    assert!(
        scenario.starts_with("no such scenario: nope (advice-trip, chief-switch"),
        "{scenario}"
    );
    assert_eq!(
        of("--chief kimi").unwrap_err().to_string(),
        "no such eval harness: kimi"
    );
    assert_eq!(
        of("--staff claude,kimi").unwrap_err().to_string(),
        "no such eval harness: kimi"
    );
    assert!(matches!(of("--arm both").unwrap_err(), Error::Usage(_)));
}

#[test]
fn a_scenario_that_switches_the_chief_needs_a_switch_to_that_is_on_the_staff_and_is_not_the_chief()
{
    let message = "chief-switch switches the chief: --switch-to names a --staff harness other than the chief's";
    for line in [
        "--scenario chief-switch",
        "--scenario chief-switch --chief claude --staff claude,codex",
        "--scenario chief-switch --chief claude --staff claude --switch-to claude",
        "--scenario chief-switch --chief claude --staff claude,codex --switch-to pi",
        "--scenario chief-switch --chief claude --staff claude,codex --switch-to claude",
    ] {
        assert_eq!(of(line).unwrap_err().to_string(), message, "{line}");
    }
    let options =
        of("--scenario chief-switch --chief claude --staff claude,codex --switch-to codex")
            .unwrap();
    assert_eq!(options.switch_to.as_deref(), Some("codex"));
    // Another scenario has no use for it.
    assert!(of("--scenario round-trip --switch-to codex").is_ok());
}

#[test]
fn a_bare_chief_has_no_card_to_be_switched_from_so_the_scenario_that_switches_is_refused_for_it() {
    assert_eq!(
        of("--scenario chief-switch --arm bare --staff claude,codex --switch-to codex")
            .unwrap_err()
            .to_string(),
        "chief-switch switches the chief from its card, which a bare chief has none of"
    );
    assert!(of("--scenario round-trip --arm bare").is_ok());
}

#[test]
fn a_number_of_runs_or_minutes_that_is_no_number_is_the_humans_mistake() {
    assert_eq!(of("--repeat 3").unwrap().repeat, 3);
    // The login a command's plays run on must last every repeat at its limit.
    assert_eq!(
        of("--repeat 3 --timeout-min 40").unwrap().planned(),
        Duration::from_secs(120 * 60)
    );
    assert_eq!(of("").unwrap().planned(), Duration::from_secs(40 * 60));
    assert_eq!(
        of("--timeout-min 0.5").unwrap().timeout,
        Duration::from_secs(30)
    );
    for line in [
        "--repeat many",
        "--timeout-min soon",
        "--repeat -1",
        "--timeout-min NaN",
    ] {
        assert!(matches!(of(line), Err(Error::Usage(_))), "{line}");
    }
    assert_eq!(
        of("--repeat many").unwrap_err().to_string(),
        "--repeat takes a number"
    );
}

#[test]
fn a_gate_and_a_model_are_taken_as_given() {
    let options = of("--gate --model claude-sonnet-5 --arm nocard --arm bare").unwrap();
    assert!(options.gate);
    assert_eq!(options.model.as_deref(), Some("claude-sonnet-5"));
    assert_eq!(
        options.arm,
        Arm::Bare,
        "the last of a flag given twice is the one"
    );
    assert_eq!(Arm::NoCard.name(), "nocard");
}
