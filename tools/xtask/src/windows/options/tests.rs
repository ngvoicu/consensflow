use super::*;
use crate::windows::testing::strings;

fn words(line: &str) -> Vec<OsString> {
    line.split_whitespace().map(OsString::from).collect()
}

fn run(line: &str) -> Result<Options, String> {
    Options::parse(&words(line))
}

#[test]
fn a_host_and_a_command_are_all_a_run_needs_and_the_rest_is_the_usual() {
    assert_eq!(
        run("--host zeewin -- npm run live:paste"),
        Ok(Options {
            machine: Some(Machine::at("zeewin")),
            build: false,
            dir: "consensflow-build".into(),
            inside: ".".into(),
            env: vec![],
            command: strings(&["npm", "run", "live:paste"]),
        })
    );
}

#[test]
fn a_machine_is_named_or_a_host_given_and_with_neither_the_default_is_chosen_when_the_run_goes() {
    let named = run("--machine zeewin2 -- cargo test").unwrap();
    assert_eq!(named.machine, Machine::named("zeewin2"));
    assert!(named.machine.and_then(|machine| machine.shared).is_some());
    assert_eq!(
        run("--machine=zeewin -- x").unwrap().machine,
        Some(Machine::at("zeewin"))
    );
    assert_eq!(run("-- npm test").unwrap().machine, None);
    assert_eq!(run("--build npm test").unwrap().machine, None);
    let options = matrix("--machine zeewin2 --scenario s --pair a:b").unwrap();
    assert_eq!(options.machine, Machine::named("zeewin2"));
    assert_eq!(matrix("--scenario s --pair a:b").unwrap().machine, None);
}

#[test]
fn a_host_that_is_a_machines_name_is_that_machine_with_its_guards_and_its_node() {
    // `--host zeewin2` once skipped poker-lab's lock, the disk floor and CI's Node (T-610).
    for line in [
        "--host zeewin2 -- cargo test",
        "--host=zeewin2 -- cargo test",
        "--machine=zeewin2 -- cargo test",
    ] {
        let machine = run(line).unwrap().machine.unwrap();
        assert_eq!(Some(machine.clone()), Machine::named("zeewin2"), "{line}");
        assert!(machine.shared.is_some(), "{line}");
        assert_eq!(machine.node, r"$env:USERPROFILE\cf-tools\node", "{line}");
    }
    assert_eq!(
        matrix("--host zeewin2 --scenario s --pair a:b")
            .unwrap()
            .machine,
        Machine::named("zeewin2")
    );
    // zeewin is the same machine whichever way it is named; any other host is reached as it is.
    assert_eq!(
        run("--host zeewin -- x").unwrap().machine,
        Machine::named("zeewin")
    );
    assert_eq!(
        run("--host box -- x").unwrap().machine,
        Some(Machine::at("box"))
    );
}

#[test]
fn every_option_is_read_by_name_with_its_value_after_it_or_after_an_equals_sign() {
    let options = run(
        "--host=zeewin --build --dir build-2 --in=app/src-tauri --env A=1 --env=B=x=y -- cargo test",
    )
    .unwrap();
    assert_eq!(
        (options.machine.clone(), options.build),
        (Some(Machine::at("zeewin")), true),
        "{options:?}"
    );
    assert_eq!(options.dir, "build-2");
    assert_eq!(options.inside, "app/src-tauri");
    assert_eq!(
        options.env,
        [("A".into(), "1".into()), ("B".into(), "x=y".into())]
    );
    // Given twice, a text option is the last one given.
    assert_eq!(
        run("--host a --host b -- x").unwrap().machine,
        Some(Machine::at("b"))
    );
}

#[test]
fn the_command_is_what_follows_the_first_double_dash_as_it_was_written() {
    let options = run("--host zeewin -- cargo xtask live paste -- --help --build -x").unwrap();
    assert_eq!(
        options.command,
        strings(&["cargo", "xtask", "live", "paste", "--", "--help", "--build", "-x"])
    );
    assert!(!options.build);
}

#[test]
fn the_command_begins_at_the_first_word_that_is_no_option_when_there_is_no_double_dash() {
    let options = run("--host zeewin --build cargo test --build").unwrap();
    assert!(options.build);
    assert_eq!(options.command, strings(&["cargo", "test", "--build"]));
}

#[test]
fn what_is_missing_or_wrong_is_refused_in_words() {
    for (line, said) in [
        (
            "--host zeewin",
            "windows takes the command to run there after a --",
        ),
        (
            "--host zeewin --",
            "windows takes the command to run there after a --",
        ),
        (
            "--machine zeewin2 --",
            "windows takes the command to run there after a --",
        ),
        (
            "--machine laptop -- x",
            "--machine is one of zeewin2, zeewin, not laptop",
        ),
        (
            "--machine zeewin2 --host zeewin -- x",
            "--machine and --host name the same thing: give one",
        ),
        ("--host= -- x", "--host is an ssh host, not an empty word"),
        (
            "--machine",
            "--machine takes a value (to start one with a dash: --machine=-value)",
        ),
        (
            "--host",
            "--host takes a value (to start one with a dash: --host=-value)",
        ),
        (
            "--host --build -- x",
            "--host takes a value (to start one with a dash: --host=-value)",
        ),
        ("--host zeewin --build=1 -- x", "--build takes no value"),
        (
            "--host=-oProxyCommand=x -- cmd",
            "--host is an ssh host, not an option of ssh: -oProxyCommand=x",
        ),
        ("--host zeewin --nope -- x", "unknown option: --nope"),
        ("--host zeewin -x -- x", "unknown option: -x"),
        (
            "--host zeewin --dir a/b -- x",
            "--dir is a folder name: a/b",
        ),
        ("--host zeewin --dir .. -- x", "--dir is a folder name: .."),
        ("--host zeewin --dir . -- x", "--dir is a folder name: ."),
        (
            "--host zeewin --in ../x -- x",
            "--in is a folder of the copy: ../x",
        ),
        (
            "--host zeewin --in a/../b -- x",
            "--in is a folder of the copy: a/../b",
        ),
        (
            "--host zeewin --in a;b -- x",
            "--in is a folder of the copy: a;b",
        ),
        (
            "--host zeewin --env NOVALUE -- x",
            "--env is NAME=VALUE, a variable's name and what to give it: NOVALUE",
        ),
        (
            "--host zeewin --env A=1 --env A=2 -- x",
            "--env A is given twice",
        ),
    ] {
        let refused = run(line).unwrap_err();
        assert!(refused.starts_with(said), "{line}: {refused}");
    }
}

#[cfg(unix)]
#[test]
fn a_word_that_is_no_text_is_refused_not_changed() {
    use std::os::unix::ffi::OsStringExt;

    let mut args = words("--host zeewin --");
    args.push(OsString::from_vec(vec![b'a', 0xff]));
    assert_eq!(
        Options::parse(&args).unwrap_err(),
        "an argument is not UTF-8: a\u{fffd}"
    );
}

fn matrix(line: &str) -> Result<MatrixOptions, String> {
    MatrixOptions::parse(&words(line))
}

#[test]
fn a_matrix_is_a_host_scenarios_and_pairs_each_as_many_as_there_are() {
    let options = matrix(
        "--host zeewin --build --env CONSENSFLOW_TEST_DAEMON=native --scenario round-trip \
         --scenario question-trip --pair devin:devin --pair claude:devin,pi \
         --claude-model claude-sonnet-5 --pi-model openai/gpt-5.6-luna",
    )
    .unwrap();
    assert_eq!(
        options,
        MatrixOptions {
            machine: Some(Machine::at("zeewin")),
            build: true,
            env: vec![("CONSENSFLOW_TEST_DAEMON".into(), "native".into())],
            scenarios: strings(&["round-trip", "question-trip"]),
            pairs: vec![
                Pair {
                    chief: "devin".into(),
                    staff: "devin".into()
                },
                Pair {
                    chief: "claude".into(),
                    staff: "devin,pi".into()
                },
            ],
            claude_model: Some("claude-sonnet-5".into()),
            pi_model: Some("openai/gpt-5.6-luna".into()),
        }
    );
}

#[test]
fn a_matrix_that_is_missing_what_it_needs_or_names_a_pair_wrongly_is_refused_before_a_run() {
    let usage = "windows-matrix takes a --scenario <name> and a --pair <chief>:<staff>";
    for (line, said) in [
        ("--host h --pair a:b", usage),
        (
            "--machine zeewin3 --scenario s --pair a:b",
            "--machine is one of zeewin2, zeewin, not zeewin3",
        ),
        ("--host h --scenario s", usage),
        (
            "--host h --scenario s --pair devin",
            "a pair is chief:staff, not devin",
        ),
        (
            "--host h --scenario s --pair :devin",
            "a pair is chief:staff, not :devin",
        ),
        (
            "--host h --scenario s --pair devin:",
            "a pair is chief:staff, not devin:",
        ),
        (
            "--host h --scenario s --pair a:b stray",
            "unexpected argument: stray",
        ),
        (
            "--host h --scenario s --pair a:b --dir x",
            "unknown option: --dir",
        ),
        (
            "--host=-oProxyCommand=x --scenario s --pair a:b",
            "--host is an ssh host, not an option of ssh: -oProxyCommand=x",
        ),
        (
            "--host h --scenario s --pair a:b --env X",
            "--env is NAME=VALUE, a variable's name and what to give it: X",
        ),
    ] {
        let refused = matrix(line).unwrap_err();
        assert!(refused.starts_with(said), "{line}: {refused}");
    }
}
