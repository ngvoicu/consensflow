//! The machine a run is on: where its folders are, what its windows find on their
//! `PATH`, and the environment the rig gives them.

use std::path::Path;

use super::*;
use crate::plan::chief_environment;

fn options(line: &str) -> Options {
    let words: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
    Options::of(&words).unwrap()
}

/// A machine whose home is `/home/u` and whose harnesses are at `programs`.
fn machine(programs: &[(Harness, &str)], node: Option<&str>) -> Machine {
    Machine {
        home: PathBuf::from("/home/u"),
        installs: programs
            .iter()
            .map(|(harness, program)| Installed::at(*harness, *program, "/home/u"))
            .collect(),
        node: node.map(PathBuf::from),
    }
}

fn folders(machine: &Machine) -> Vec<PathBuf> {
    std::env::split_paths(&machine.path()).collect()
}

#[test]
fn a_run_works_in_folders_of_its_own_below_the_users_home() {
    let machine = machine(&[], None);
    let below = Path::new("/home/u")
        .join(".consensflow-candidate")
        .join("evals");
    assert_eq!(machine.workspace(), below.join("workspace"));
    assert_eq!(machine.bin(), below.join("bin"));
}

#[test]
fn the_wrappers_are_first_on_the_path_then_the_harnesses_and_node_and_no_folder_twice() {
    let machine = machine(
        &[
            (Harness::Claude, "/home/u/.local/bin/claude"),
            (Harness::Codex, "/opt/homebrew/bin/codex"),
            (Harness::Pi, "/home/u/.local/bin/pi"),
        ],
        Some("/usr/local/bin"),
    );
    let folders = folders(&machine);
    assert_eq!(folders[0], machine.bin());
    assert_eq!(folders[1], Path::new("/home/u/.local/bin"));
    assert_eq!(folders[2], Path::new("/opt/homebrew/bin"));
    for found in ["/home/u/.local/bin", "/opt/homebrew/bin", "/usr/local/bin"] {
        assert_eq!(
            folders
                .iter()
                .filter(|folder| *folder == Path::new(found))
                .count(),
            1,
            "{found}: {folders:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn the_rest_of_the_path_on_the_mac_is_the_systems_and_never_this_shells() {
    let machine = machine(&[(Harness::Claude, "/home/u/.local/bin/claude")], None);
    let folders = folders(&machine);
    assert_eq!(
        folders[2..],
        [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin"
        ]
        .map(PathBuf::from)
    );
}

#[test]
fn a_harness_of_the_run_is_found_by_its_name_and_one_that_is_not_in_it_is_an_error_that_says_so() {
    let machine = machine(&[(Harness::Codex, "/opt/homebrew/bin/codex")], None);
    assert_eq!(
        machine.install("codex").unwrap().command(),
        Path::new("/opt/homebrew/bin/codex")
    );
    assert_eq!(
        machine.install("claude").unwrap_err().to_string(),
        "claude is no harness of this run"
    );
    assert!(machine.install("kimi").is_err());
}

#[test]
fn a_harness_reads_its_login_from_the_users_home_and_codex_from_its_own_folder() {
    let home = Path::new("/home/u");
    assert_eq!(login_of(Harness::Claude, home), home);
    assert_eq!(login_of(Harness::Pi, home), home);
    // Codex's own folder: `.codex` in the home, unless the machine names another.
    let codex = login_of(Harness::Codex, home);
    assert!(
        codex == home.join(".codex") || own_var("CODEX_HOME").is_some(),
        "{codex:?}"
    );
}

#[test]
fn the_rig_a_run_is_on_has_the_users_own_home_the_wrappers_first_and_no_claude_config_folder() {
    let machine = machine(&[(Harness::Claude, "/home/u/.local/bin/claude")], None);
    let options = options("--arm nocard");
    let chief = chief_environment("claude", None).unwrap();
    let config = machine.rig(
        &options,
        &chief,
        Path::new("/home/u/.consensflow-candidate/evals/bin/claude"),
    );
    let said = format!("{config:?}");
    for part in [
        r#"("HOME", "/home/u")"#,
        r#"("ANTHROPIC_MODEL", "claude-opus-5")"#,
        "CONSENSFLOW_EVAL_CHIEF_CARD",
        "no-card.md",
        r#"unset: ["CLAUDE_CONFIG_DIR"#,
        &format!("(\"PATH\", \"{}\")", machine.path().replace('\\', "\\\\")),
    ] {
        assert!(said.contains(part), "{part}\n{said}");
    }
    // The arm with a card has no other card than the product's own.
    let plain = machine.rig(
        &super::super::options::Options::of(&[]).unwrap(),
        &chief,
        Path::new("claude"),
    );
    assert!(!format!("{plain:?}").contains("CONSENSFLOW_EVAL_CHIEF_CARD"));
}
