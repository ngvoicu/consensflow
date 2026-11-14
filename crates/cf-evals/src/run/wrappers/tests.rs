//! The programs a run's windows are started through: what they add, what they say,
//! and that the shell and the daemon read them as they are meant.

#[cfg(unix)]
use cf_e2e::live::{Harness, Installed};
use serde_json::json;

use super::super::options::Options;
use super::*;
use crate::plan::chief_environment;

/// Who the windows of a run of `line` are.
fn options(line: &str) -> Crew {
    let words: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
    Options::of(&words).unwrap().crew()
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn claude_is_started_with_no_mcp_server_and_no_browser_and_the_chiefs_effort_when_it_is_the_chief()
{
    assert_eq!(
        claude_flags(&options("--chief claude")),
        strings(&["--strict-mcp-config", "--no-chrome", "--effort", "high"])
    );
    assert_eq!(
        claude_flags(&options("--chief claude --effort low")),
        strings(&["--strict-mcp-config", "--no-chrome", "--effort", "low"])
    );
    // A Claude member of another chief's staff has its own effort, in its roster.
    assert_eq!(
        claude_flags(&options("--chief codex --staff claude")),
        strings(&["--strict-mcp-config", "--no-chrome"])
    );
}

#[test]
fn codex_is_started_with_its_model_the_chiefs_effort_and_every_mcp_server_switched_off() {
    let servers = json!([
        { "name": "cua_repl", "transport": { "type": "stdio", "command": "cua" } },
        { "name": "idea", "transport": { "type": "streamable_http", "url": "http://127.0.0.1:64342/stream" } },
    ]);
    let chief = options("--chief codex");
    let setup = chief_environment("codex", None).unwrap();
    assert_eq!(
        codex_flags(&chief, &setup, &servers).unwrap(),
        strings(&[
            "-c",
            "model=\"gpt-5.6-luna\"",
            "-c",
            "model_reasoning_effort=\"high\"",
            "-c",
            "mcp_servers.cua_repl.command=\"/usr/bin/true\"",
            "-c",
            "mcp_servers.cua_repl.enabled=false",
            "-c",
            "mcp_servers.idea.url=\"http://127.0.0.1:9/disabled\"",
            "-c",
            "mcp_servers.idea.enabled=false",
        ])
    );
    // A chief named another model is on it; a member of another chief's staff on the cheap one.
    let named = chief_environment("codex", Some("gpt-x")).unwrap();
    assert_eq!(
        codex_flags(&chief, &named, &json!([])).unwrap()[1],
        "model=\"gpt-x\""
    );
    let member = options("--chief claude --staff codex");
    let claude = chief_environment("claude", None).unwrap();
    assert_eq!(
        codex_flags(&member, &claude, &json!([])).unwrap(),
        strings(&["-c", "model=\"gpt-5.6-luna\""]),
        "no effort of the chief's, and not the chief's model"
    );
    // A server whose name Codex would read as more than one key is never half switched off.
    let refused = codex_flags(&chief, &setup, &json!([{ "name": "a.b" }])).unwrap_err();
    assert!(
        refused.to_string().contains("cannot switch off"),
        "{refused}"
    );
}

#[test]
fn a_shell_wrapper_starts_the_real_program_with_the_flags_before_every_word_it_is_given() {
    assert_eq!(
        script(
            "/home/me/.local/bin/claude",
            &strings(&["--no-chrome", "-c", "a b"])
        ),
        "#!/bin/sh\n# Written by cf-evals: eval windows reach no MCP server and no browser.\n\
         exec '/home/me/.local/bin/claude' '--no-chrome' '-c' 'a b' \"$@\"\n"
    );
    // A word with a quote in it is still one word.
    assert_eq!(shell_word("it's"), r"'it'\''s'");
    assert!(script("/it's/claude", &[]).contains(r"exec '/it'\''s/claude' "));
}

/// Runs the script `text` (written to a file in `folder`) by the shell with `words`:
/// what it printed.
#[cfg(unix)]
fn run(folder: &tempfile::TempDir, text: &str, words: &[&str]) -> String {
    let file = folder.path().join("wrapper");
    cf_e2e::files::write_executable(&file, text).unwrap();
    // Run by the shell: a script just written and started as a program may be
    // refused as a file still open for writing, when another test starts one.
    cf_e2e::process::Run::new("/bin/sh")
        .arg(&file)
        .args(words)
        .run()
        .unwrap()
        .stdout
}

#[cfg(unix)]
#[test]
fn the_wrapper_is_a_program_that_runs_the_real_one_with_the_flags_first_and_its_words_kept() {
    let folder = tempfile::tempdir().unwrap();
    let text = script(
        "/bin/echo",
        &strings(&["--strict-mcp-config", "--no-chrome"]),
    );
    assert_eq!(
        run(&folder, &text, &["--settings", "a b", "--model", "m"]),
        "--strict-mcp-config --no-chrome --settings a b --model m\n"
    );
}

#[cfg(unix)]
#[test]
fn the_pi_wrapper_gives_a_window_its_model_and_thinking_where_it_names_none_and_a_subcommand_nothing(
) {
    let folder = tempfile::tempdir().unwrap();
    let text = pi_script("/bin/echo", "openrouter/x", "high");
    // A window: both, in front.
    assert_eq!(
        run(&folder, &text, &["--session", "s-1"]),
        "--thinking high --model openrouter/x --session s-1\n"
    );
    // A member's roster agent names its own: it is not repeated.
    assert_eq!(
        run(&folder, &text, &["--model", "own", "--thinking", "low"]),
        "--model own --thinking low\n"
    );
    assert_eq!(
        run(&folder, &text, &["--model", "own"]),
        "--thinking high --model own\n"
    );
    // Pi's own subcommands take neither.
    assert_eq!(run(&folder, &text, &["install", "x"]), "install x\n");
    assert_eq!(
        run(&folder, &text, &[]),
        "--thinking high --model openrouter/x\n"
    );
}

#[test]
fn a_wrapper_is_found_by_the_command_it_stands_for() {
    let wrappers = Wrappers {
        claude: Some(PathBuf::from("bin/claude")),
        codex: None,
        pi: Some(PathBuf::from("bin/pi")),
    };
    assert_eq!(wrappers.of("claude"), Some(Path::new("bin/claude")));
    assert_eq!(wrappers.of("codex"), None);
    assert_eq!(wrappers.of("pi"), Some(Path::new("bin/pi")));
    assert_eq!(wrappers.of("devin"), None);
}

/// A machine whose home is `home` and whose harnesses are `programs` (a harness and
/// the program that stands for it).
#[cfg(unix)]
fn machine(home: &Path, programs: &[(Harness, &Path)]) -> Machine {
    Machine {
        home: home.to_path_buf(),
        installs: programs
            .iter()
            .map(|(harness, program)| Installed::at(*harness, *program, home))
            .collect(),
        node: None,
        login: None,
    }
}

#[cfg(unix)]
#[test]
fn a_run_writes_the_wrappers_of_the_harnesses_in_it_and_no_others_and_each_run_begins_afresh() {
    let home = tempfile::tempdir().unwrap();
    let codex = home.path().join("codex-real");
    cf_e2e::files::write_executable(&codex, "#!/bin/sh\necho '[{\"name\":\"cua_repl\"}]'\n")
        .unwrap();
    let machine = machine(
        home.path(),
        &[
            (Harness::Claude, Path::new("/bin/echo")),
            (Harness::Codex, &codex),
        ],
    );
    // A wrapper of an earlier run, for a chief that is not this run's.
    let stale = machine.bin().join("pi");
    cf_e2e::files::write_executable(&stale, "#!/bin/sh\n").unwrap();

    let run_options = options("--chief claude --staff claude,codex");
    let chief = chief_environment("claude", None).unwrap();
    let wrappers = write(&machine, &run_options, &chief).unwrap();
    assert_eq!(wrappers.claude, Some(machine.bin().join("claude")));
    assert_eq!(wrappers.codex, Some(machine.bin().join("codex")));
    assert_eq!(wrappers.pi, None);
    assert!(
        !stale.exists(),
        "a wrapper written for another chief must not outlive its run"
    );
    let claude = cf_e2e::files::read_string(&machine.bin().join("claude")).unwrap();
    assert!(
        claude.contains(
            "exec '/bin/echo' '--strict-mcp-config' '--no-chrome' '--effort' 'high' \"$@\""
        ),
        "{claude}"
    );
    let codex_text = cf_e2e::files::read_string(&machine.bin().join("codex")).unwrap();
    assert!(
        codex_text.contains(
            "'-c' 'model=\"gpt-5.6-luna\"' '-c' 'mcp_servers.cua_repl.command=\"/usr/bin/true\"'"
        ),
        "{codex_text}"
    );
    assert_eq!(
        wrappers.of("claude").map(Path::to_path_buf),
        wrappers.claude
    );
}

#[cfg(unix)]
#[test]
fn a_pi_chief_gets_the_pi_wrapper_with_its_model_and_effort_and_a_claude_staff_the_claude_one() {
    let home = tempfile::tempdir().unwrap();
    let machine = machine(
        home.path(),
        &[
            (Harness::Pi, Path::new("/bin/echo")),
            (Harness::Claude, Path::new("/bin/echo")),
        ],
    );
    let run_options = options("--chief pi --staff claude --effort medium");
    let chief = chief_environment("pi", None).unwrap();
    let wrappers = write(&machine, &run_options, &chief).unwrap();
    let pi = cf_e2e::files::read_string(wrappers.pi.as_deref().unwrap()).unwrap();
    assert!(
        pi.contains("--model 'openrouter/deepseek/deepseek-v4-pro-0813'")
            && pi.contains("--thinking 'medium'"),
        "{pi}"
    );
    // The chief is not Claude: the flags of the wrapper do not carry its effort.
    let claude = cf_e2e::files::read_string(wrappers.claude.as_deref().unwrap()).unwrap();
    assert!(!claude.contains("--effort"), "{claude}");
}

#[test]
fn a_harness_that_is_not_in_the_run_is_an_error_that_names_it() {
    let home = tempfile::tempdir().unwrap();
    let machine = Machine {
        home: home.path().to_path_buf(),
        installs: Vec::new(),
        node: None,
        login: None,
    };
    let chief = chief_environment("claude", None).unwrap();
    let failed = write(&machine, &options("--chief claude"), &chief).unwrap_err();
    assert!(
        failed
            .to_string()
            .contains("claude is no harness of this run"),
        "{failed}"
    );
}

/// The stand-in members of a run of `review-loop` on `machine`, played by `agent`.
#[cfg(unix)]
fn stand_ins_for(machine: &Machine, agent: &Path) -> StandIns {
    let scenario = crate::scenarios::by_id("review-loop").unwrap();
    StandIns::of(machine, &scenario, agent).unwrap()
}

/// What the wrapper at `wrapper` does with `words`: the code it exits with, and what it prints.
#[cfg(unix)]
fn ran(wrapper: &Path, words: &[&str]) -> (Option<i32>, String, String) {
    let ran = cf_e2e::process::Run::new("/bin/sh")
        .arg(wrapper)
        .args(words)
        .run()
        .unwrap();
    (ran.code, ran.stdout, ran.stderr)
}

/// A run's machine with a real Claude that echoes its words, a stand-in agent that writes the folder
/// it was given and its words to `played`, and the claude wrapper written for the stand-in members.
#[cfg(unix)]
fn with_stand_ins(home: &Path) -> (Machine, StandIns, PathBuf, PathBuf) {
    let machine = machine(home, &[(Harness::Claude, Path::new("/bin/echo"))]);
    let played = home.join("played");
    let agent = home.join("fake-agent");
    cf_e2e::files::write_executable(
        &agent,
        format!(
            "#!/bin/sh\nprintf '%s|%s\\n' \"$CF_TEST_SCRIPTED_RESULTS\" \"$*\" > '{}'\n",
            played.display()
        ),
    )
    .unwrap();
    let stand_ins = stand_ins_for(&machine, &agent);
    let run_options = options("--chief claude --staff claude");
    let chief = chief_environment("claude", None).unwrap();
    let wrappers = write_with(&machine, &run_options, &chief, Some(&stand_ins)).unwrap();
    (machine, stand_ins, wrappers.claude.unwrap(), played)
}

/// The two models of the stand-in members, with the folder of each in `stand_ins`.
#[cfg(unix)]
fn members(stand_ins: &StandIns) -> [(&'static str, PathBuf); 2] {
    [
        (
            "scripted-reviewer",
            stand_ins.reviewer_folder().unwrap().to_path_buf(),
        ),
        (
            "scripted-worker",
            stand_ins.worker_folder().unwrap().to_path_buf(),
        ),
    ]
}

#[cfg(unix)]
#[test]
fn a_window_on_the_stand_in_model_is_the_stand_in_agent_with_this_runs_folder_and_its_words() {
    let home = tempfile::tempdir().unwrap();
    let (_machine, stand_ins, wrapper, played) = with_stand_ins(home.path());
    stand_ins.restart().unwrap();
    for (model, folder) in members(&stand_ins) {
        let (code, stdout, stderr) = ran(
            &wrapper,
            &[
                "--session-id",
                "s-1",
                "--model",
                model,
                "--effort",
                "medium",
                "the brief",
            ],
        );
        assert_eq!((code, stdout.as_str(), stderr.as_str()), (Some(0), "", ""));
        assert_eq!(
            cf_e2e::files::read_string(&played).unwrap(),
            format!(
                "{}|--session-id s-1 --model {model} --effort medium the brief\n",
                folder.display()
            ),
            "the stand-in, with the folder of its own and the words as the daemon gave them: none of the wrapper's flags"
        );
    }
    let folders = members(&stand_ins);
    assert_ne!(folders[0].1, folders[1].1, "one folder for each");
}

/// The variable a window of Claude Code is given its config folder in: a test's window here, not
/// an eval's choice (the scan of `tests/claude_config.rs` is for the evals' own code).
#[cfg(unix)]
const WINDOWS_CONFIG: &str = concat!("CLAUDE_CONFIG", "_DIR");

/// The folders a stand-in is handed to write below (`CF_LIVE_MADE_ROOTS`), as the wrapper `wrapper`
/// hands them to a window on `model` that runs in `project` with the Claude config `config`. The
/// kit's own program start adds the folders this test process made through the door after them.
#[cfg(unix)]
fn roots_handed(
    wrapper: &Path,
    agent: &Path,
    model: &str,
    project: &Path,
    config: Option<&Path>,
) -> String {
    let seen = project.join("roots-seen");
    cf_e2e::files::write_executable(
        agent,
        format!(
            "#!/bin/sh\nprintf '%s' \"$CF_LIVE_MADE_ROOTS\" > '{}'\n",
            seen.display()
        ),
    )
    .unwrap();
    let mut window = cf_e2e::process::Run::new("/bin/sh")
        .arg(wrapper)
        .args(["--model", model])
        .cwd(project);
    if let Some(config) = config {
        window = window.var(WINDOWS_CONFIG, config);
    }
    let ran = window.run().unwrap();
    assert_eq!(ran.code, Some(0), "{ran}");
    cf_e2e::files::read_string(&seen).unwrap()
}

#[cfg(unix)]
#[test]
fn the_stand_in_is_handed_its_answers_its_config_and_its_project_and_after_them_what_the_window_was_handed(
) {
    // A stand-in the door did not start writes only below what it is handed: the run's folder of
    // answers (its journal, and a worker's files), the Claude config its window runs on (its
    // transcript and status), and the project the window is in. Anything else is not its to write.
    let home = tempfile::tempdir().unwrap();
    let (_machine, stand_ins, wrapper, _played) = with_stand_ins(home.path());
    stand_ins.restart().unwrap();
    let project = home.path().join("project");
    let config = home.path().join("config");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    // The window's `$PWD` is where it is, by its real path.
    let project = std::fs::canonicalize(&project).unwrap();
    let agent = home.path().join("fake-agent");
    // A folder this test process made through the door: the kit's program start hands it to the
    // window, and the wrapper keeps it after its own roots.
    let rigs = cf_e2e::door::made("cf-evals-roots-").unwrap();
    let inherited = std::fs::canonicalize(rigs.path()).unwrap();
    // What the wrapper hands, by its own words, comes first; the folders the window was handed
    // (this process's, among them the one made above) follow, each of its own.
    let handed_first = |model: &str, config: Option<&Path>, expected: String| {
        let roots = roots_handed(&wrapper, &agent, model, &project, config);
        let rest = roots.strip_prefix(&expected).unwrap_or_else(|| {
            panic!("{model}: the stand-in is handed {roots:?}, not {expected:?} first")
        });
        let kept: Vec<&str> = rest.split(':').filter(|root| !root.is_empty()).collect();
        assert!(
            rest.is_empty() || rest.starts_with(':'),
            "{model}: {rest:?} follows {expected:?} as a folder of its own"
        );
        assert!(
            kept.contains(&inherited.to_string_lossy().as_ref()),
            "{model}: what the window was handed ({inherited:?}) is not kept after the run's own: {roots:?}"
        );
    };
    for (model, folder) in members(&stand_ins) {
        handed_first(
            model,
            Some(&config),
            format!(
                "{}:{}:{}",
                folder.display(),
                config.display(),
                project.display()
            ),
        );
        // A window on no config of the run's is handed none.
        handed_first(
            model,
            None,
            format!("{}:{}", folder.display(), project.display()),
        );
    }
}

#[cfg(unix)]
#[test]
fn a_window_on_any_other_model_is_the_real_claude_with_the_flags_as_always() {
    let home = tempfile::tempdir().unwrap();
    let (_machine, stand_ins, wrapper, played) = with_stand_ins(home.path());
    stand_ins.restart().unwrap();
    for model in [
        "claude-haiku-4-5-20251001",
        "scripted-reviewer-2",
        "xscripted-reviewer",
        "scripted-worker-2",
        "xscripted-worker",
    ] {
        let (code, stdout, _) = ran(&wrapper, &["--session-id", "s-1", "--model", model]);
        assert_eq!(code, Some(0));
        assert_eq!(
            stdout,
            format!(
                "--strict-mcp-config --no-chrome --effort high --session-id s-1 --model {model}\n"
            )
        );
    }
    assert!(!played.exists(), "the stand-in never ran");
}

#[cfg(unix)]
#[test]
fn the_wrapper_refuses_the_stand_in_model_outside_an_eval_run_and_runs_neither_program() {
    for role in ["reviewer", "worker"] {
        let home = tempfile::tempdir().unwrap();
        let (_machine, stand_ins, wrapper, played) = with_stand_ins(home.path());
        let model = format!("scripted-{role}");
        let words = [
            "--session-id",
            "s-1",
            "--model",
            model.as_str(),
            "the brief",
        ];
        // Before the run made its folder, and once it has made it: the run's marker is what lets it in.
        let refused = |when: &str| {
            let (code, stdout, stderr) = ran(&wrapper, &words);
            assert_eq!(code, Some(1), "{role}: {when}");
            assert_eq!(stdout, "", "{role}: {when}: the real Claude did not run");
            assert!(
                stderr.contains(&format!(
                    "{model} is the stand-in {role} of an eval run, and this is none: refused"
                )),
                "{role}: {when}: {stderr}"
            );
            assert!(!played.exists(), "{role}: {when}: the stand-in did not run");
        };
        refused("before the run's folder");
        stand_ins.restart().unwrap();
        let folder = members(&stand_ins)
            .into_iter()
            .find(|(has, _)| *has == model)
            .unwrap()
            .1;
        std::fs::remove_file(folder.join(".eval-run")).unwrap();
        refused("a folder with no marker");
        // The run is over: its folders are gone, and the wrapper it left is refused the same way.
        drop(stand_ins);
        refused("after the run");
    }
}

#[cfg(unix)]
#[test]
fn the_wrapper_refuses_the_stand_in_model_when_the_agent_is_not_there() {
    let home = tempfile::tempdir().unwrap();
    let (_machine, stand_ins, wrapper, played) = with_stand_ins(home.path());
    stand_ins.restart().unwrap();
    std::fs::remove_file(home.path().join("fake-agent")).unwrap();
    for (model, _) in members(&stand_ins) {
        let (code, stdout, stderr) = ran(&wrapper, &["--session-id", "s-1", "--model", model]);
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{model}");
        assert!(
            stderr.contains("this is none: refused"),
            "{model}: {stderr}"
        );
    }
    assert!(!played.exists());
}

#[cfg(unix)]
#[test]
fn a_run_with_only_a_stand_in_reviewer_has_a_wrapper_that_knows_no_stand_in_worker() {
    let home = tempfile::tempdir().unwrap();
    let machine = machine(home.path(), &[(Harness::Claude, Path::new("/bin/echo"))]);
    let mut scenario = crate::scenarios::by_id("review-loop").unwrap();
    scenario.stand_in_work.clear();
    let stand_ins = StandIns::of(&machine, &scenario, Path::new("/bin/fake-agent")).unwrap();
    assert!(stand_ins.worker_folder().is_none());
    let chief = chief_environment("claude", None).unwrap();
    let wrappers = write_with(
        &machine,
        &options("--chief claude"),
        &chief,
        Some(&stand_ins),
    )
    .unwrap();
    let text = cf_e2e::files::read_string(wrappers.claude.as_deref().unwrap()).unwrap();
    assert!(
        text.contains("scripted-reviewer") && !text.contains("scripted-worker"),
        "{text}"
    );
    // A window on the worker's model is the real Claude, as any other.
    let (code, stdout, _) = ran(
        wrappers.claude.as_deref().unwrap(),
        &["--model", "scripted-worker"],
    );
    assert_eq!(code, Some(0));
    assert!(stdout.contains("--model scripted-worker"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn a_run_with_no_stand_in_has_a_wrapper_that_knows_no_such_model() {
    let home = tempfile::tempdir().unwrap();
    let machine = machine(home.path(), &[(Harness::Claude, Path::new("/bin/echo"))]);
    let chief = chief_environment("claude", None).unwrap();
    let wrappers = write(&machine, &options("--chief claude"), &chief).unwrap();
    let text = cf_e2e::files::read_string(wrappers.claude.as_deref().unwrap()).unwrap();
    assert!(
        !text.contains("scripted-reviewer")
            && !text.contains("scripted-worker")
            && !text.contains("fake-agent"),
        "{text}"
    );
}

#[cfg(unix)]
#[test]
fn a_stand_in_needs_a_claude_in_the_run_and_a_machine_that_is_not_windows() {
    let home = tempfile::tempdir().unwrap();
    let machine = machine(home.path(), &[(Harness::Pi, Path::new("/bin/echo"))]);
    let stand_ins = stand_ins_for(&machine, Path::new("/bin/fake-agent"));
    let chief = chief_environment("pi", None).unwrap();
    let failed = write_with(
        &machine,
        &options("--chief pi --staff pi"),
        &chief,
        Some(&stand_ins),
    )
    .unwrap_err();
    assert!(
        failed
            .to_string()
            .contains("claude must be the chief or on the staff"),
        "{failed}"
    );
}
