use super::*;

fn process(pid: u32, command: &str) -> BackgroundProcess {
    BackgroundProcess {
        pid,
        command: command.to_owned(),
    }
}

#[test]
fn a_note_quotes_the_first_commands_and_says_how_many_it_leaves_out() {
    let left: Vec<_> = (1..=7)
        .map(|n| process(n, &format!("python3 job.py {n}")))
        .collect();
    assert_eq!(
        quote(&left),
        "python3 job.py 1; python3 job.py 2; python3 job.py 3; python3 job.py 4; python3 job.py 5; and 2 more"
    );
    // A host that lists none of the processes it names still says how many.
    assert_eq!(quote_of(&[], 2), "2 processes");
    // A host that lists the first sixteen of twenty counts the other four too.
    let listed: Vec<_> = (1..=16)
        .map(|n| process(n, &format!("python3 job.py {n}")))
        .collect();
    assert!(quote_of(&listed, 20).ends_with("python3 job.py 5; and 15 more"));
}

#[test]
fn a_command_is_one_line_and_bounded_and_a_secret_in_it_is_hidden() {
    let long = format!("tool {}", "x".repeat(400));
    let quoted = quote(&[process(1, &long)]);
    assert_eq!(quoted.chars().count(), COMMAND_CHARS);
    assert!(quoted.ends_with('…'));

    let quoted = quote(&[process(
        2,
        "curl -H 'Authorization: Bearer abcdef0123456789abcdef' --data  a\nb",
    )]);
    assert!(!quoted.contains("abcdef0123456789abcdef"), "{quoted}");
    assert!(quoted.contains("[masked]"), "{quoted}");
    assert!(
        quoted.ends_with("(1 key- or token-like string masked)"),
        "{quoted}"
    );
    assert!(!quoted.contains('\n'));
}

#[test]
fn a_process_whose_command_could_not_be_read_is_named_by_its_number() {
    assert_eq!(quote(&[process(42, "")]), "process 42");
}
