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
fn a_windows_wrapper_is_a_cmd_in_the_shape_of_an_npm_shim_that_names_node_and_the_script_it_runs() {
    let shim = cmd(
        Path::new(r"C:\Program Files\nodejs\node.exe"),
        Path::new(r"C:\Users\u\bin\claude.mjs"),
    );
    assert_eq!(
        shim,
        "@echo off\r\nREM Written by cf-evals: eval windows reach no MCP server and no browser.\r\n\
         \"C:\\Program Files\\nodejs\\node.exe\" \"C:\\Users\\u\\bin\\claude.mjs\" %*\r\n"
    );
    // The daemon reads the last line that passes the arguments on, and takes the last
    // two words it quotes there for the program and the script: the comment is no such line.
    let line = shim.lines().rev().find(|line| line.contains("%*")).unwrap();
    assert_eq!(line.matches('"').count(), 4);
    assert!(!shim.lines().next().unwrap_or_default().contains("%*"));
}

#[test]
fn the_script_a_windows_wrapper_runs_starts_the_real_program_and_ends_as_it_ends() {
    let text = relay(&strings(&[
        "C:\\node.exe",
        "C:\\cli.js",
        "--no-chrome",
        "-c",
        "model=\"m\"",
    ]));
    assert!(
        text.contains(
            "const [file, ...args] = [\"C:\\\\node.exe\",\"C:\\\\cli.js\",\"--no-chrome\",\"-c\",\"model=\\\"m\\\"\"]"
        ),
        "{text}"
    );
    for part in [
        "import { spawn } from 'node:child_process'",
        "for (const signal of ['SIGINT', 'SIGBREAK']) process.on(signal, () => {})",
        "spawn(file, [...args, ...process.argv.slice(2)], { stdio: 'inherit' })",
        "child.on('exit', (code) => process.exit(code ?? 1))",
    ] {
        assert!(text.contains(part), "{part}");
    }
    assert!(text.ends_with('\n'));
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
