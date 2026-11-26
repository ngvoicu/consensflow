//! What a run has the machine run is text, and nothing here reaches a machine:
//! that the variables a run names get to its command, and no others, is read in
//! the run script; what the bootstrap does (the lock, the unpack, the tree said) in
//! the bootstrap.

mod bootstrap;
mod golden;
#[cfg(windows)]
mod lock;
mod owned;
mod packages;
mod powershell;
mod reports;
mod root;
mod runs;
#[cfg(windows)]
mod stage;
#[cfg(windows)]
mod stale;
mod sweep;
mod temp;
mod unpack;

use super::*;
use crate::windows::tail::Tail;
use crate::windows::testing::{script_of, strings};

const DIR: &str = "consensflow-build";
/// A run's name, as [`run_id`] makes them.
const ID: &str = "0000abcd-12345678";
const BUILD: [&str; 2] = ["npm ci --no-audit --no-fund", "npm run build:bridge"];
const EXIT_PATH_WRAPPER: &str = "try { %BODY% } finally { Remove-Item -LiteralPath (Join-Path $env:TEMP 'cf-nothing-here') -Force -ErrorAction SilentlyContinue }";

#[test]
fn stale_cases_with_long_lived_children_use_the_owned_fixture_job() {
    let stale = include_str!("tests/stale.rs");
    for name in [
        "a_run_killed_hard_leaves_records_that_neither_make_the_machine_busy_nor_refuse_the_next_run",
        "a_run_holds_nothing_of_the_machine_by_the_time_its_exit_line_is_read",
    ] {
        let body = stale
            .split_once(&format!("fn {name}()"))
            .unwrap()
            .1
            .split_once("\n}\n")
            .unwrap()
            .0;
        assert!(
            body.contains("case.contained_run("),
            "{name} starts a command that can outlive the bootstrap; its fixture must keep a job"
        );
    }
}

fn exit_path_invocation(
    shell: &str,
    temporary: &std::path::Path,
    script: &str,
) -> crate::process::Invocation {
    crate::process::Invocation::new(shell, ".")
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
        .arg(encode_script(script).unwrap())
        .var("TEMP", temporary)
}

#[test]
fn exit_path_fixture_leaves_a_same_named_file_in_the_callers_temp() {
    use cf_base::env::Env;

    let Some(shell) = powershell::shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let outside = tempfile::tempdir().unwrap();
    let own = tempfile::tempdir().unwrap();
    let decoy = outside.path().join("cf-nothing-here");
    std::fs::write(&decoy, b"unrelated").unwrap();
    let env = Env::from_process()
        .with_var("TEMP", &outside.path().display().to_string())
        .with_var(
            "XDG_CACHE_HOME",
            &own.path().join("cache").display().to_string(),
        );
    let script = format!(
        "{}\nexit 0",
        EXIT_PATH_WRAPPER.replace("%BODY%", "Write-Output 'done'")
    );
    let env = powershell::shell_environment(shell, env);
    let ran = powershell::with_shell_turn(shell, || {
        crate::process::capture(&exit_path_invocation(shell, own.path(), &script), &env)
    })
    .unwrap();
    assert_eq!(ran.code, 0, "{}", said(&ran));
    assert!(decoy.is_file(), "the unrelated same-named file was removed");
    assert_eq!(std::fs::read(decoy).unwrap(), b"unrelated");
}

#[test]
fn t1655_the_sweep_leaves_old_evidence_alone() {
    assert!(KEPT.contains(&"evals"));
    assert!(!sweep_script().contains("$reports = Join-Path $dir 'evals/reports'"));
}

#[test]
fn t1655_the_listing_reads_the_live_matrix_evidence() {
    assert!(reports_script(DIR).contains(r"'app\src-tauri\target\live-matrix'"));
}

/// The run script of a run in the usual folder, in the copy's root: what it is asked for.
fn script(steps: &[&str], command: &str, env: &[(String, String)]) -> String {
    run_script(&Script {
        id: ID,
        dir: DIR,
        inside: ".",
        steps,
        command,
        env,
        node: r"C:\Program Files\nodejs",
        machine: "zeewin2",
        marked: true,
        limits: None,
    })
}

fn plain(env: &[(String, String)]) -> String {
    script(&[], &command_line(&["npm", "run", "test:integration"]), env)
}

/// The bootstrap of a run in the usual folder, which does not depend on what the run is
/// asked for.
fn bootstrap() -> String {
    script_of_run(ID)
}

/// The lines of a script that give the process a variable of its own, by name
/// (PATH and the machine's name, [`TEST_MACHINE`], are the script's).
fn given(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("$env:")?;
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            (rest[name.len()..].starts_with(" = ") && name != "Path" && name != TEST_MACHINE)
                .then_some(name)
        })
        .collect()
}

/// What a PowerShell said on its error stream, as text. Started with `-EncodedCommand` and
/// its error stream piped, it says the stream as CLIXML: `#< CLIXML`, then its objects, its
/// error records among them as `<S S="Error">` strings whose line ends are `_x000D__x000A_`.
/// Printed as it came, a failed case said `#< CLIXML` and no more (CI run 40132724600,
/// 2026-10-24), and whether that was the whole of the stream or only what a reader skipped
/// was not to be told. Each error text of it comes back on lines of its own; a stream that
/// holds none says so; text that is no CLIXML comes back as it is.
fn said_on_stderr(stderr: &str) -> String {
    let Some(objects) = stderr.strip_prefix("#< CLIXML") else {
        return stderr.to_owned();
    };
    let mut errors = Vec::new();
    let mut rest = objects;
    while let Some((_, after)) = rest.split_once("<S S=\"Error\">") {
        let (text, tail) = after.split_once("</S>").unwrap_or((after, ""));
        errors.push(clixml_text(text));
        rest = tail;
    }
    if errors.is_empty() {
        let kept = objects.trim();
        if kept.is_empty() {
            return "#< CLIXML, the header and no object after it".to_owned();
        }
        return format!("#< CLIXML, with no error text in it: {kept}");
    }
    errors.concat()
}

/// A string of a CLIXML stream as the text it stands for: its XML entities, then the
/// `_x000D_` escapes it makes of what XML would not keep (the carriage returns go).
fn clixml_text(escaped: &str) -> String {
    let mut text = escaped.to_owned();
    for (entity, plain) in [
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&apos;", "'"),
        ("&amp;", "&"),
    ] {
        text = text.replace(entity, plain);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some((before, after)) = rest.split_once("_x") {
        out.push_str(before);
        let coded = after
            .get(..5)
            .filter(|code| code.ends_with('_'))
            .and_then(|code| u32::from_str_radix(&code[..4], 16).ok())
            .and_then(char::from_u32);
        match coded {
            Some(character) => {
                if character != '\r' {
                    out.push(character);
                }
                rest = &after[5..];
            }
            None => {
                out.push_str("_x");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// What a program that ran to its end came to, for a case's failure to say: its code, what
/// it wrote, and what PowerShell wrote on its error stream, as text.
fn said(ran: &crate::process::Captured) -> String {
    format!(
        "exit {}\n--- its output\n{}\n--- its error stream\n{}",
        ran.code,
        ran.stdout,
        said_on_stderr(&ran.stderr)
    )
}

#[test]
fn what_a_powershell_said_on_its_error_stream_is_read_as_text_and_a_stream_with_no_text_says_so() {
    let error = |text: &str| format!("<S S=\"Error\">{text}</S>");
    let stream = format!(
        "#< CLIXML\r\n<Objs Version=\"1.1.0.1\" xmlns=\"http://schemas.microsoft.com/powershell/2004/04\">{}{}</Objs>",
        error("Get-Content : The process cannot access the file &apos;C:\\a\\x_x005F_y&apos; because it is being used by another process._x000D__x000A_"),
        error("At line:5 char:1 &lt;here&gt;_x000D__x000A_"),
    );
    assert_eq!(
        said_on_stderr(&stream),
        "Get-Content : The process cannot access the file 'C:\\a\\x_y' because it is being used by another process.\nAt line:5 char:1 <here>\n"
    );
    // What a case saw on 2026-10-24: the header, and nothing after it.
    assert_eq!(
        said_on_stderr("#< CLIXML\r\n"),
        "#< CLIXML, the header and no object after it"
    );
    // Progress records are no error: they are shown, so that they are not taken for the cause.
    let progress = "#< CLIXML\r\n<Objs Version=\"1.1.0.1\"><Obj S=\"progress\" RefId=\"0\"><MS><AV>Preparing modules for first use.</AV></MS></Obj></Objs>";
    assert_eq!(
        said_on_stderr(progress),
        format!(
            "#< CLIXML, with no error text in it: {}",
            progress.strip_prefix("#< CLIXML").unwrap().trim()
        )
    );
    // Text that is no CLIXML, a stream that is empty, and an escape that is none, as they are.
    assert_eq!(said_on_stderr(""), "");
    assert_eq!(
        said_on_stderr("fatal: not a repository\n"),
        "fatal: not a repository\n"
    );
    assert_eq!(
        said_on_stderr(&format!("#< CLIXML\r\n{}", error("a_xZZZZ_b _x00 _x000A"))),
        "a_xZZZZ_b _x00 _x000A"
    );
    // All three parts of a case's failure.
    let ran = crate::process::Captured {
        code: 1,
        stdout: String::new(),
        stderr: "#< CLIXML\r\n".to_owned(),
    };
    assert_eq!(
        said(&ran),
        "exit 1\n--- its output\n\n--- its error stream\n#< CLIXML, the header and no object after it"
    );
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

/// T-2119 N1-b: `--env` cannot mark a machine a test machine, in any case of the name: the
/// runner's own line is the only one that does, from the machine's record.
#[test]
fn the_test_machine_marker_is_refused_as_an_env_word_in_any_case() {
    for name in [
        "CONSENSFLOW_TEST_MACHINE",
        "consensflow_test_machine",
        "Consensflow_Test_Machine",
    ] {
        let said = parse_env(&[format!("{name}=zeewin2")])
            .unwrap_err()
            .to_string();
        assert_eq!(
            said,
            format!("--env {name} says whether the machine is a test machine, which only the runner says, from the machine it runs on"),
        );
    }
    assert!(parse_env(&strings(&["CONSENSFLOW_TEST_MACHINE_NOT=1"])).is_ok());
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

/// T-2007 round 3 (N1): on a machine that is no test machine (zeewin2, whose account is its
/// owner's own) the run gives no marker, and takes away one the account itself may hold.
#[test]
fn a_run_on_an_unmarked_machine_gives_its_command_no_test_machine_marker() {
    let text = run_script(&Script {
        marked: false,
        ..longest(&[])
    });
    assert!(!text.contains("$env:CONSENSFLOW_TEST_MACHINE ="), "{text}");
    assert!(
        text.contains("Remove-Item Env:\\CONSENSFLOW_TEST_MACHINE -ErrorAction SilentlyContinue"),
        "{text}"
    );
    assert!(text.contains("== test machine: none"), "{text}");
}

#[test]
fn a_run_tells_its_command_which_machine_it_is_on_before_the_steps_and_the_command_run() {
    // The live kit renews the logins of Claude Code and Pi on a machine that has this variable,
    // and nowhere else (T-1067): nothing but this runner sets it, so it is the signal that a
    // program was started on a machine that is there to test on.
    let text = script(&BUILD, "npm run test:integration", &[]);
    assert!(
        text.contains("$env:CONSENSFLOW_TEST_MACHINE = 'zeewin2'\n"),
        "{text}"
    );
    assert!(
        text.contains(r#"Write-Output "== test machine: $env:CONSENSFLOW_TEST_MACHINE""#),
        "{text}"
    );
    let at = |what: &str| {
        text.find(what)
            .unwrap_or_else(|| panic!("{what} is not there"))
    };
    assert!(
        at("$env:CONSENSFLOW_TEST_MACHINE =") < at("foreach ($step in"),
        "the steps (the build) run on the machine too"
    );
    assert!(at("$env:CONSENSFLOW_TEST_MACHINE =") < at(r#"cmd /c "$run 2>&1""#));
    // The name is the runner's own, said as any value is: a quote twice.
    let odd = run_script(&Script {
        machine: "it's",
        ..longest(&[])
    });
    assert!(
        odd.contains("$env:CONSENSFLOW_TEST_MACHINE = 'it''s'\n"),
        "{odd}"
    );
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

/// PowerShell reads the typographic single quotes as quotes too: a plant named "a worker’s
/// turn" ended the run's command at its quote, on zeewin2 (integration B round 2, T-800). A
/// variable's value is ASCII (`parse_env`), so the command is where one can come.
#[test]
fn a_typographic_quote_in_the_command_is_said_again_as_an_apostrophe_is() {
    let text = script(
        &[],
        &command_line(&[
            "cargo",
            "xtask",
            "plants",
            "daemon",
            "a worker\u{2019}s turn",
        ]),
        &[],
    );
    assert!(
        text.contains("'cargo xtask plants daemon \"a worker\u{2019}\u{2019}s turn\"'"),
        "{text}"
    );
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

#[test]
fn the_line_that_ends_the_output_gives_the_status_the_command_ended_with() {
    for (line, said) in [
        ("== exit 0", 0),
        ("== exit 7", 7),
        ("== exit 75", 75),
        ("== exit 255", 255),
        ("== exit 256", 256),
        ("== exit -1", -1),
        ("== exit -1073741819", -1_073_741_819),
        ("== exit 2147483647", i32::MAX),
        ("== exit -2147483648", i32::MIN),
        // As a line ends where PowerShell writes it, and as a line may be left.
        ("== exit 7\r", 7),
        ("== exit 7\r\n", 7),
        ("== exit 7  ", 7),
        ("== exit 007", 7),
    ] {
        assert_eq!(exit_said(line), Some(said), "{line:?}");
    }
    // Any other line says nothing: one that is nearly it, and one a command wrote.
    for line in [
        "",
        "== exit",
        "== exit ",
        "== exit x",
        "== exit 7 8",
        "== exit 7x",
        "== exit 0x7",
        "== exit 7.0",
        "== exit +7",
        "== exit --7",
        "== exit -",
        "== exit  7",
        "== exit \u{661}",
        "== exit 2147483648",
        "== exit -2147483649",
        "==exit 7",
        "== Exit 7",
        " == exit 7",
        "x== exit 7",
        "== env CF=7",
        "== exit 7 and the command goes on",
    ] {
        assert_eq!(exit_said(line), None, "{line:?}");
    }
}

#[test]
fn every_way_out_of_the_script_goes_through_the_function_that_says_how_it_went() {
    let (boot, run) = (
        bootstrap(),
        script(&BUILD_STEPS, "cargo xtask test integration", &[]),
    );
    // `exit` is a word of the scripts in one line: the function that writes the
    // line and exits, whose own words are the mark and the status.
    let leaving: Vec<&str> = boot
        .lines()
        .chain(run.lines())
        .filter(|line| {
            line.split(|c: char| !c.is_ascii_alphanumeric())
                .any(|word| word == "exit")
        })
        .collect();
    assert_eq!(
        leaving,
        [
            r#"function Exit-Run([int]$code) { $script:completed = $code; exit $code }"#,
            r#"    echo "`n== exit $script:completed""#,
        ]
    );
    // And it is what a profile or a copy that is a link, the lock, a machine with no tar of its
    // own, the unpacking and the sweep leave by (the bootstrap), and a step, a preparation that
    // could not be done and the command (the run script).
    assert_eq!(boot.matches("Exit-Run ").count(), 5, "{boot}");
    assert_eq!(run.matches("Exit-Run ").count(), 3, "{run}");
    assert!(
        boot.contains("if ($failed.Count) { Exit-Run 1 }\n"),
        "{boot}"
    );
    assert!(
        boot.contains(r#"$($other.Id))" })$(if ($why) { " (the lock could not be opened: $why)" }): one at a time."; Exit-Run 75 }"#),
        "{boot}"
    );
    // The last of the run script is what the command ended with, said.
    assert!(
        run.ends_with("cmd /c \"$run 2>&1\"\nExit-Run $LASTEXITCODE\n"),
        "{run}"
    );
    // What the bootstrap does after it reads the run script takes away what the run leaves
    // (the job's end-on-close, its records if they are its, its lock, its tarball), which says
    // nothing (so the line is the last); the records and the lock went before the line, in
    // `Exit-Run`.
    assert!(
        boot.contains(
            "\n} finally {\n  if ($null -ne $job) { [CfJob]::Release($job) }\n  Free-Machine\n  if ($null -ne $script:completed) {"
        ),
        "{boot}"
    );
}

/// The machine's own PowerShell, on a machine that has one: from every place the
/// script leaves from, the function says the status as the last line the program
/// writes and the program ends with it. (What ssh then answers is another matter:
/// `windows`' module doc says.)
#[cfg(windows)]
#[test]
fn powershell_says_how_it_went_in_its_last_line_from_every_place_the_script_leaves_from() {
    use cf_base::env::Env;

    use crate::process;

    let text = bootstrap();
    // The function and the one it calls first (`Free-Machine`, which holds nothing here), as the
    // bootstrap has them.
    let from = text.find("function Free-Machine").unwrap();
    let to = text[from..].find("function Exit-Run").unwrap() + from;
    let end_of_line = text[to..].find('\n').unwrap() + to;
    let function = format!(
        "$held = $false\n$lock = $null\n{}",
        &text[from..end_of_line]
    );
    let temporary = tempfile::tempdir().unwrap();
    let way_out = EXIT_PATH_WRAPPER;
    for (body, status) in [
        // At the top, as the lock's.
        ("Exit-Run 75".to_owned(), 75),
        // In a try, as the unpacking's, and in a loop, as a step's.
        (
            way_out.replace("%BODY%", r#"cmd /c "exit 7"; if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }"#),
            7,
        ),
        (
            way_out.replace("%BODY%", r#"foreach ($step in @('a', 'b')) { Write-Output "== $step"; cmd /c "exit 3"; if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE } }"#),
            3,
        ),
        // At the end, as the command's, whether it ended well or not at all as a number.
        (way_out.replace("%BODY%", r#"cmd /c "exit 0"; Exit-Run $LASTEXITCODE"#), 0),
        (way_out.replace("%BODY%", r#"cmd /c "exit -1"; Exit-Run $LASTEXITCODE"#), -1),
    ] {
        let finally = text.rsplit_once("} finally {").unwrap().1;
        let folder = temporary.path().display().to_string().replace('\'', "''");
        let script = format!("$ProgressPreference = 'SilentlyContinue'\n$runFolder = '{folder}'\n$recordable = $true\n$job = $null\n{function}\ntry {{ {body} }} finally {{{finally}");
        let ran = process::capture(
            &exit_path_invocation("powershell", temporary.path(), &script),
            &Env::from_process(),
        )
        .unwrap();
        let mut tail = Tail::default();
        tail.push(ran.stdout.as_bytes());
        assert_eq!(
            tail.last_line().as_deref().and_then(exit_said),
            Some(status),
            "{body}\n{}",
            said(&ran)
        );
        assert_eq!(ran.code, status, "{body}\n{}", said(&ran));
    }
}

/// The machine's own PowerShell: a command that leaves its last line unfinished (as a
/// program does that writes to the stream itself, `cmd`'s `set /p` with no input among them,
/// and `type` of a file with no final line end) has the line ended before the one that
/// says how the run went, and that line is read back, a pass answered as a pass.
#[cfg(windows)]
#[test]
fn powershell_ends_the_line_a_command_left_unfinished_before_it_says_how_it_went() {
    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    let text = bootstrap();
    let from = text.find("function Free-Machine").unwrap();
    let to = text[from..].find("function Exit-Run").unwrap() + from;
    let end = text[to..].find('\n').unwrap() + to;
    let function = &text[from..end];
    let temporary = tempfile::tempdir().unwrap();
    for (body, status) in [
        (r#"cmd /c "<nul set /p x=unfinished"; Exit-Run 0"#, 0),
        (r#"[Console]::Out.Write('unfinished'); Exit-Run 0"#, 0),
        (r#"cmd /c "<nul set /p x=unfinished"; Exit-Run 7"#, 7),
        // And one that did end its last line is as well read, with a blank line before the exit line.
        (r#"Write-Output 'finished'; Exit-Run 0"#, 0),
    ] {
        let finally = text.rsplit_once("} finally {").unwrap().1;
        let folder = temporary.path().display().to_string().replace('\'', "''");
        let script = format!("$ProgressPreference = 'SilentlyContinue'\n$runFolder = '{folder}'\n$recordable = $true\n$job = $null\n$held = $false\n$lock = $null\n{function}\ntry {{ {body} }} finally {{{finally}");
        let ran = process::capture(
            &Invocation::new("powershell", ".")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                .arg(encode_script(&script).unwrap()),
            &Env::from_process(),
        )
        .unwrap();
        assert!(
            !ran.stdout.contains("unfinished== exit"),
            "{body}\n{}",
            ran.stdout
        );
        let mut tail = Tail::default();
        tail.push(ran.stdout.as_bytes());
        assert_eq!(
            tail.last_line().as_deref().and_then(exit_said),
            Some(status),
            "{body}\n{}",
            said(&ran)
        );
        assert_eq!(ran.code, status, "{body}\n{}", said(&ran));
    }
}

#[test]
fn a_stop_without_its_record_leaves_a_same_named_tarball_alone() {
    let Some(shell) = powershell::shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let folder = tempfile::tempdir().unwrap();
    let profile = folder.path().join("profile");
    std::fs::create_dir_all(profile.join(DIR)).unwrap();
    let tarball = profile.join(upload_name(DIR, ID));
    std::fs::create_dir_all(tarball.parent().unwrap()).unwrap();
    std::fs::write(&tarball, b"unrelated").unwrap();
    let profile = profile.display().to_string().replace('\'', "''");
    let script = format!("$env:USERPROFILE = '{profile}'\n{}", stop_script(DIR, ID));
    let failure = powershell::told(shell, folder.path(), &script).unwrap_err();
    assert!(failure.contains("No matching run record"), "{failure}");
    assert_eq!(std::fs::read(tarball).unwrap(), b"unrelated");
}

/// The script of a run named `id`, in the usual folder.
fn script_of_run(id: &str) -> String {
    remote_script(&Script {
        id,
        dir: DIR,
        inside: ".",
        steps: &[],
        command: "x",
        env: &[],
        node: r"C:\Program Files\nodejs",
        machine: "zeewin2",
        marked: true,
        limits: None,
    })
}

#[test]
fn a_run_is_named_to_the_width_a_script_is_made_for_and_no_two_runs_are_named_alike() {
    let ids: Vec<String> = (0..50).map(|_| run_id()).collect();
    for id in &ids {
        // Eight hex digits (this process's), a dash, eight more (random): always the
        // same width, which the script's size is checked with before the run has one.
        assert_eq!(id.len(), 17, "{id}");
        assert!(
            id.char_indices().all(|(at, c)| if at == 8 {
                c == '-'
            } else {
                c.is_ascii_digit() || ('a'..='f').contains(&c)
            }),
            "{id}"
        );
    }
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        ids.len(),
        "two runs of one process named alike"
    );
    // The run's process is the first of its name.
    assert!(ids[0].starts_with(&format!("{:08x}-", std::process::id())));
    // The sweep reads no such name: it removes no tarball by the shape of its name (T-1368).
    assert!(
        !sweep_script().contains(".tgz"),
        "the sweep reads the names runs are given"
    );
}

#[test]
fn a_run_names_the_tarball_it_unpacks_by_its_own_name_and_no_other_name_goes_in_the_script() {
    // The machine is one profile folder, and runs from several trees send to it: a
    // name they shared let the run that sent last be the one every script unpacked.
    let (a, b) = ("0000abcd-12345678", "0000abce-9abcdef0");
    assert_eq!(
        upload_name(DIR, a),
        "consensflow-wrun-0000abcd-12345678/tree.tgz"
    );
    assert_ne!(upload_name(DIR, a), upload_name(DIR, b));
    assert_eq!(upload_name("one", a), upload_name("two", a));
    let (for_a, for_b) = (script_of_run(a), script_of_run(b));
    for (text, own, other) in [(&for_a, a, b), (&for_b, b, a)] {
        let tarball =
            format!("$tarball = Join-Path $env:USERPROFILE 'consensflow-wrun-{own}/tree.tgz'\n");
        assert!(text.contains(&tarball), "{text}");
        assert!(!text.contains(other), "{text}");
        // Its name is in the bootstrap once; the unpack reads that variable.
        assert_eq!(
            text.matches(&format!("consensflow-wrun-{own}/tree.tgz"))
                .count(),
            1,
            "{text}"
        );
        assert_eq!(text.matches(".tgz'").count(), 1, "{text}");
        assert!(text.contains("& $tar -xmzf $tarball -C $dir\n"), "{text}");
    }
}

#[test]
fn a_run_records_completion_and_removes_lock_records_only_if_it_holds_them() {
    let text = bootstrap();
    let at = |what: &str| {
        text.find(what)
            .unwrap_or_else(|| panic!("{what} is not there: {text}"))
    };
    // A lock refusal reaches the finally without taking another run's records.
    assert!(at("$held = $false\n") < at("try {\n"));
    assert!(at("try {\n") < at("Exit-Run 75"));
    assert!(at("Exit-Run 75") < at("} finally {\n"));
    let finally = &text[at("} finally {\n")..];
    assert!(
        finally.contains("Join-Path $runFolder 'completed.txt'"),
        "{finally}"
    );
    // The records are the run's own to take away only if it holds the lock, and the pid file
    // only if it still holds this run's process: a script of the older form may have
    // written its own over it. They go first of what the run does to say it has ended
    // (`Free-Machine` in `finally` before the exit line), so that the next run is never
    // refused by this one's end.
    assert!(text.contains("ri -Lit $owner -Force -ea 0"), "{text}");
    assert!(
        text.contains("if ((gc $running -ea 0) -eq $PID) { ri -Lit $running -Force -ea 0 }"),
        "{text}"
    );
    assert!(finally.contains("  Free-Machine\n"), "{finally}");
    assert!(at("function Free-Machine") < at("function Exit-Run"));
    // Held from the line that writes the records, and from none before it.
    assert!(at("Set-Content -Path $running") < at("Set-Content -Path $owner"));
    assert!(at("Set-Content -Path $owner") < at("$held = $true\n"));
    assert!(at("$held = $true\n") < at("& $tar -xmzf"));
    assert_eq!(text.matches("$held = $true").count(), 1, "{text}");
}

#[test]
fn the_lock_is_a_file_opened_for_the_run_alone_before_anything_else_is_done_in_the_folder() {
    let text = bootstrap();
    let at = |what: &str| {
        text.find(what)
            .unwrap_or_else(|| panic!("{what} is not there: {text}"))
    };
    // Opened in one act of the system that no other run can share, so that of two runs
    // that start within a millisecond one is refused: not looked at and then written.
    let open = "try { $lock = [IO.File]::Open((Join-Path $dir '.windows-run.lock'), 'OpenOrCreate', 'ReadWrite', 'None') } catch [IO.IOException] {} catch { $why = $_.Exception.Message }\n";
    assert!(text.contains(open), "{text}");
    assert_eq!(text.matches("[IO.File]::Open(").count(), 1, "{text}");
    // A run that cannot open it is refused, and so is one that can while a process of the
    // older form of the lock is alive; both with the answer 75, the process said where it
    // is known, and the reason where the open failed for any cause but the lock being open
    // (a run that dies of that has no exit line, and the Mac answers 1 for a refusal).
    assert!(
        text.contains("if (-not $lock -or $other) { echo \"A run is still going on this machine$(if ($other) { \" (process $($other.Id))\" })$(if ($why) { \" (the lock could not be opened: $why)\" }): one at a time.\"; Exit-Run 75 }\n"),
        "{text}"
    );
    // Before the folder is touched by the run: the record, the owner, the unpack.
    assert!(at(open.trim_end()) < at("Exit-Run 75"));
    assert!(at("Exit-Run 75") < at("Set-Content -Path $running"));
    assert!(at("Exit-Run 75") < at("& $tar -xmzf"));
    assert!(at("ni -Type Directory") < at(open.trim_end()));
    // Closed when the run ends however it ends, a refusal too; the file stays in the folder.
    let finally = &text[at("} finally {\n")..];
    assert!(
        text.contains("  if ($script:lock) { $script:lock.Dispose(); $script:lock = $null }\n}\n"),
        "{text}"
    );
    assert!(finally.contains("  Free-Machine\n"), "{finally}");
    assert!(!finally.contains("windows-run.lock"), "{finally}");
    assert_eq!(LOCK, ".windows-run.lock");
    assert!(
        KEPT.contains(&LOCK),
        "the sweep would remove the lock the run holds"
    );
}

#[test]
fn the_process_is_still_written_and_read_as_every_version_reads_it_and_the_run_is_named_beside_it()
{
    let text = bootstrap();
    // A script of an older form holds the process in the file and refuses a run that is
    // still going by looking at it: this one writes it, so that such a script refuses, and
    // reads it, so that it is refused by one.
    assert!(
        text.contains("Set-Content -Path $running -Value $PID\n"),
        "{text}"
    );
    // The file is read as nothing where it is gone or being written (no error, no process),
    // and a process counts only if it is a PowerShell that was started before the file was
    // written: a process id that a later program has taken is no run, and the lock decides.
    assert!(text.contains("$holder = gc $running -ea 0\n"), "{text}");
    assert!(
        text.contains("$other = if ($holder) { try { gps -Id $holder -ea Stop | ? { ($_.Name -in @('powershell', 'pwsh')) -and $_.StartTime -le (gi $running).LastWriteTime } } catch {} }\n"),
        "{text}"
    );
    // The run is named in the file beside it, with its process, which only a stop reads.
    assert!(
        text.contains("Set-Content -Path $owner -Value @('0000abcd-12345678', $PID)\n"),
        "{text}"
    );
    assert_eq!(RUNNING, ".windows-run.pid");
    assert_eq!(OWNER, ".windows-run.id");
    assert!(KEPT.contains(&OWNER) && KEPT.contains(&RUNNING));
    // Written once the lock is the run's, never before.
    let at = |what: &str| text.find(what).unwrap();
    assert!(at("Exit-Run 75") < at("Set-Content -Path $running -Value $PID"));
}

#[test]
fn a_command_whose_last_output_has_no_line_end_still_passes() {
    // The command's output goes to the stream as it writes it, so it may end inside a line
    // (`type` of a file with no final line end does, and the script's line was written onto
    // it: `marker of tree A== exit 0`, which is no exit line, and a pass was answered as 1,
    // seen on zeewin 2026-10-22). The line the run ends by begins with a line feed, and
    // that is the only line of the script that is read back, so it is the only one that must.
    let text = bootstrap();
    assert!(
        text.contains("echo \"`n== exit $script:completed\""),
        "{text}"
    );
    // What reads it back takes the last line that is not blank, so the blank line that
    // comes of a command that did end its last line is no matter.
    for output in [
        "marker of tree A\n\n== exit 0\r\n",
        "marker of tree A\r\n\n== exit 0\r\n",
        "\n== exit 0\r\n",
    ] {
        let mut tail = Tail::default();
        tail.push(output.as_bytes());
        assert_eq!(
            tail.last_line().as_deref().and_then(exit_said),
            Some(0),
            "{output:?}"
        );
    }
}

#[test]
fn a_stop_checks_the_run_and_the_process_handle_before_termination() {
    let stop = stop_script(DIR, ID);
    for required in [
        "$of[0] -ne '0000abcd-12345678'",
        "$of[2] -ne 'job'",
        "$of[4] -ne 'consensflow-wrun-0000abcd-12345678/tree.tgz'",
        "[int64]::TryParse($of[3]",
        "if ($actual -ne $created)",
        "OpenProcess(0x101001, false, pid)",
        "GetProcessTimes(p, out created",
        "if (created != expected) return \"gone\"",
        "TerminateProcess(p, 1)",
    ] {
        assert!(stop.contains(required), "missing {required}: {stop}");
    }
    assert!(!stop.contains("taskkill"), "{stop}");
    assert!(
        !stop.contains("Remove-Item"),
        "a later run can replace these paths: {stop}"
    );
    assert!(!stop_script(DIR, "0000abce-9abcdef0").contains(ID));
}

#[test]
fn a_run_records_its_job_and_process_identity_before_starting_its_command() {
    let run = script(&[], "cmd /c exit 0", &[]);
    let at = |what: &str| run.find(what).unwrap_or_else(|| panic!("no {what}: {run}"));
    assert!(
        run.contains("Add-Content -LiteralPath $owner -Value @('job',"),
        "{run}"
    );
    assert!(
        run.contains("GetCurrentProcess().StartTime.ToFileTimeUtc()"),
        "{run}"
    );
    assert!(
        run.contains("consensflow-wrun-0000abcd-12345678/tree.tgz"),
        "{run}"
    );
    assert!(at("$job = $made") < at("Add-Content -LiteralPath $owner"));
    assert!(at("Add-Content -LiteralPath $owner") < at("foreach ($step in"));
    let stop = stop_script(DIR, ID);
    assert!(stop.contains("$of.Count -lt 5"), "{stop}");
    assert!(!stop.contains("/T"), "{stop}");
}

#[test]
fn the_script_says_which_tree_it_unpacked_once_the_sweep_has_gone_and_before_anything_runs() {
    let text = bootstrap();
    let at = |what: &str| {
        text.find(what)
            .unwrap_or_else(|| panic!("{what} is not there: {text}"))
    };
    assert!(
        text.contains(
            "echo \"== tree $(Get-Content -LiteralPath (Join-Path $dir '.windows-tree') -TotalCount 1)\"\n"
        ),
        "{text}"
    );
    assert!(at("if ($failed.Count) { Exit-Run 1 }") < at("echo \"== tree "));
    // And only then is the run script read: what the run is asked for runs on a tree that is said.
    let read =
        ". ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '.windows-run.ps1'))))\n";
    assert!(text.contains(read), "{text}");
    assert!(at("echo \"== tree ") < at(read));
    assert!(at(read) < at("} finally {"));
    assert!(!text.contains("foreach ($step in"), "{text}");
    assert!(TREE_MARK.starts_with("== ") && TREE_MARK.ends_with(' '));
    assert_eq!(RUN, ".windows-run.ps1");
    assert!(KEPT.contains(&RUN), "the sweep would remove the run script");
}

#[test]
fn what_a_run_is_asked_for_is_in_the_run_script_and_the_bootstrap_does_not_depend_on_it() {
    // The steps, the variables and the command go in the tarball; the command line of ssh
    // holds the bootstrap only, which is the same for any run of one folder and one name.
    let env = parse_env(&strings(&["CONSENSFLOW_TEST_DAEMON=native"])).unwrap();
    fn asked<'a>(
        steps: &'a [&'a str],
        command: &'a str,
        env: &'a [(String, String)],
    ) -> Script<'a> {
        Script {
            id: ID,
            dir: DIR,
            inside: "app/src-tauri",
            steps,
            command,
            env,
            node: r"C:\Program Files\nodejs",
            machine: "zeewin2",
            marked: true,
            limits: None,
        }
    }
    let (plain, heavy) = (
        asked(&[], "x", &[]),
        asked(&BUILD_STEPS, "cargo test --package example", &env),
    );
    assert_eq!(remote_script(&plain), remote_script(&heavy));
    let (light, full) = (run_script(&plain), run_script(&heavy));
    assert_ne!(light, full);
    for what in [
        "'npm ci --no-audit --no-fund'",
        "$env:CONSENSFLOW_TEST_DAEMON = 'native'\n",
        "'cargo test --package example'",
        "Set-Location (Join-Path $dir 'app/src-tauri')\n",
    ] {
        assert!(
            full.contains(what) && !remote_script(&heavy).contains(what),
            "{what}"
        );
    }
    // The run script begins as the command does: the program's search path, the steps in the
    // copy, then the variables, then the command, which it ends the run with.
    let at = |what: &str| full.find(what).unwrap_or_else(|| panic!("{what}: {full}"));
    assert!(at("$env:Path = ") < at("Set-Location $dir\n"));
    assert!(at("Set-Location $dir\n") < at("foreach ($step in"));
    assert!(at("foreach ($step in") < at("$env:CONSENSFLOW_TEST_DAEMON ="));
    assert!(at("$env:CONSENSFLOW_TEST_DAEMON =") < at("[IO.File]::WriteAllText"));
    assert!(full.ends_with("cmd /c \"$run 2>&1\"\nExit-Run $LASTEXITCODE\n"));
}

/// The machine's own PowerShell runs `finally` when `exit` leaves the try: the completion
/// receipt is written before the controller's separate cleanup, and the lock's record is
/// retained if the run does not hold it.
#[cfg(windows)]
#[test]
fn powershell_writes_the_receipt_in_finally_when_the_script_leaves_by_exit() {
    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    for (held, lock_stays) in [("$true", false), ("$false", true)] {
        let folder = tempfile::tempdir().unwrap();
        let (tarball, lock, receipt) = (
            folder.path().join("a.tgz"),
            folder.path().join("lock"),
            folder.path().join("completed.txt"),
        );
        std::fs::write(&tarball, "x").unwrap();
        std::fs::write(&lock, "x").unwrap();
        let script = format!(
            "$receipt = '{}'\n$running = '{}'\n$held = {held}\ntry {{ exit 75 }} finally {{\n  [IO.File]::WriteAllText($receipt, 'finished')\n  if ($held) {{ Remove-Item -LiteralPath $running -Force -ErrorAction SilentlyContinue }}\n}}\n",
            receipt.display(),
            lock.display(),
        );
        let ran = process::capture(
            &Invocation::new("powershell", ".")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                .arg(encode_script(&script).unwrap()),
            &Env::from_process(),
        )
        .unwrap();
        assert_eq!(ran.code, 75, "{}", said(&ran));
        assert!(tarball.exists(), "the upload was removed: {held}");
        assert_eq!(std::fs::read_to_string(receipt).unwrap(), "finished");
        assert_eq!(lock.exists(), lock_stays, "held {held}");
    }
}

#[test]
fn a_run_is_put_in_a_job_that_ends_with_it_before_it_starts_anything_and_a_run_that_cannot_be_says_so(
) {
    let text = script(&BUILD, "x", &parse_env(&strings(&["A=1"])).unwrap());
    let at = |what: &str| {
        text.find(what)
            .unwrap_or_else(|| panic!("{what} is not there: {text}"))
    };
    // The run script puts its process in the job first: every program it starts after is in it.
    assert!(
        text.starts_with("\n$job = $null\ntry {\n  Add-Type -TypeDefinition @'\nusing System;"),
        "{text}"
    );
    assert!(
        text.contains("  $made = [CfJob]::Enter()\n  if ($made -eq [IntPtr]::Zero) { throw [CfJob]::Why }\n  $job = $made\n"),
        "{text}"
    );
    assert!(at("$made = [CfJob]::Enter()") < at("$env:Path"));
    assert!(at("$made = [CfJob]::Enter()") < at("foreach ($step in"));
    assert!(at("$made = [CfJob]::Enter()") < at("$env:A = '1'"));
    assert!(at("$made = [CfJob]::Enter()") < at("cmd /c \"$run 2>&1\""));
    // A run that cannot be put in a job (no compiler, no right, the system's no) goes on as it did,
    // saying so, and never ends without its exit line for it.
    assert!(
        text.contains("} catch { Write-Output \"== no job: $($_.Exception.Message), so a stop cannot safely end this run\" }\n"),
        "{text}"
    );
    // What the C# does: a job that ends what is in it when its last handle closes (the only one is this
    // process's), this process put in it, and `Release`, which takes the end-on-close away.
    assert!(text.contains("Limit(job, 0x2000)"), "{text}");
    assert!(
        text.contains("AssignProcessToJobObject(job, GetCurrentProcess())"),
        "{text}"
    );
    assert!(
        text.contains("public static void Release(IntPtr job) { Limit(job, 0); }"),
        "{text}"
    );
    assert!(
        text.contains("SetInformationJobObject(job, 9, ref e, Marshal.SizeOf(typeof(Extended)))"),
        "{text}"
    );
    // The here-string that holds it is closed on a line of its own, and nothing in the C# ends it.
    assert_eq!(text.matches("\n'@\n").count(), 1, "{text}");
    assert!(!JOB_SOURCE.contains("'@"));
}

#[test]
fn the_job_is_released_where_a_run_ends_by_itself_and_not_where_it_is_killed() {
    let boot = bootstrap();
    let at = |what: &str| {
        boot.find(what)
            .unwrap_or_else(|| panic!("{what} is not there: {boot}"))
    };
    // Set before the try, so that a run that never gets so far has none to release.
    assert!(at("$job = $null\n") < at("try {\n"));
    assert_eq!(boot.matches("[CfJob]::Release($job)").count(), 1, "{boot}");
    // In the `finally`, which `exit` goes through and a killed process does not: the first
    // thing it does, so that nothing else it does can keep a program from being left running.
    let finally = &boot[at("} finally {\n")..];
    assert!(
        finally.starts_with("} finally {\n  if ($null -ne $job) { [CfJob]::Release($job) }\n"),
        "{finally}"
    );
    // The bootstrap does not name the job's C#: it is in the run script, and the bootstrap
    // calls it only where it was made.
    assert!(
        !boot.contains("Add-Type") && !boot.contains("CreateJobObject"),
        "{boot}"
    );
    // The stop ends that process, and the system ends the programs of the run when it is gone.
    assert!(stop_script(DIR, ID).contains("[CfStop]::End($number, $created)"));
}

#[test]
fn a_mark_in_a_value_is_only_text_and_no_template_has_a_mark_nothing_fills() {
    // What is written in is not read again.
    let text = script(&[], "echo «dir» «running»", &[]);
    assert!(text.contains("'echo «dir» «running»'"), "{text}");
    // Every mark of the templates is one the fills name: none is left in what is sent.
    for text in [
        bootstrap(),
        plain(&[]),
        stop_script(DIR, ID),
        sweep_script(),
    ] {
        let left: Vec<_> = text.matches('«').collect();
        assert!(left.is_empty(), "{text}");
    }
    // A mark that is never closed is left as it was written.
    assert_eq!(fill("a «b", &[("b", "x")]), "a «b");
    assert_eq!(fill("a «b» «c»", &[("b", "x")]), "a x «c»");
}

/// A long run: a workspace build and test, with as many variables as a run is likely to name.
fn longest(env: &[&str]) -> Script<'static> {
    let env: &'static [(String, String)] =
        Box::leak(parse_env(&strings(env)).unwrap().into_boxed_slice());
    Script {
        id: ID,
        dir: DIR,
        inside: ".",
        steps: &BUILD_STEPS,
        command: "cargo test --workspace --all-targets -- --test-threads=4",
        env,
        node: r"C:\Program Files\nodejs",
        machine: "zeewin2",
        marked: true,
        limits: None,
    }
}

#[test]
fn the_script_goes_encoded_as_powershell_reads_it_and_comes_back_as_it_was() {
    let text = remote_script(&longest(&["CONSENSFLOW_TEST_DAEMON=native"]));
    assert_eq!(script_of(&encode_script(&text).unwrap()), text);
    // A character past the first plane is two units, as PowerShell reads UTF-16.
    let two = encode_script("é😀").unwrap();
    assert_eq!(two, STANDARD.encode([0xe9, 0, 0x3d, 0xd8, 0x00, 0xde]));
}

#[test]
fn the_longest_run_there_is_leaves_the_command_line_a_margin_of_a_thousand_and_more() {
    // The bootstrap is all that is on ssh's command line, and nothing a run is asked for
    // changes it: a build and a command with four variables are as long as no line at all.
    let bare = Script {
        steps: &[],
        command: "x",
        env: &[],
        ..longest(&[])
    };
    let sizes: Vec<usize> = [
        &[][..],
        &["CONSENSFLOW_TEST_DAEMON=native"],
        &[
            "CONSENSFLOW_TEST_DAEMON=native",
            "CONSENSFLOW_TEST_CLI=native",
            "CONSENSFLOW_TEST_VARIABLE_ONE=a value that is long enough to count for something",
            "CONSENSFLOW_TEST_VARIABLE_TWO=a value that is long enough to count for something",
        ],
    ]
    .iter()
    .map(|env| encode_script(&remote_script(&longest(env))).unwrap().len())
    .collect();
    let base = encode_script(&remote_script(&bare)).unwrap().len();
    assert_eq!(sizes, [base, base, base]);
    assert!(
        ENCODED_LIMIT - base >= 1000,
        "the bootstrap takes {base} of {ENCODED_LIMIT}"
    );
    // The stop script goes on the same line.
    let stop = encode_script(&stop_script(DIR, ID)).unwrap().len();
    assert!(ENCODED_LIMIT - stop >= 1000, "the stop takes {stop}");
    // And the run script, in the tarball, has no limit: it holds what the line could not.
    let run = run_script(&longest(&[
        "CONSENSFLOW_TEST_VARIABLE_ONE=a value that is long enough to count for something",
    ]));
    assert!(
        run.contains("cargo test --workspace --all-targets -- --test-threads=4")
            && run.contains("CONSENSFLOW_TEST_VARIABLE_ONE"),
        "{run}"
    );
    // It holds the job's C# besides, and is longer than the line could carry: that is what the
    // tarball is for.
    assert!(
        encode_script(&run).is_err(),
        "the run script has outgrown the line, as it may"
    );
}

#[test]
fn a_folder_with_a_name_too_long_for_the_command_line_is_refused_saying_why() {
    // The copy's name is in the bootstrap; a long one can make the line too long.
    let long = "d".repeat(900);
    let text = remote_script(&Script {
        dir: &long,
        ..longest(&[])
    });
    let said = encode_script(&text).unwrap_err().to_string();
    assert!(
        said.starts_with("the script for the machine is ")
            && said.ends_with(
                " characters encoded, and its shell reads 8000 at most: use a shorter --dir"
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

/// What a machine takes at once: the run script gives the process the builds' jobs and the tests'
/// threads that machine's memory carries, before the steps and the command (T-585: zeewin, with 7 GB
/// and 12 threads, had 0 GB free in both of its workspace runs, one of them 1,396 s and two lock
/// cases failed on its time). A machine that carries all it is given says nothing, and a variable
/// the run names itself, which is given after, wins.
#[test]
fn a_machine_that_takes_less_at_once_has_its_limits_given_before_the_steps_and_the_command() {
    let limited = run_script(&Script {
        limits: Some(Limits {
            build_jobs: 4,
            test_threads: 3,
        }),
        ..longest(&["CARGO_BUILD_JOBS=2"])
    });
    let at = |needle: &str| {
        limited
            .find(needle)
            .unwrap_or_else(|| panic!("no {needle:?} in\n{limited}"))
    };
    let jobs = at("$env:CARGO_BUILD_JOBS = '4'");
    let threads = at("$env:RUST_TEST_THREADS = '3'");
    assert!(
        limited.contains("== limits: CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=3"),
        "{limited}"
    );
    let steps = at("foreach ($step in @(");
    assert!(jobs < steps && threads < steps, "before the steps");
    // The run's own variable is given after, so it is the command's.
    let given = at("$env:CARGO_BUILD_JOBS = '2'");
    assert!(
        jobs < given && given < at("cmd /c \"$run 2>&1\""),
        "{limited}"
    );
    // A machine with no limits says nothing of them.
    let free = run_script(&Script {
        limits: None,
        ..longest(&[])
    });
    assert!(
        !free.contains("RUST_TEST_THREADS") && !free.contains("== limits"),
        "{free}"
    );
}

/// A run is sent the steps that make sure of what its tests run on, after the build's steps and
/// before its command, whether or not it was asked to build: a copy where nobody had built was
/// left to Windows' own console host (a test of the keys read Alt as key 18, zeewin2, T-487) and
/// to a registry that lacked the optional crates (nextest ended at once, T-627).
#[test]
fn every_run_makes_sure_of_the_crates_and_the_console_host_before_its_command() {
    for steps in [&[][..], &BUILD[..]] {
        let run = script(steps, "cargo test -p cf-panes", &[]);
        let at = |needle: &str| {
            run.find(needle)
                .unwrap_or_else(|| panic!("no {needle:?} in\n{run}"))
        };
        let command = at("cmd /c \"$run 2>&1\"");
        let mut last = steps
            .last()
            .map_or(0, |step| at(&format!("'{step}'")) + step.len());
        for step in PREPARE_STEPS {
            let here = at(&format!("'{step}'"));
            assert!(last <= here && here < command, "{step}: {run}");
            last = here;
        }
    }
}

/// The console host goes beside the programs of both profiles: cargo puts the profile's folder
/// on the PATH of the tests it runs, so `cargo test` finds `conpty.dll` in `debug`, and the
/// programs the black-box suites build (`release`) find it there.
#[test]
fn the_console_host_is_staged_beside_the_debug_and_the_release_builds() {
    for profile in ["debug", "release"] {
        let step = format!("cargo xtask conpty --into app/src-tauri/target/{profile}");
        assert!(PREPARE_STEPS.contains(&step.as_str()), "{step}");
    }
}

/// The crates are asked of the network only when the registry lacks one: a machine offline
/// with everything fetched is not refused.
#[test]
fn the_crates_are_fetched_only_when_the_registry_lacks_one() {
    let step = PREPARE_STEPS[0];
    let (offline, online) = step.split_once(" || ").unwrap();
    assert!(
        offline.starts_with("cargo fetch --locked --offline "),
        "{step}"
    );
    assert_eq!(online, "cargo fetch --locked");
}

/// A step that cannot be done ends the run with its status, saying what the run needed, and the
/// command after it is not run.
#[test]
fn a_run_whose_preparation_fails_ends_with_its_status_before_the_command() {
    let run = script(&[], "cargo test", &[]);
    let (_, after) = run.split_once("foreach ($step in @('cargo fetch").unwrap();
    let (loop_body, _) = after
        .split_once("Set-Location (Join-Path $dir '.')")
        .unwrap();
    assert!(
        loop_body.contains("if ($LASTEXITCODE -ne 0)"),
        "{loop_body}"
    );
    assert!(
        loop_body.contains("the command is not run on what happens to be on the machine"),
        "{loop_body}"
    );
    assert!(loop_body.contains("Exit-Run $LASTEXITCODE"), "{loop_body}");
}

#[test]
fn a_reused_pid_does_not_identify_a_run_for_stopping() {
    let Some(shell) = powershell::shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let folder = tempfile::tempdir().unwrap();
    let profile = folder.path().join("profile");
    let marker = folder.path().join("wrong-process-ended");
    std::fs::create_dir_all(profile.join(DIR)).unwrap();
    let quoted = |path: &std::path::Path| path.display().to_string().replace('\'', "''");
    let script = format!(
        "$env:USERPROFILE = '{}'\n$owner = Join-Path (Join-Path $env:USERPROFILE '{DIR}') '{OWNER}'\nSet-Content -LiteralPath $owner -Value @('{ID}', $PID, 'job', '1', 'consensflow-wrun-{ID}/tree.tgz')\nfunction Get-Process {{ [pscustomobject]@{{ Name = 'powershell'; Id = $PID; StartTime = (Get-Date).AddMinutes(-1) }} }}\nfunction Get-Item {{ [pscustomobject]@{{ LastWriteTime = Get-Date }} }}\nfunction taskkill {{ [IO.File]::WriteAllText('{}', 'wrong process') }}\n{}",
        quoted(&profile),
        quoted(&marker),
        stop_script(DIR, ID)
    );
    powershell::run(shell, folder.path(), &script);
    assert!(
        !marker.exists(),
        "a reused PID was selected for termination"
    );
}

#[test]
fn a_runs_upload_is_in_its_unique_folder() {
    let first = upload_name(DIR, ID);
    let second = upload_name(DIR, "0000abcd-12345679");
    assert_eq!(first, format!("consensflow-wrun-{ID}/tree.tgz"));
    assert_ne!(first, second);
}

#[test]
fn a_bootstrap_does_not_remove_the_upload_without_a_completion_receipt() {
    let text = bootstrap();
    assert!(
        !text.contains("Remove-Item -LiteralPath $tarball"),
        "{text}"
    );
    assert!(text.contains("completed.txt"), "{text}");
}
