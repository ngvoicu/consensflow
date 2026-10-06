//! The words as Node's `parseArgs` reads them (probed on Node v26.8.1,
//! strict): a few of each kind here, the thousands in
//! `tests/args.rs` against the recording.

use super::*;

const UI: [Opt; 2] = [Opt::flag("json"), Opt::flag("no-open")];
const CATALOG: [Opt; 2] = [Opt::text("harness"), Opt::flag("json")];

fn words(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn ui(args: &[&str]) -> Result<Parsed, String> {
    parse(&words(args), &UI, Positionals::Allowed)
}

fn catalog(args: &[&str]) -> Result<Parsed, String> {
    parse(&words(args), &CATALOG, Positionals::Allowed)
}

fn unknown(option: &str, spelled: &str) -> Result<Parsed, String> {
    Err(format!(
        "Unknown option '{option}'. To specify a positional argument starting with a '-', place it at the end of the command after '--', as in '-- {spelled}"
    ))
}

#[test]
fn flags_and_positionals_are_read_as_parse_args_reads_them() {
    for (args, json, no_open, positionals) in [
        (&[][..], false, false, &[][..]),
        (&["--json"], true, false, &[]),
        (&["--no-open", "--json"], true, true, &[]),
        (&["--json", "--json"], true, false, &[]),
        (&["pos", "--json"], true, false, &["pos"]),
        (&["-"], false, false, &["-"]),
        (&[""], false, false, &[""]),
        // After `--` every word is a positional, an option's look included.
        (&["--no-open", "--", "--foo"], false, true, &["--foo"]),
        (&["--", "--json"], false, false, &["--json"]),
        (&["--"], false, false, &[]),
    ] {
        let parsed = ui(args).unwrap();
        assert_eq!(
            (
                parsed.flag("json"),
                parsed.flag("no-open"),
                parsed.positionals
            ),
            (json, no_open, words(positionals)),
            "{args:?}"
        );
    }
}

#[test]
fn a_text_option_takes_the_next_word_whatever_it_looks_like_or_what_follows_its_equals() {
    for (args, harness) in [
        (&["--harness", "claude"][..], Some("claude")),
        (&["--harness=claude"], Some("claude")),
        (&["--harness="], Some("")),
        (&["--harness", ""], Some("")),
        (&["--harness", "-"], Some("-")),
        (&["--harness=--json"], Some("--json")),
        (&["--harness=a=b"], Some("a=b")),
        (&["--harness", "a", "--harness", "b"], Some("b")),
        (&["--harness=a", "--harness", "b"], Some("b")),
        (&["--json"], None),
    ] {
        assert_eq!(catalog(args).unwrap().text("harness"), harness, "{args:?}");
    }
    // It takes the word, so the word is no positional.
    assert_eq!(
        catalog(&["--harness", "x", "y"]).unwrap().positionals,
        words(&["y"])
    );
}

#[test]
fn what_a_verb_does_not_know_is_refused_in_the_words_of_node_which_has_no_closing_quote() {
    for (args, expected) in [
        (&["--foo"][..], unknown("--foo", "\"--foo\"")),
        (&["--foo=bar"], unknown("--foo", "\"--foo\"")),
        (&["--jso"], unknown("--jso", "\"--jso\"")),
        (&["--JSON"], unknown("--JSON", "\"--JSON\"")),
        (&["---x"], unknown("---x", "\"---x\"")),
        (&["--=x"], unknown("--=x", "\"--=x\"")),
        (&["-j"], unknown("-j", "\"-j\"")),
        (&["-jx"], unknown("-j", "\"-j\"")),
        (&["-ab"], unknown("-a", "\"-a\"")),
        (&["-j=1"], unknown("-j", "\"-j\"")),
        (&["--json", "-x"], unknown("-x", "\"-x\"")),
        // The suggestion is the option as JSON writes it.
        (&["--a\"b"], unknown("--a\"b", "\"--a\\\"b\"")),
        (&["--a\\b"], unknown("--a\\b", "\"--a\\\\b\"")),
    ] {
        assert_eq!(ui(args), expected, "{args:?}");
    }
}

#[test]
fn an_equals_right_after_the_dashes_is_part_of_the_name_until_another_comes() {
    // `--=a` is an option named `=a`; `--=a=b` splits at the first `=`, which
    // leaves the name empty and the option `--`.
    assert_eq!(ui(&["--=a"]), unknown("--=a", "\"--=a\""));
    assert_eq!(ui(&["--=a=b"]), unknown("--", "\"--\""));
    assert_eq!(ui(&["--="]), unknown("--=", "\"--=\""));
    assert_eq!(ui(&["--a=b=c"]), unknown("--a", "\"--a\""));
}

#[test]
fn a_letter_past_u_ffff_is_half_a_pair_which_goes_out_as_u_fffd_and_as_its_escape() {
    assert_eq!(ui(&["-😀"]), unknown("-\u{FFFD}", "\"-\\ud83d\""));
    assert_eq!(ui(&["-😀x"]), unknown("-\u{FFFD}", "\"-\\ud83d\""));
    // A long option keeps its whole name, and a letter in the plane below its own.
    assert_eq!(ui(&["--😀"]), unknown("--😀", "\"--😀\""));
    assert_eq!(ui(&["-é"]), unknown("-é", "\"-é\""));
}

#[test]
fn a_flag_given_a_value_is_refused_and_the_first_refusal_wins() {
    let refused = |option: &str| Err(format!("Option '--{option}' does not take an argument"));
    assert_eq!(ui(&["--json=x"]), refused("json"));
    assert_eq!(ui(&["--no-open=1"]), refused("no-open"));
    assert_eq!(ui(&["--json=x", "--foo"]), refused("json"));
    assert_eq!(ui(&["--json", "--no-open="]), refused("no-open"));
    assert_eq!(ui(&["--foo", "--json=x"]), unknown("--foo", "\"--foo\""));
}

#[test]
fn a_text_option_with_no_text_or_with_what_looks_like_an_option_is_refused() {
    assert_eq!(
        catalog(&["--harness"]),
        Err("Option '--harness <value>' argument missing".to_owned())
    );
    assert_eq!(
        catalog(&["--json", "--harness"]),
        Err("Option '--harness <value>' argument missing".to_owned())
    );
    let ambiguous = Err(
        "Option '--harness' argument is ambiguous.\nDid you forget to specify the option argument for '--harness'?\nTo specify an option argument starting with a dash use '--harness=-XYZ'."
            .to_owned(),
    );
    for args in [
        &["--harness", "--json"][..],
        &["--harness", "-x"],
        &["--harness", "--"],
        &["--harness", "-1"],
    ] {
        assert_eq!(catalog(args), ambiguous, "{args:?}");
    }
}

#[test]
fn a_verb_that_takes_no_positionals_refuses_the_first_and_says_no_how_to_pass_one() {
    let none = |args: &[&str]| parse(&words(args), &[], Positionals::Refused);
    assert_eq!(none(&[]).unwrap().positionals, Vec::<String>::new());
    assert_eq!(
        none(&["x"]),
        Err("Unexpected argument 'x'. This command does not take positional arguments".to_owned())
    );
    assert_eq!(
        none(&["--", "x"]),
        Err("Unexpected argument 'x'. This command does not take positional arguments".to_owned())
    );
    // An option it does not know is refused before a positional after it,
    // and with no suggestion, since nothing could follow `--`.
    assert_eq!(
        none(&["--foo", "x"]),
        Err("Unknown option '--foo'".to_owned())
    );
    assert_eq!(
        none(&["x", "--foo"]),
        Err("Unexpected argument 'x'. This command does not take positional arguments".to_owned())
    );
}
