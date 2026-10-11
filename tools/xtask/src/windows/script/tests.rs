//! What a run has the machine run is text, and nothing here reaches a machine:
//! that the variables a run names get to its command, and no others, is read in
//! the script.

use std::fs;
use std::path::Path;

use super::*;
use crate::windows::testing::{script_of, strings};

const DIR: &str = "consensflow-build";
const BUILD: [&str; 2] = ["npm ci --no-audit --no-fund", "npm run build:bridge"];

/// The script of a run in the usual folder, in the copy's root.
fn script(steps: &[&str], command: &str, env: &[(String, String)]) -> String {
    remote_script(&Script {
        dir: DIR,
        inside: ".",
        steps,
        command,
        env,
    })
}

fn plain(env: &[(String, String)]) -> String {
    script(&[], &command_line(&["npm", "run", "test:integration"]), env)
}

/// The lines of a script that give the process a variable of its own, by name
/// (PATH is the script's).
fn given(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("$env:")?;
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            (rest[name.len()..].starts_with(" = ") && name != "Path").then_some(name)
        })
        .collect()
}

#[test]
fn the_variables_a_run_names_are_name_value_each_a_variable_of_its_own_with_the_value_it_is_given()
{
    assert_eq!(parse_env(&[]), Ok(vec![]));
    assert_eq!(
        parse_env(&strings(&["CONSENSFLOW_TEST_DAEMON=native"])),
        Ok(vec![("CONSENSFLOW_TEST_DAEMON".into(), "native".into())])
    );
    // The first = ends the name; the value is the rest, a JSON array's quotes and commas too.
    assert_eq!(
        parse_env(&strings(&[
            "A=b=c",
            r#"B_2=["bin/cf.exe","ui","--json"]"#,
            "_x=",
            "C=it's 100%",
        ])),
        Ok(vec![
            ("A".into(), "b=c".into()),
            ("B_2".into(), r#"["bin/cf.exe","ui","--json"]"#.into()),
            ("_x".into(), String::new()),
            ("C".into(), "it's 100%".into()),
        ])
    );
}

#[test]
fn the_variables_are_refused_when_they_are_not_that_before_anything_is_sent() {
    for entry in ["NOVALUE", "=x", "1A=x", "A-B=x", "A B=x", "É=x", ""] {
        let said = parse_env(&strings(&[entry])).unwrap_err().to_string();
        assert_eq!(
            said,
            format!("--env is NAME=VALUE, a variable's name and what to give it: {entry}"),
            "{entry:?}"
        );
    }
    for value in ["a\nb", "a\rb", "a\tb", "café", "a\0b", "smart ‘quote’"] {
        let said = parse_env(&[format!("A={value}")]).unwrap_err().to_string();
        assert_eq!(
            said, "--env A is not printable ASCII: it could not reach the run as it is",
            "{value:?}"
        );
    }
    let said = parse_env(&strings(&["A=1", "B=2", "A=3"])).unwrap_err();
    assert_eq!(said.to_string(), "--env A is given twice");
}

#[test]
fn a_run_carries_the_variables_it_names_to_its_command_after_the_build_steps_and_before_it() {
    let env = parse_env(&strings(&[
        "CONSENSFLOW_TEST_DAEMON=native",
        "CONSENSFLOW_TEST_CLI=native",
    ]))
    .unwrap();
    let text = script(
        &BUILD,
        &command_line(&["npm", "run", "test:integration"]),
        &env,
    );
    assert_eq!(
        given(&text),
        ["CONSENSFLOW_TEST_DAEMON", "CONSENSFLOW_TEST_CLI"]
    );
    assert!(text.contains("$env:CONSENSFLOW_TEST_DAEMON = 'native'\n"));
    assert!(text.contains("$env:CONSENSFLOW_TEST_CLI = 'native'\n"));
    let at = |what: &str| {
        text.find(what)
            .unwrap_or_else(|| panic!("{what} is not there"))
    };
    assert!(
        at("foreach ($step in") < at("$env:CONSENSFLOW_TEST_DAEMON ="),
        "not before the steps"
    );
    assert!(at("$env:CONSENSFLOW_TEST_CLI =") < at("Set-Location (Join-Path $dir '.')"));
    assert!(
        at("Set-Location (Join-Path $dir '.')") < at("$run = Join-Path"),
        "before the command"
    );
    assert!(at("$env:CONSENSFLOW_TEST_DAEMON =") < at(r#"cmd /c "$run 2>&1""#));
    // The run's own output says what it was given, as the first thing after the build.
    assert!(text
        .contains(r#"Write-Output "== env CONSENSFLOW_TEST_DAEMON=$env:CONSENSFLOW_TEST_DAEMON""#));
}

#[test]
fn a_run_gives_its_command_no_variable_that_was_not_named() {
    let text = script(&BUILD, "npm run test:integration", &[]);
    assert_eq!(given(&text), Vec::<String>::new());
    assert!(!text.contains("== env "));
    // Only what is named: this machine's environment is not in the script.
    let one = parse_env(&strings(&["A=1"])).unwrap();
    assert_eq!(given(&plain(&one)), ["A"]);
}

#[test]
fn a_quote_in_a_value_is_said_again_and_everything_else_is_taken_as_it_is() {
    let env = parse_env(&strings(&[
        "A=it's",
        r#"B=["bin/cf.exe","ui"]"#,
        r#"C=$x `y` %z% "q" & |"#,
    ]))
    .unwrap();
    let text = plain(&env);
    assert!(text.contains("$env:A = 'it''s'\n"));
    assert!(text.contains("$env:B = '[\"bin/cf.exe\",\"ui\"]'\n"));
    assert!(text.contains("$env:C = '$x `y` %z% \"q\" & |'\n"));
}

#[test]
fn the_variables_keep_the_order_they_were_named_in() {
    let env = parse_env(&strings(&["Z=1", "A=2", "M=3"])).unwrap();
    assert_eq!(given(&plain(&env)), ["Z", "A", "M"]);
}

#[test]
fn the_command_runs_as_cmd_exe_reads_a_line_from_a_file_with_its_percent_signs_doubled() {
    assert_eq!(
        command_line(&["npm", "run", "live:paste"]),
        "npm run live:paste"
    );
    assert_eq!(
        command_line(&["echo", "a b", "c\"d", "%PATH%"]),
        r#"echo "a b" "c\"d" "%%PATH%%""#
    );
    // An empty word is a word, and a plain one has none of what a shell reads.
    assert_eq!(
        command_line(&["echo", "", "a=b,c@d+e"]),
        r#"echo "" a=b,c@d+e"#
    );
    let text = script(&[], &command_line(&["echo", "it's"]), &[]);
    assert!(text.contains(r#"'echo "it''s"'"#), "{text}");
}

/// What the JavaScript sent (`remoteScript`, and before it `sentBefore`), recorded
/// before it was deleted: `tests/golden/windows/<name>.ps1`, where each carriage
/// return the script holds is the mark `␍`, which git's `text=auto` would take out
/// of a file.
fn golden(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("windows")
        .join(format!("{name}.ps1"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(!text.contains('\r'), "{name} holds a real carriage return");
    text.replace('\u{240d}', "\r")
}

/// A run as the golden files record it: what the command line said, where its
/// copy was and where in it the command ran, what ran first, the variables named.
struct Recorded {
    golden: &'static str,
    words: &'static [&'static str],
    dir: &'static str,
    inside: &'static str,
    steps: &'static [&'static str],
    env: &'static [&'static str],
}

const RECORDED: [Recorded; 7] = [
    Recorded {
        golden: "live-paste",
        words: &["npm", "run", "live:paste"],
        dir: DIR,
        inside: ".",
        steps: &[],
        env: &[],
    },
    Recorded {
        golden: "eval-with-the-build",
        words: &[
            "npm",
            "run",
            "eval",
            "--",
            "--scenario",
            "round-trip",
            "--chief",
            "devin",
        ],
        dir: DIR,
        inside: ".",
        steps: &BUILD_STEPS,
        env: &[],
    },
    Recorded {
        golden: "cargo-test-in-a-folder",
        words: &["cargo", "test", "it"],
        dir: "build two",
        inside: "app/src-tauri",
        steps: &[],
        env: &[],
    },
    Recorded {
        golden: "node-e-with-quotes",
        words: &["node", "-e", r#"console.log("it's 100%")"#],
        dir: DIR,
        inside: ".",
        steps: &[],
        env: &[],
    },
    Recorded {
        golden: "echo-with-every-quote",
        words: &["echo", "a'b", "c d", "e\"f", "%PATH%"],
        dir: DIR,
        inside: "tests",
        steps: &[BUILD_STEPS[0], BUILD_STEPS[1]],
        env: &[],
    },
    // The command this landing is proven by on the machine.
    Recorded {
        golden: "xtask-test-integration",
        words: &["cargo", "xtask", "test", "integration"],
        dir: DIR,
        inside: ".",
        steps: &[],
        env: &[],
    },
    // The one with variables: what `--env` added, as the JavaScript wrote it.
    Recorded {
        golden: "env-with-the-build",
        words: &[
            "cargo",
            "xtask",
            "eval",
            "--scenario",
            "round-trip",
            "--chief",
            "devin",
        ],
        dir: DIR,
        inside: ".",
        steps: &BUILD_STEPS,
        env: &[
            "CONSENSFLOW_TEST_DAEMON=native",
            "CONSENSFLOW_NOTE=it's 100% $x `y` \"q\" & |",
        ],
    },
];

#[test]
fn a_run_is_sent_the_script_the_javascript_sent_byte_for_byte() {
    for run in RECORDED {
        let env = parse_env(&strings(run.env)).unwrap();
        let sent = remote_script(&Script {
            dir: run.dir,
            inside: run.inside,
            steps: run.steps,
            command: &command_line(run.words),
            env: &env,
        });
        let expected = golden(run.golden);
        assert!(
            sent == expected,
            "{}: not what the JavaScript sent\n--- sent\n{sent:?}\n--- recorded\n{expected:?}",
            run.golden
        );
    }
}

#[test]
fn a_run_that_names_no_variable_is_sent_what_it_was_sent_before_there_was_env() {
    let without: Vec<_> = RECORDED.iter().filter(|run| run.env.is_empty()).collect();
    assert_eq!(without.len(), 6, "the six the JavaScript's own test held");
    for run in without {
        assert!(!golden(run.golden).contains("== env "), "{}", run.golden);
    }
}

#[test]
fn the_script_that_ends_a_run_there_is_the_one_the_javascript_sent_with_its_progress_left_unsaid() {
    assert_eq!(
        stop_script(DIR),
        "\n$ProgressPreference = 'SilentlyContinue'\n\
         $running = Join-Path (Join-Path $env:USERPROFILE 'consensflow-build') '.windows-run.pid'\n\
         if (Test-Path $running) { taskkill /PID (Get-Content $running) /T /F | Out-Null; Remove-Item -LiteralPath $running -Force }\n"
    );
}

#[test]
fn a_mark_in_a_value_is_only_text_and_no_template_has_a_mark_nothing_fills() {
    // What is written in is not read again.
    let text = script(&[], "echo «dir» «running»", &[]);
    assert!(text.contains("'echo «dir» «running»'"), "{text}");
    // Every mark of the two templates is one the fills name: none is left in what is sent.
    for text in [plain(&[]), stop_script(DIR)] {
        let left: Vec<_> = text.matches('«').collect();
        assert!(left.is_empty(), "{text}");
    }
    // A mark that is never closed is left as it was written.
    assert_eq!(fill("a «b", &[("b", "x")]), "a «b");
    assert_eq!(fill("a «b» «c»", &[("b", "x")]), "a x «c»");
}

/// The longest run there is: the build, a command as long as an eval's.
fn longest(env: &[&str]) -> String {
    let command = command_line(&[
        "cargo",
        "xtask",
        "eval",
        "--scenario",
        "question-trip",
        "--chief",
        "claude",
        "--staff",
        "devin",
        "--timeout-min",
        "30",
        "--model",
        "claude-sonnet-5",
    ]);
    script(&BUILD_STEPS, &command, &parse_env(&strings(env)).unwrap())
}

#[test]
fn the_script_goes_encoded_as_powershell_reads_it_and_comes_back_as_it_was() {
    let text = longest(&["CONSENSFLOW_TEST_DAEMON=native"]);
    assert_eq!(script_of(&encode_script(&text).unwrap()), text);
    // A character past the first plane is two units, as PowerShell reads UTF-16.
    let two = encode_script("é😀").unwrap();
    assert_eq!(two, STANDARD.encode([0xe9, 0, 0x3d, 0xd8, 0x00, 0xde]));
}

#[test]
fn the_build_and_the_variables_a_run_names_in_practice_go_with_room_to_spare() {
    for env in [
        &[][..],
        &["CONSENSFLOW_TEST_DAEMON=native"],
        &[
            "CONSENSFLOW_TEST_DAEMON=native",
            "CONSENSFLOW_TEST_CLI=native",
        ],
    ] {
        let encoded = encode_script(&longest(env)).unwrap();
        assert!(encoded.len() < ENCODED_LIMIT, "{env:?}: {}", encoded.len());
    }
}

#[test]
fn a_script_the_machines_shell_could_not_read_is_refused_saying_why() {
    let many: Vec<String> = (0..12)
        .map(|at| format!("CONSENSFLOW_TEST_VARIABLE_{at}={}", "x".repeat(40)))
        .collect();
    let env = parse_env(&many).unwrap();
    let text = script(&BUILD_STEPS, "cargo xtask eval", &env);
    let said = encode_script(&text).unwrap_err().to_string();
    assert!(
        said.starts_with("the script for the machine is ")
            && said.ends_with(
                " characters encoded, and its shell reads 8000 at most: name fewer variables with --env, or run a shorter command"
            ),
        "{said}"
    );
}

#[test]
fn the_build_stages_the_bundle_and_fetches_the_console_host_through_cargo_xtask_not_the_scripts() {
    assert!(BUILD_STEPS.contains(&"cargo xtask stage"));
    assert!(BUILD_STEPS.contains(&"cargo xtask conpty --into app/src-tauri/target/release"));
    for step in BUILD_STEPS {
        for script in ["conpty", "prepare-sidecar", "build-cf", "portable"] {
            assert!(!step.contains(&format!("app/scripts/{script}")), "{step}");
        }
        assert!(!step.contains("prepare-sidecar"), "{step}");
    }
}

#[test]
fn the_build_keeps_the_page_before_the_bundle_and_the_pane_host_before_the_console_host() {
    let at = |step: &str| BUILD_STEPS.iter().position(|one| *one == step).unwrap();
    assert!(at("npm --prefix app run bundle:ui") < at("cargo xtask stage"));
    assert!(
        at("npm run build:bridge") < at("cargo xtask conpty --into app/src-tauri/target/release")
    );
}

#[test]
fn each_step_of_the_build_is_a_line_the_machines_shell_reads_whole() {
    for step in BUILD_STEPS {
        let words: Vec<&str> = step.split(' ').collect();
        assert_eq!(command_line(&words), step);
    }
}
