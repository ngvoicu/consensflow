//! A text reaches `cf` whole from each shell a window or a human runs it in on Windows (issue
//! `cmd-cuts-multiline-cf-arguments`, `d25a180`): a many-line argument, and the same text on standard
//! input at `-`. The windows name `cf.exe` by its path with forward slashes, which Windows PowerShell
//! 5.1, PowerShell 7, Git Bash and cmd.exe all run; `cf setup` writes `cf.cmd` for the human's own
//! terminal, which cmd.exe runs, and a `.cmd` cannot carry a line break in an argument: cmd.exe ends a
//! command at the first one, so a reviewer's 6,250-character question reached the chief as its first
//! line, 528 characters, while `cf` said it was asked (2026-10-03). Nothing a batch file does can see
//! the rest.
//!
//! Each case runs the shell on a text in a file, and `cf ask` against a scripted board, and compares
//! the body the board received with the text. What a shell does to the text on the way is the
//! shell's, and a case says it where it is not the text's:
//!
//! - Windows PowerShell 5.1 gives a native command an argument's own double quotes bare, which splits
//!   that argument, so its argument case has none; and it pipes in the system's ANSI code page, so
//!   what it pipes to `-` is ASCII.
//! - A pipe in PowerShell and `type` end the text with `\r\n`, which `cf` takes for the text's last
//!   line ending, as it takes a heredoc's `\n`.
//! - Through `cf.cmd` the argument is cut at its first line break, and the text goes on `-`.
//!
//! The shells are looked for where Windows and Git for Windows put them, and one that is not there
//! fails its cases: a case that passed by skipping its shell would say nothing of it.
#![cfg(windows)]
// The tests' own helpers: a failure in one is the test's, and they look for the shells where
// the machine has them.
#![allow(clippy::expect_used, clippy::disallowed_methods)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use cf_board::scripted::{reply, scripted};
use common::{cf_at, own_cf};
use serde_json::json;

/// Line breaks, quotes, what cmd.exe, PowerShell and a Unix shell each expand, their operators,
/// diacritics.
const TEXT: &str = "Întrebarea 1: verific și versiunea în engleză?\n\
                    He said \"only the Romanian one\" & left; 100% sure | %PATH% ^ !x! `echo` $(whoami)\n\
                    \n\
                    PLOP-6142";

/// The same without a double quote, which Windows PowerShell 5.1 cannot hand on.
fn without_quotes() -> String {
    TEXT.replace('"', "")
}

/// What Windows PowerShell 5.1 can pipe: no double quote is a problem to a pipe, but no character
/// outside the ANSI code page survives one.
fn ascii() -> String {
    "He said \"only the Romanian one\" & left; 100% sure | %PATH% ^ !x!\n\nPLOP-6142".to_owned()
}

/// A text in a file, `cf setup`'s `cf.cmd` beside it, and a scripted board for one `cf ask`.
struct Case {
    folder: tempfile::TempDir,
    text: String,
}

impl Case {
    fn new(text: &str) -> Self {
        let folder = tempfile::tempdir().expect("a folder");
        fs::write(folder.path().join("question.md"), text).expect("the text");
        let case = Self {
            folder,
            text: text.to_owned(),
        };
        // The command `cf setup` writes for the human's terminal, for this `cf` and a home of its own.
        let home = case.at("consensflow");
        let ran = cf_at(
            &own_cf(),
            &["setup"],
            &[
                ("CONSENSFLOW_HOME", &home.to_string_lossy()),
                ("HOME", &case.at("home").to_string_lossy()),
                ("CLAUDE_CONFIG_DIR", &case.at("claude").to_string_lossy()),
            ],
            "",
        );
        assert_eq!(
            ran.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&ran.stderr)
        );
        assert!(
            case.cf_cmd().is_file(),
            "cf setup wrote no {}",
            case.cf_cmd().display()
        );
        case
    }

    fn at(&self, name: &str) -> PathBuf {
        self.folder.path().join(name)
    }

    /// The terminal's command `cf setup` wrote.
    fn cf_cmd(&self) -> PathBuf {
        self.at("consensflow").join("bin").join("cf.cmd")
    }

    /// `program args` run with the text's file, `cf.exe` and `cf.cmd` named in its environment, and
    /// the body the board received of the one `cf ask` it expects, or none.
    fn run(&self, program: &Path, args: &[&str]) -> (Option<i32>, String, Option<String>) {
        let api = scripted(vec![reply(
            201,
            json!({ "message": { "id": 5, "recipient": "chief", "state": "queued" } }),
        )]);
        // Forward slashes: Git Bash and PowerShell both take them, and the role texts name cf.exe so.
        let slashed = |path: &Path| path.to_string_lossy().replace('\\', "/");
        let native = |path: &Path| path.to_string_lossy().into_owned();
        let (text_file, exe, cmd) = (self.at("question.md"), own_cf(), self.cf_cmd());
        let ran = cf_at(
            program,
            args,
            &[
                ("CONSENSFLOW_URL", &api.url),
                ("CONSENSFLOW_TOKEN", "tok"),
                ("TEXT_FILE", &slashed(&text_file)),
                ("BIN_UNDER_TEST", &slashed(&exe)),
                ("CF_CMD", &slashed(&cmd)),
                // cmd.exe's own commands take a path with backslashes.
                ("TEXT_FILE_WIN", &native(&text_file)),
                ("CF_CMD_WIN", &native(&cmd)),
            ],
            "",
        );
        let body = api
            .received()
            .into_iter()
            .next()
            .and_then(|got| got.json())
            .and_then(|sent| sent.get("body").cloned())
            .and_then(|body| body.as_str().map(str::to_owned));
        (
            ran.status.code(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&ran.stdout),
                String::from_utf8_lossy(&ran.stderr)
            ),
            body,
        )
    }

    fn arrives_whole(&self, shell: &str, program: &Path, args: &[&str]) {
        let (code, said, body) = self.run(program, args);
        assert_eq!(
            (code, body.as_deref()),
            (Some(0), Some(self.text.as_str())),
            "{shell}: the text should reach the board whole; the shell said: {said}"
        );
    }
}

/// Windows PowerShell 5.1, where Windows keeps it.
fn powershell_5() -> PathBuf {
    let root = std::env::var_os("SystemRoot").expect("Windows names its folder");
    let exe = Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    assert!(
        exe.is_file(),
        "Windows PowerShell 5.1 is not at {}",
        exe.display()
    );
    exe
}

/// PowerShell 7, found on the PATH as the shell finds it (its MSIX has an alias there).
fn powershell_7() -> PathBuf {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|folder| folder.join("pwsh.exe"))
        .find(|exe| exe.exists())
        .expect("PowerShell 7 (pwsh.exe) is not on the PATH: install Microsoft.PowerShell")
}

/// Git Bash, where Git for Windows puts it.
fn git_bash() -> PathBuf {
    [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|exe| exe.is_file())
    .expect("Git Bash is not installed: install Git for Windows")
}

/// The words of a PowerShell that read the text from its file as UTF-8, and then `rest`.
fn read_then(rest: &str) -> String {
    format!(
        "$q = [System.IO.File]::ReadAllText($env:TEXT_FILE, [System.Text.Encoding]::UTF8); {rest}"
    )
}

const POWERSHELL: [&str; 3] = ["-NoProfile", "-NonInteractive", "-Command"];

#[test]
fn windows_powershell_5_1_gives_cf_exe_a_many_line_argument_whole() {
    let case = Case::new(&without_quotes());
    let script = read_then("& $env:BIN_UNDER_TEST ask $q");
    case.arrives_whole(
        "Windows PowerShell 5.1, an argument",
        &powershell_5(),
        &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], &script],
    );
}

#[test]
fn windows_powershell_5_1_gives_cf_exe_what_it_pipes_to_a_dash_whole() {
    let case = Case::new(&ascii());
    let script = read_then("$q | & $env:BIN_UNDER_TEST ask -");
    case.arrives_whole(
        "Windows PowerShell 5.1, standard input",
        &powershell_5(),
        &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], &script],
    );
}

#[test]
fn powershell_7_gives_cf_exe_a_many_line_argument_with_quotes_whole() {
    let case = Case::new(TEXT);
    let script = read_then("& $env:BIN_UNDER_TEST ask $q");
    case.arrives_whole(
        "PowerShell 7, an argument",
        &powershell_7(),
        &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], &script],
    );
}

#[test]
fn powershell_7_gives_cf_exe_what_it_pipes_to_a_dash_whole() {
    let case = Case::new(TEXT);
    let script = read_then("$q | & $env:BIN_UNDER_TEST ask -");
    case.arrives_whole(
        "PowerShell 7, standard input",
        &powershell_7(),
        &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], &script],
    );
}

#[test]
fn git_bash_gives_cf_exe_a_many_line_argument_with_quotes_whole() {
    let case = Case::new(TEXT);
    case.arrives_whole(
        "Git Bash, an argument",
        &git_bash(),
        &["-c", r#"q=$(cat "$TEXT_FILE"); "$BIN_UNDER_TEST" ask "$q""#],
    );
}

#[test]
fn git_bash_gives_cf_exe_what_it_pipes_to_a_dash_whole() {
    let case = Case::new(TEXT);
    case.arrives_whole(
        "Git Bash, standard input",
        &git_bash(),
        &["-c", r#"cat "$TEXT_FILE" | "$BIN_UNDER_TEST" ask -"#],
    );
}

/// cmd.exe runs a batch file: the lines of one are its own, and a command with a quote in it is
/// written once, here, without the quoting a `Command` would give it.
fn batch(case: &Case, lines: &str) -> PathBuf {
    let file = case.at("run.cmd");
    fs::write(&file, format!("@echo off\r\n{lines}\r\n")).expect("the batch file");
    file
}

#[test]
fn cmd_exe_gives_cf_exe_what_it_pipes_to_a_dash_whole() {
    // By the path with forward slashes the role texts name it by.
    let case = Case::new(TEXT);
    let file = batch(
        &case,
        r#"type "%TEXT_FILE_WIN%" | "%BIN_UNDER_TEST%" ask -"#,
    );
    case.arrives_whole(
        "cmd.exe, cf.exe, standard input",
        Path::new("cmd.exe"),
        &["/d", "/c", &file.to_string_lossy()],
    );
}

#[test]
fn cmd_exe_gives_cf_cmd_what_it_pipes_to_a_dash_whole() {
    let case = Case::new(TEXT);
    let file = batch(&case, r#"type "%TEXT_FILE_WIN%" | "%CF_CMD_WIN%" ask -"#);
    case.arrives_whole(
        "cmd.exe through cf.cmd, standard input",
        Path::new("cmd.exe"),
        &["/d", "/c", &file.to_string_lossy()],
    );
}

#[test]
fn a_many_line_argument_through_cf_cmd_is_cut_at_its_first_line_break_so_a_long_text_goes_on_a_dash(
) {
    // What Claude Code's PowerShell tool and Codex ran, `& '…\cf.cmd' ask $q`, with $q read from a
    // file, before the windows were given `cf.exe`: the question that reached the chief was its first
    // line. A `.cmd` cannot be mended; this keeps the reason `cf.exe` is what a window is told to run.
    let case = Case::new(&without_quotes());
    let script = read_then("& $env:CF_CMD_WIN ask $q");
    let (code, said, body) = case.run(
        &powershell_5(),
        &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], &script],
    );
    let first_line = case.text.lines().next().unwrap_or_default();
    assert_eq!(
        (code, body.as_deref()),
        (Some(0), Some(first_line)),
        "through cf.cmd only the first line arrives; the shell said: {said}"
    );
    assert_ne!(first_line, case.text, "a text of many lines");
}

#[test]
fn the_text_the_cases_run_on_is_many_lines_and_has_what_a_shell_has_a_meaning_for() {
    // The cases are only as strong as their text.
    assert!(TEXT.lines().count() >= 4 && TEXT.contains('"') && TEXT.contains('%'));
    assert!(!without_quotes().contains('"') && without_quotes().lines().count() >= 4);
    assert!(ascii().is_ascii() && ascii().lines().count() >= 3);
}
