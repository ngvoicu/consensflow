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
        "python3 …; python3 …; python3 …; python3 …; python3 …; and 2 more"
    );
    // A host that lists none of the processes it names still says how many.
    assert_eq!(quote_of(&[], 2), "2 processes");
    // A host that lists the first sixteen of twenty counts the other four too.
    let listed: Vec<_> = (1..=16)
        .map(|n| process(n, &format!("python3 job.py {n}")))
        .collect();
    assert!(quote_of(&listed, 20).ends_with("python3 …; and 15 more"));
}

#[test]
fn a_command_is_its_program_and_never_an_argument_nor_a_path() {
    // The program the list names, alone or with more after it; a path to it is its last part.
    assert_eq!(line(&process(1, "sleep")), "sleep");
    assert_eq!(line(&process(1, "sleep 600")), "sleep …");
    assert_eq!(
        line(&process(
            1,
            "/Users/me/secret-project/bin/node server.mjs --password hunter22"
        )),
        "node …"
    );
    // A program the list does not name is its process's number, whatever it was given.
    for command in [
        "curl -H 'Authorization: Bearer abcdef0123456789abcdef' --data  a\nb",
        "bash -c 'export TOKEN=redgreenblue'",
        "TOKEN=redgreenblue cargo test",
        "/Users/me/my secret/bin/node x",
        "sk-abcdefghijklmnop",
        "",
    ] {
        assert_eq!(line(&process(7, command)), "process 7", "{command}");
    }
    // One whose number is not known (a command a harness lists) is a command.
    assert_eq!(line(&process(0, "bash x")), "a command");
    let quoted = quote(&[process(2, "curl -u alice:swordfish https://example.test/")]);
    assert_eq!(quoted, "process 2");
}

#[test]
fn a_process_whose_command_could_not_be_read_is_named_by_its_number() {
    assert_eq!(quote(&[process(42, "")]), "process 42");
}
