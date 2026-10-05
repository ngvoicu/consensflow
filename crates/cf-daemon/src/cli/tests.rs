//! The verb's arguments as Node's `parseArgs` reads them (probed on Node
//! v26.8.1, strict, positionals allowed, `json` and `no-open` booleans), and
//! what it prints for the handle line.

use super::*;

fn words(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn flags(json: bool, no_open: bool) -> Result<Flags, String> {
    Ok(Flags { json, no_open })
}

#[test]
fn the_two_options_and_positionals_are_read_as_parse_args_reads_them() {
    for (args, expected) in [
        (&[][..], flags(false, false)),
        (&["--json"], flags(true, false)),
        (&["--no-open"], flags(false, true)),
        (&["--no-open", "--json"], flags(true, true)),
        (&["--json", "--json"], flags(true, false)),
        // Positionals are allowed and ignored.
        (&["pos", "--json"], flags(true, false)),
        (&["-"], flags(false, false)),
        // After `--` every word is a positional, an option's look included.
        (&["--no-open", "--", "--foo"], flags(false, true)),
        (&["--", "--json"], flags(false, false)),
        (&["--"], flags(false, false)),
    ] {
        assert_eq!(parse(&words(args)), expected, "{args:?}");
    }
}

#[test]
fn what_it_does_not_know_is_refused_in_the_words_of_node_which_has_no_closing_quote() {
    let unknown = |option: &str| {
        Err(format!(
            "Unknown option '{option}'. To specify a positional argument starting with a '-', place it at the end of the command after '--', as in '-- \"{option}\""
        ))
    };
    for (args, expected) in [
        (&["--foo"][..], unknown("--foo")),
        (&["--foo=bar"], unknown("--foo")),
        (&["--jso"], unknown("--jso")),
        (&["--JSON"], unknown("--JSON")),
        (&["---x"], unknown("---x")),
        (&["--=x"], unknown("--=x")),
        (&["-j"], unknown("-j")),
        (&["-jx"], unknown("-j")),
        (&["-ab"], unknown("-a")),
        (&["-j=1"], unknown("-j")),
        (&["--json", "-x"], unknown("-x")),
    ] {
        assert_eq!(parse(&words(args)), expected, "{args:?}");
    }
}

#[test]
fn a_boolean_option_that_is_given_a_value_is_refused_and_the_first_refusal_wins() {
    let refused = |option: &str| Err(format!("Option '--{option}' does not take an argument"));
    assert_eq!(parse(&words(&["--json=x"])), refused("json"));
    assert_eq!(parse(&words(&["--no-open=1"])), refused("no-open"));
    assert_eq!(parse(&words(&["--json=x", "--foo"])), refused("json"));
    assert_eq!(parse(&words(&["--json", "--no-open="])), refused("no-open"));
}

fn handle() -> HandleLine {
    HandleLine {
        url: "http://127.0.0.1:43517/".to_owned(),
        token: "ab".repeat(24),
    }
}

#[test]
fn the_app_gets_the_handle_line_as_json_and_nothing_else() {
    let printed = lines(
        Flags {
            json: true,
            no_open: true,
        },
        &handle(),
    )
    .unwrap();
    assert_eq!(
        printed,
        [format!(
            r#"{{"url":"http://127.0.0.1:43517/","token":"{}"}}"#,
            "ab".repeat(24)
        )]
    );
}

#[test]
fn a_person_gets_the_address_with_the_token_in_its_query_and_how_to_stop() {
    let printed = lines(Flags::default(), &handle()).unwrap();
    assert_eq!(
        printed,
        [
            format!("agents: http://127.0.0.1:43517/?token={}", "ab".repeat(24)),
            "Ctrl-C to stop — nothing keeps running after it.".to_owned(),
        ]
    );
}
