//! `cf_base::args` held to Node's `util.parseArgs`: every argument list of
//! tests/goldens/args.json (`npm run goldens:cli`, from
//! `tests/goldens/cli/parse-args.mjs`) read with the option set of the verb it
//! was recorded for, and its values, positionals or message compared with
//! what Node answered.

// The goldens' own reading: a failure in it is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cf_base::args::{parse, Opt, Parsed, Positionals};
use serde_json::{json, Value};

/// The option sets of the verbs of `bin/cf.mjs`, as the recording names them.
fn options(verb: &str) -> (Vec<Opt>, Positionals) {
    match verb {
        "ui" => (
            vec![Opt::flag("json"), Opt::flag("no-open")],
            Positionals::Allowed,
        ),
        "catalog" => (
            vec![Opt::text("harness"), Opt::flag("json")],
            Positionals::Allowed,
        ),
        "agent" => (
            vec![
                Opt::text("harness"),
                Opt::text("model"),
                Opt::text("effort"),
                Opt::text("work-tier"),
                Opt::text("description"),
                Opt::flag("designer"),
                Opt::flag("json"),
            ],
            Positionals::Allowed,
        ),
        "setup" => (Vec::new(), Positionals::Refused),
        other => panic!("a verb the tests do not know: {other}"),
    }
}

/// What `parseArgs` returns for what was read: each option the verb takes as
/// Node holds it (a flag as `true`, a text as itself, and none when absent).
fn values(parsed: &Parsed, options: &[Opt]) -> Value {
    let mut values = serde_json::Map::new();
    for option in options {
        if let Some(text) = parsed.text(option.name) {
            values.insert(option.name.to_owned(), json!(text));
        } else if parsed.flag(option.name) {
            values.insert(option.name.to_owned(), json!(true));
        }
    }
    json!({ "values": values, "positionals": parsed.positionals })
}

/// The values Node returned, with the flags it defaulted to `false` taken
/// out: the words did not give them.
fn given(answer: &Value) -> Value {
    let values: serde_json::Map<String, Value> = answer["values"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, value)| **value != json!(false))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    json!({ "values": values, "positionals": answer["positionals"] })
}

#[test]
fn every_list_of_words_is_read_as_node_reads_it() {
    let golden: Value = serde_json::from_str(include_str!("goldens/args.json")).unwrap();
    // The recording's option sets are the ones read here.
    for (verb, spec) in golden["specs"].as_object().unwrap() {
        let (options, positionals) = options(verb);
        let recorded: Vec<(&str, &str)> = spec["options"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, kind)| (name.as_str(), kind.as_str().unwrap()))
            .collect();
        let read: Vec<(&str, &str)> = options
            .iter()
            .map(|option| {
                (
                    option.name,
                    match option.takes {
                        cf_base::args::Takes::Text => "string",
                        cf_base::args::Takes::Nothing => "boolean",
                    },
                )
            })
            .collect();
        assert_eq!(recorded, read, "{verb}");
        assert_eq!(
            spec["positionals"].as_bool().unwrap(),
            positionals == Positionals::Allowed,
            "{verb}"
        );
    }
    let cases = golden["cases"].as_array().unwrap();
    // A recording with fewer has lost some.
    assert!(cases.len() >= 3000, "{} cases", cases.len());
    let mut differ = Vec::new();
    for case in cases {
        let verb = case[0].as_str().unwrap();
        let args: Vec<String> = case[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_str().unwrap().to_owned())
            .collect();
        let (options, positionals) = options(verb);
        let said = match parse(&args, &options, positionals) {
            Ok(parsed) => values(&parsed, &options),
            Err(message) => json!({ "error": message }),
        };
        let expected = if case[2].get("error").is_some() {
            case[2].clone()
        } else {
            given(&case[2])
        };
        if said != expected {
            differ.push(format!(
                "{verb} {args:?}:\n  node: {expected}\n  rust: {said}"
            ));
        }
    }
    assert!(
        differ.is_empty(),
        "{} of {} lists differ:\n\n{}",
        differ.len(),
        cases.len(),
        differ
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n")
    );
}
