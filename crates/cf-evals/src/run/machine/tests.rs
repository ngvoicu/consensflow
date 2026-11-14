//! The machine a run is on: where its folders are, what its windows find on their
//! `PATH`, and the environment the rig gives them.

use std::path::Path;
use std::time::Duration;

use cf_e2e::rig::ClaudeConfig;

use super::super::options::Options;
use super::*;
use crate::plan::chief_environment;

/// The config folder of a run, which is made nowhere: a path.
#[derive(Debug)]
struct Folder;

impl ClaudeConfig for Folder {
    fn folder(&self) -> Support<PathBuf> {
        Ok(PathBuf::from("/tmp/consensflow-live-claude-1-run"))
    }

    fn described(&self) -> String {
        "the test".to_owned()
    }

    fn finish(&self) -> Support {
        Ok(())
    }
}

/// A config folder whose run touched the owner's settings.
#[derive(Debug)]
struct Touches;

impl ClaudeConfig for Touches {
    fn folder(&self) -> Support<PathBuf> {
        Ok(PathBuf::from("/tmp/consensflow-live-claude-1-touched"))
    }

    fn described(&self) -> String {
        "the test".to_owned()
    }

    fn finish(&self) -> Support {
        Err(cf_e2e::Error::Daemon(
            "\n!!!\nTHE RUN CHANGED THE OWNER'S CLAUDE SETUP:\n  - ~/.claude/settings.json is not as it was\n!!!".to_owned(),
        ))
    }
}

/// `machine` as a run with a throwaway login is on: Claude Code's windows run on its folder.
fn with_login(mut machine: Machine) -> Machine {
    let login = Arc::new(Throwaway::over(Folder));
    machine.installs = machine
        .installs
        .iter()
        .map(|install| {
            installed(
                install.harness(),
                install.command().to_path_buf(),
                &machine.home,
                Some(&login),
                &stand_ins(),
            )
            .unwrap()
        })
        .collect();
    machine.login = Some(login);
    machine
}

/// A run whose every window is the stand-in agent's: no harness of it reads a login.
fn stand_ins() -> Seated {
    Seated {
        windows: Vec::new(),
        planned: Duration::from_secs(3600),
    }
}

/// A run of an hour with a real window of each of `harnesses`, Pi's on an OpenRouter model.
fn real(harnesses: &[Harness]) -> Seated {
    Seated {
        windows: harnesses
            .iter()
            .map(|harness| {
                let model = if *harness == Harness::Pi {
                    "openrouter/x/y"
                } else {
                    "m"
                };
                (*harness, model.to_owned())
            })
            .collect(),
        planned: Duration::from_secs(3600),
    }
}

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
        login: None,
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
    assert_eq!(machine.workspaces(), below.join("workspaces"));
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
fn only_claude_code_runs_on_the_throwaway_login_of_a_run() {
    let login = Arc::new(Throwaway::over(Folder));
    let home = Path::new("/home/u");
    for harness in [Harness::Claude, Harness::Codex, Harness::Pi] {
        let install = installed(
            harness,
            PathBuf::from("/bin/x"),
            home,
            Some(&login),
            &stand_ins(),
        )
        .unwrap();
        assert_eq!(
            install.claude_config().is_some(),
            harness == Harness::Claude,
            "{harness:?}"
        );
        assert!(
            installed(harness, PathBuf::from("/bin/x"), home, None, &stand_ins())
                .unwrap()
                .claude_config()
                .is_none()
        );
    }
}

#[test]
fn a_run_with_a_throwaway_login_hands_its_folder_to_the_rig_and_clears_no_variable() {
    let machine = with_login(machine(
        &[(Harness::Claude, "/home/u/.local/bin/claude")],
        None,
    ));
    let chief = chief_environment("claude", None).unwrap();
    let config = machine.rig(
        &options("").crew(),
        &chief,
        Path::new("/home/u/.consensflow-candidate/evals/bin/claude"),
    );
    let said = format!("{config:?}");
    // The rig asks the folder for the windows and the daemon; a variable the run clears is not
    // there at all, and would undo it.
    assert!(said.contains("claude_config: Some(Throwaway"), "{said}");
    assert!(!said.contains("CLAUDE_CONFIG_DIR"), "{said}");
    assert!(said.contains(r#"("HOME", "/home/u")"#), "{said}");
}

#[test]
fn the_claude_config_folder_of_a_run_is_its_throwaway_the_users_own_without_one_and_none_without_claude(
) {
    let claude = [(Harness::Claude, "/home/u/.local/bin/claude")];
    let with = with_login(machine(&claude, None));
    assert_eq!(
        with.claude_config().unwrap(),
        Some(PathBuf::from("/tmp/consensflow-live-claude-1-run"))
    );
    // A run on the stand-ins, or on Windows: Claude Code is as it was.
    assert_eq!(
        machine(&claude, None).claude_config().unwrap(),
        Some(PathBuf::from("/home/u/.claude"))
    );
    // A run with no Claude Code: nothing of the user's `.claude` is the run's to clear.
    let codex = [(Harness::Codex, "/home/u/.local/bin/codex")];
    assert_eq!(machine(&codex, None).claude_config().unwrap(), None);
    assert_eq!(
        with_login(machine(&codex, None)).claude_config().unwrap(),
        None
    );
}

#[test]
fn a_runs_claude_records_are_in_its_throwaway_or_in_the_users_claude_when_it_has_none() {
    let claude = [(Harness::Claude, "/home/u/.local/bin/claude")];
    assert_eq!(
        with_login(machine(&claude, None)).claude_records().unwrap(),
        PathBuf::from("/tmp/consensflow-live-claude-1-run")
    );
    assert_eq!(
        machine(&claude, None).claude_records().unwrap(),
        PathBuf::from("/home/u/.claude")
    );
    // Without Claude Code there is nothing to read in them.
    let codex = [(Harness::Codex, "/home/u/.local/bin/codex")];
    assert_eq!(
        with_login(machine(&codex, None)).claude_records().unwrap(),
        PathBuf::from("/home/u/.claude")
    );
}

#[test]
fn what_a_run_did_to_the_owners_claude_is_what_its_login_found() {
    let claude = [(Harness::Claude, "/home/u/.local/bin/claude")];
    assert_eq!(machine(&claude, None).homes_findings(), None);
    assert_eq!(with_login(machine(&claude, None)).homes_findings(), None);
    let mut touched = with_login(machine(&claude, None));
    let login = Arc::new(Throwaway::over(Touches));
    login.finish().unwrap();
    touched.login = Some(login);
    assert_eq!(
        touched.homes_findings().as_deref(),
        Some("~/.claude/settings.json is not as it was")
    );
}

#[test]
fn the_rig_a_run_without_a_login_is_on_has_the_users_own_home_the_wrappers_first_and_no_claude_config_folder(
) {
    let machine = machine(&[(Harness::Claude, "/home/u/.local/bin/claude")], None);
    let options = options("--arm nocard");
    let chief = chief_environment("claude", None).unwrap();
    let config = machine.rig(
        &options.crew(),
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
        &super::super::options::Options::of(&[]).unwrap().crew(),
        &chief,
        Path::new("claude"),
    );
    assert!(!format!("{plain:?}").contains("CONSENSFLOW_EVAL_CHIEF_CARD"));
}

/// Before T-785 the evals built their installs bare, so an eval's Pi, Codex and Devin windows ran
/// for hours on the owner's own login folders, and the rig gave every window the owner's
/// `CODEX_HOME`. Every harness with a real window of the run runs on the kit's home now, kept for
/// the report; one whose every window is the stand-in reads no login and gets none.
#[test]
fn an_eval_machines_real_windows_run_on_the_kits_homes_and_its_rig_names_no_codex_home_of_the_owners(
) {
    let home = Path::new("/home/u");
    let others: Vec<Harness> = if cfg!(unix) {
        vec![Harness::Codex, Harness::Pi, Harness::Devin, Harness::Muse]
    } else {
        // Devin's and Muse's folders on Windows are not known to the kit (`live::isolation`).
        vec![Harness::Codex, Harness::Pi]
    };
    let seated = real(&others);
    let installs: Vec<Installed> = others
        .iter()
        .map(|harness| installed(*harness, PathBuf::from("/bin/x"), home, None, &seated).unwrap())
        .collect();
    for install in &installs {
        let isolation = install
            .isolation()
            .unwrap_or_else(|| panic!("{:?} runs on the owner's login folders", install.harness()));
        assert_eq!(isolation.harness(), install.harness());
        // Its end keeps what it finds for the report, and fails no rig.
        assert_eq!(isolation.found(), Some(Default::default()));
    }
    // The stand-in's windows (the dry runs) read no login: none is copied for them.
    for harness in &others {
        let bare = installed(*harness, PathBuf::from("/bin/x"), home, None, &stand_ins()).unwrap();
        assert!(bare.isolation().is_none(), "{harness:?}");
    }
    // The rig sets each home's variables last. (Muse Code is no eval harness yet, T-1026; its
    // rig runs it through a launcher of the kit's own, which needs a Muse installed.)
    let seats: Vec<Harness> = others
        .iter()
        .copied()
        .filter(|harness| *harness != Harness::Muse)
        .collect();
    let machine = Machine {
        installs: installs
            .into_iter()
            .filter(|install| seats.contains(&install.harness()))
            .collect(),
        ..machine(&[], None)
    };
    let chief = chief_environment("codex", None).unwrap();
    let config = machine.rig(
        &options("--chief codex").crew(),
        &chief,
        Path::new("claude"),
    );
    let said = format!("{config:?}");
    for harness in &seats {
        let name = format!("{harness:?}");
        assert!(
            said.contains(&format!("Isolation {{ harness: {name}")),
            "{name}: {said}"
        );
    }
    assert!(!said.contains("\"CODEX_HOME\""), "{said}");
    assert!(said.contains(r#"("HOME", "/home/u")"#), "{said}");
}

/// An eval's Pi home holds the login of its own windows' model (T-785: not the live kit's asked
/// model), for the run's planned length and the evals' 30 minutes, once: a login that lasts an hour
/// and 45 minutes fits a run of an hour, and is refused for one of an hour and a half.
#[test]
fn an_eval_pi_home_holds_the_login_its_models_need_for_the_planned_run_and_its_half_hour() {
    let owner = tempfile::tempdir().unwrap();
    let agent = owner.path().join(".pi").join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    let expires = jiff::Timestamp::now().as_millisecond() + 105 * 60 * 1000;
    std::fs::write(
        agent.join("auth.json"),
        format!(
            r#"{{"openrouter": {{"type": "api_key", "key": "k"}},
                "openai": {{"type": "oauth", "access": "a", "refresh": "r", "expires": {expires}}}}}"#
        ),
    )
    .unwrap();
    // The kit honours PI_CODING_AGENT_DIR from the caller. Run this assertion in a separate
    // process whose Pi folder is the fixture, without changing other tests' environment.
    let ran = cf_e2e::process::case_of(
        std::env::current_exe().unwrap(),
        "run::machine::tests::pi_fixture_checks_models_and_margins",
        Duration::from_secs(30),
    )
    .args(["--ignored", "--nocapture"])
    .inheriting_env()
    .without("CONSENSFLOW_TEST_MACHINE")
    .var("CF_EVALS_TEST_PI_HOME", owner.path())
    .var("PI_CODING_AGENT_DIR", &agent)
    .run()
    .unwrap();
    assert_eq!(ran.code, Some(0), "{ran}");
    assert!(ran.stdout.contains("1 passed;"), "{ran}");
}

/// The original assertions, run by the parent with its own synthetic Pi folder.
#[test]
#[ignore = "fixture subprocess run by an_eval_pi_home_holds_the_login_its_models_need_for_the_planned_run_and_its_half_hour"]
#[allow(clippy::disallowed_methods)] // The child reads the fixture path its parent gave it.
fn pi_fixture_checks_models_and_margins() {
    use cf_e2e::rig::HarnessHome;
    let owner = PathBuf::from(std::env::var_os("CF_EVALS_TEST_PI_HOME").unwrap());
    let on = |minutes: u64| Seated {
        windows: vec![(Harness::Pi, "openai/gpt-6-luna".to_owned())],
        planned: Duration::from_secs(minutes * 60),
    };
    let fits = isolation(Harness::Pi, &owner, &on(60)).unwrap().unwrap();
    let vars = fits.vars().unwrap();
    let (name, folder) = &vars[0];
    assert_eq!(name, "PI_CODING_AGENT_DIR");
    let copied = std::fs::read_to_string(Path::new(folder).join("auth.json")).unwrap();
    assert!(
        copied.contains("openai") && !copied.contains("openrouter"),
        "{copied}"
    );
    // The eval's margin, not the kit's on top of it: a run of 70 minutes fits the login (70 and
    // 30 minutes), which the kit's 10 more would not.
    let seventy = isolation(Harness::Pi, &owner, &on(70)).unwrap().unwrap();
    assert!(
        seventy.vars().is_ok(),
        "70 minutes and the evals' half hour fit"
    );
    let longer = isolation(Harness::Pi, &owner, &on(90)).unwrap().unwrap();
    let said = longer.vars().unwrap_err().to_string();
    assert!(said.contains("`openai` expires in 1 h 4"), "{said}");
}
