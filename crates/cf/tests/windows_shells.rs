//! A text reaches `cf` whole from each shell a window or a human runs it in on Windows (issue
//! `cmd-cuts-multiline-cf-arguments`, `d25a180`): a many-line argument, and the same text on standard
//! input at `-`. The windows name `cf.exe` by its path with forward slashes, which Windows PowerShell
//! 5.1, PowerShell 7, Git Bash and cmd.exe all run. The human's own terminal runs `cf` by its name,
//! the command `cf setup` makes in the home's `bin`, which is on the `PATH` the shell finds it by.
//! That command was a `.cmd`, and a `.cmd` cannot carry a line break in an argument: cmd.exe ends a
//! command at the first one, so a reviewer's 6,250-character question reached the chief as its first
//! line, 528 characters, while `cf` said it was asked (2026-10-03), and PowerShell, which gives a
//! many-line argument whole to a program, gave it to the `.cmd` and lost it there. Nothing a batch
//! file does can see the rest. The command is a program now, and the cases below run it as each shell
//! finds it.
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
//! - cmd.exe cannot carry a line break in an argument at all, whatever it runs, so its case is the
//!   text on `-`; the usage of `cf` says so.
//!
//! The shells are looked for where Windows and Git for Windows put them, and one that is not there
//! fails its cases: a case that passed by skipping its shell would say nothing of it.
//!
//! The last cases run the line the role texts teach a PowerShell window (T-1435), as the text says
//! it: `& 'C:/Program Files/…/cf.exe' ask --file ask.md`, a `cf.exe` in a folder with a space, a
//! brief with backticks, `$( )`, quotes and non-ASCII in a file written as UTF-8, with and without
//! a byte order mark, in Windows PowerShell 5.1 and PowerShell 7. A PowerShell window is not taught
//! a heredoc, which it has none of, nor to pipe or redirect a brief: 5.1 pipes only what the ANSI
//! code page holds and its `>` writes UTF-16, which cases below show (what `cf --file` makes of
//! UTF-16 is not covered: it reads UTF-8 and a file written otherwise is the writer's to mend).
#![cfg(windows)]
// The tests' own helpers: a failure in one is the test's, and they look for the shells where
// the machine has them.
#![allow(clippy::expect_used, clippy::disallowed_methods)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_board::scripted::{reply, scripted, Reply};
use cf_engine::roles::role_instructions;
use cf_proto::cf_word::{CfWord, Shell};
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

/// A text in a file, the command `cf setup` made beside it, and a scripted board for one `cf ask`.
struct Case {
    text: String,
    folder: tempfile::TempDir,
}

impl Case {
    fn new(text: &str) -> Self {
        let folder = tempfile::tempdir().expect("a folder");
        fs::write(folder.path().join("question.md"), text).expect("the text");
        let case = Self {
            folder,
            text: text.to_owned(),
        };
        // The command `cf setup` makes for the human's terminal, for this `cf` and a home of its own.
        let home = case.at("consensflow");
        let ran = cf_at(
            &own_cf(),
            &["setup"],
            &[
                ("CONSENSFLOW_HOME", &home.to_string_lossy()),
                ("HOME", &case.at("home").to_string_lossy()),
                // The folders Windows programs read for a user are the case's too: `setup` changes
                // nobody's PATH or profile, and nothing of this machine's is read or written.
                ("USERPROFILE", &case.at("home").to_string_lossy()),
                ("APPDATA", &case.at("appdata").to_string_lossy()),
                ("LOCALAPPDATA", &case.at("localappdata").to_string_lossy()),
                ("CLAUDE_CONFIG_DIR", &case.at("claude").to_string_lossy()),
                // No harness on the PATH: `setup` prepares none of the machine's.
                ("PATH", &case.at("bin").to_string_lossy()),
            ],
            "",
        );
        assert_eq!(
            ran.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&ran.stderr)
        );
        // Whatever it is made of, `cf setup` made a command called `cf`.
        assert!(
            fs::read_dir(case.bin())
                .expect("the home's bin")
                .flatten()
                .any(|entry| entry.path().file_stem().is_some_and(|stem| stem == "cf")),
            "cf setup made no cf in {}",
            case.bin().display()
        );
        case
    }

    fn at(&self, name: &str) -> PathBuf {
        self.folder.path().join(name)
    }

    /// The folder `cf setup` made the terminal's command in: the home's `bin`.
    fn bin(&self) -> PathBuf {
        self.at("consensflow").join("bin")
    }

    /// The `PATH` of a human whose terminal found the command `cf setup` made: its folder, then
    /// the machine's, without a folder that holds a `cf` of its own. Cargo puts the folder it built
    /// this `cf` in on the `PATH` of a test, and a shell that cannot run the command `setup` made
    /// (Git Bash does not run a `.cmd` by its name) would run that one and pass for it.
    fn path_with_the_command(&self) -> String {
        let machine = std::env::var_os("PATH").unwrap_or_default();
        let holds_a_cf = |folder: &Path| {
            ["cf.exe", "cf.cmd", "cf.bat", "cf"]
                .iter()
                .any(|name| folder.join(name).exists())
        };
        let others = std::env::split_paths(&machine).filter(|folder| !holds_a_cf(folder));
        std::env::join_paths(std::iter::once(self.bin()).chain(others))
            .expect("a PATH")
            .to_string_lossy()
            .into_owned()
    }

    /// `program args` run with the text's file and `cf.exe` named in its environment, the command
    /// `cf setup` made on its `PATH`, and the body the board received of the one `cf ask` it expects,
    /// or none.
    fn run(&self, program: &Path, args: &[&str]) -> (Option<i32>, String, Option<String>) {
        self.run_against(
            reply(
                201,
                json!({ "message": { "id": 5, "recipient": "chief", "state": "queued" } }),
            ),
            program,
            args,
        )
    }

    /// [`Case::run`] against a board that answers `answer`.
    fn run_against(
        &self,
        answer: Reply,
        program: &Path,
        args: &[&str],
    ) -> (Option<i32>, String, Option<String>) {
        let api = scripted(vec![answer]);
        // Forward slashes: Git Bash and PowerShell both take them, and the role texts name cf.exe so.
        let slashed = |path: &Path| path.to_string_lossy().replace('\\', "/");
        let native = |path: &Path| path.to_string_lossy().into_owned();
        let (text_file, exe) = (self.at("question.md"), own_cf());
        let path = self.path_with_the_command();
        let ran = cf_at(
            program,
            args,
            &[
                ("CONSENSFLOW_URL", &api.url),
                ("CONSENSFLOW_TOKEN", "tok"),
                ("TEXT_FILE", &slashed(&text_file)),
                ("BIN_UNDER_TEST", &slashed(&exe)),
                ("PATH", &path),
                // cmd.exe's own commands take a path with backslashes.
                ("TEXT_FILE_WIN", &native(&text_file)),
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

// The command `cf setup` made, run by its name as each shell finds it on the `PATH`.

#[test]
fn windows_powershell_5_1_gives_the_cf_setup_made_a_many_line_argument_whole() {
    // The case that was `& cf.cmd ask $q`, which reached the board as its first line.
    let case = Case::new(&without_quotes());
    let script = read_then("cf ask $q");
    case.arrives_whole(
        "Windows PowerShell 5.1, the cf setup made, an argument",
        &powershell_5(),
        &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], &script],
    );
}

#[test]
fn windows_powershell_5_1_gives_the_cf_setup_made_what_it_pipes_to_a_dash_whole() {
    let case = Case::new(&ascii());
    let script = read_then("$q | cf ask -");
    case.arrives_whole(
        "Windows PowerShell 5.1, the cf setup made, standard input",
        &powershell_5(),
        &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], &script],
    );
}

#[test]
fn powershell_7_gives_the_cf_setup_made_a_many_line_argument_with_quotes_whole() {
    let case = Case::new(TEXT);
    let script = read_then("cf ask $q");
    case.arrives_whole(
        "PowerShell 7, the cf setup made, an argument",
        &powershell_7(),
        &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], &script],
    );
}

#[test]
fn powershell_7_gives_the_cf_setup_made_what_it_pipes_to_a_dash_whole() {
    let case = Case::new(TEXT);
    let script = read_then("$q | cf ask -");
    case.arrives_whole(
        "PowerShell 7, the cf setup made, standard input",
        &powershell_7(),
        &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], &script],
    );
}

#[test]
fn git_bash_gives_the_cf_setup_made_a_many_line_argument_with_quotes_whole() {
    let case = Case::new(TEXT);
    case.arrives_whole(
        "Git Bash, the cf setup made, an argument",
        &git_bash(),
        &["-c", r#"q=$(cat "$TEXT_FILE"); cf ask "$q""#],
    );
}

#[test]
fn git_bash_gives_the_cf_setup_made_what_it_pipes_to_a_dash_whole() {
    let case = Case::new(TEXT);
    case.arrives_whole(
        "Git Bash, the cf setup made, standard input",
        &git_bash(),
        &["-c", r#"cat "$TEXT_FILE" | cf ask -"#],
    );
}

#[test]
fn cmd_exe_gives_the_cf_setup_made_what_it_pipes_to_a_dash_whole() {
    // cmd.exe cannot carry a line break in an argument, whatever it runs; `-` is what it can.
    let case = Case::new(TEXT);
    let file = batch(&case, r#"type "%TEXT_FILE_WIN%" | cf ask -"#);
    case.arrives_whole(
        "cmd.exe, the cf setup made, standard input",
        Path::new("cmd.exe"),
        &["/d", "/c", &file.to_string_lossy()],
    );
}

#[test]
fn every_shell_finds_the_cf_setup_made_as_a_program_and_none_as_a_cmd() {
    // A shell that found a `.cmd` first would hand it the text and lose its lines; a Git Bash that
    // finds none would not run it. Each is asked what `cf` is.
    let case = Case::new(TEXT);
    let bin = case.bin();
    for expected in ["cf.exe", "consensflow.exe"] {
        assert!(bin.join(expected).is_file(), "cf setup made no {expected}");
    }
    let made: Vec<String> = fs::read_dir(&bin)
        .expect("the home's bin")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
        .collect();
    assert!(
        made.iter()
            .all(|name| !name.ends_with(".cmd") && !name.ends_with(".bat")),
        "cf setup made a script for cmd.exe: {made:?}"
    );
    let found = |shell: &str, program: &Path, args: &[&str]| {
        let (_, said, _) = case.run(program, args);
        let first = said
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .replace('\\', "/");
        assert!(
            first.to_lowercase().ends_with("/bin/cf.exe") && first.contains("consensflow"),
            "{shell} finds cf as {first:?}, not the program cf setup made in {}",
            bin.display()
        );
    };
    let ask = "(Get-Command cf).Source";
    found(
        "Windows PowerShell 5.1",
        &powershell_5(),
        &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], ask],
    );
    found(
        "PowerShell 7",
        &powershell_7(),
        &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], ask],
    );
    // Git Bash names a program without its `.exe`, and finds no `cf` that is a `.cmd`.
    found(
        "Git Bash",
        &git_bash(),
        &[
            "-c",
            r#"p=$(command -v cf) && [ -f "$p.exe" ] && cygpath -m "$p.exe""#,
        ],
    );
    let file = batch(&case, "where cf");
    found(
        "cmd.exe",
        Path::new("cmd.exe"),
        &["/d", "/c", &file.to_string_lossy()],
    );
}

#[test]
fn the_text_the_cases_run_on_is_many_lines_and_has_what_a_shell_has_a_meaning_for() {
    // The cases are only as strong as their text.
    assert!(TEXT.lines().count() >= 4 && TEXT.contains('"') && TEXT.contains('%'));
    assert!(!without_quotes().contains('"') && without_quotes().lines().count() >= 4);
    assert!(ascii().is_ascii() && ascii().lines().count() >= 3);
}

// The line the role texts teach a PowerShell window (T-1435).

/// A copy of the `cf.exe` under test in a folder with a space in its name (the default install
/// of Windows has one), and the word a PowerShell window is told it by: `& '…/cf.exe'`.
fn spaced_cf(case: &Case) -> (PathBuf, CfWord) {
    let folder = case.at("Program Files").join("ConsensFlow").join("bin");
    fs::create_dir_all(&folder).expect("the folder");
    let exe = folder.join("cf.exe");
    fs::copy(own_cf(), &exe).expect("a copy of cf.exe");
    let word = CfWord::new(&exe.to_string_lossy().replace('\\', "/"), Shell::PowerShell);
    assert!(word.as_str().starts_with("& '") && word.as_str().contains("Program Files"));
    (exe, word)
}

/// The command a role's text teaches that starts with `starts` (a `{word} ask --file `), as the
/// text says it, up to the end of its line or its closing backtick.
fn taught(role: &str, word: &CfWord, starts: &str) -> String {
    let text = role_instructions(&Env::default(), role, &[], word, None).expect("a text");
    let starts = format!("{} {starts}", word.as_str());
    let from = text
        .find(&starts)
        .unwrap_or_else(|| panic!("the {role}'s text teaches no `{starts}`: {text}"));
    let rest = &text[from..];
    rest[..rest.find(['`', '\n']).expect("the end of the line")]
        .trim_end()
        .to_owned()
}

/// The brief's file written as a harness's file tool writes one (UTF-8, a final line ending) or
/// as Windows PowerShell 5.1's `Set-Content -Encoding UTF8` does (a byte order mark first).
fn brief_file(case: &Case, name: &str, bom: bool, line_ending: &str) {
    let mut bytes = if bom {
        vec![0xEF, 0xBB, 0xBF]
    } else {
        Vec::new()
    };
    bytes.extend_from_slice(TEXT.as_bytes());
    bytes.extend_from_slice(line_ending.as_bytes());
    fs::write(case.at(name), bytes).expect("the brief");
}

/// The words PowerShell runs the taught line with: in the folder of the brief's file.
fn in_the_brief_s_folder(line: &str) -> String {
    format!("Set-Location -LiteralPath (Split-Path -Parent $env:TEXT_FILE); {line}")
}

fn a_taught_file_arrives_whole_in(shell: &str, program: &Path) {
    for (bom, line_ending) in [(false, "\n"), (true, "\n"), (false, "\r\n"), (true, "")] {
        let case = Case::new("unused");
        let (_exe, word) = spaced_cf(&case);
        brief_file(&case, "ask.md", bom, line_ending);
        let line = taught("worker", &word, "ask --file ");
        assert!(line.ends_with("ask --file ask.md"), "{line}");
        let script = in_the_brief_s_folder(&line);
        let (code, said, body) = case.run(
            program,
            &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], &script],
        );
        assert_eq!(
            (code, body.as_deref()),
            (Some(0), Some(TEXT)),
            "{shell}, a byte order mark {bom}, line ending {line_ending:?}: `{line}` said {said}"
        );
    }
}

#[test]
fn windows_powershell_5_1_runs_the_line_a_member_is_taught_and_the_brief_arrives_whole() {
    a_taught_file_arrives_whole_in("Windows PowerShell 5.1", &powershell_5());
}

#[test]
fn powershell_7_runs_the_line_a_member_is_taught_and_the_brief_arrives_whole() {
    a_taught_file_arrives_whole_in("PowerShell 7", &powershell_7());
}

/// The chief's line, `task add --tier standard --file brief.md`, against a board that answers
/// as it does a task.
fn the_chief_s_line_arrives_whole_in(shell: &str, program: &Path) {
    let case = Case::new("unused");
    let (_exe, word) = spaced_cf(&case);
    brief_file(&case, "brief.md", false, "\n");
    let line = taught("chief", &word, "task add --tier standard --file ");
    assert!(
        line.ends_with("task add --tier standard --file brief.md"),
        "{line}"
    );
    let task = json!({
        "number": 3, "state": "open", "assignee": null, "requester": "chief",
        "title": "Parser", "blockedBy": [], "pool": "worker", "tier": "standard",
    });
    let answer = reply(
        201,
        json!({ "task": task, "message": null, "gated": false }),
    );
    let (code, said, body) = case.run_against(
        answer,
        program,
        &[
            POWERSHELL[0],
            POWERSHELL[1],
            POWERSHELL[2],
            &in_the_brief_s_folder(&line),
        ],
    );
    assert_eq!(
        (code, body.as_deref()),
        (Some(0), Some(TEXT)),
        "{shell}: `{line}` said {said}"
    );
}

#[test]
fn windows_powershell_5_1_runs_the_line_the_chief_is_taught_and_the_brief_arrives_whole() {
    the_chief_s_line_arrives_whole_in("Windows PowerShell 5.1", &powershell_5());
}

#[test]
fn powershell_7_runs_the_line_the_chief_is_taught_and_the_brief_arrives_whole() {
    the_chief_s_line_arrives_whole_in("PowerShell 7", &powershell_7());
}

/// The texts say Windows PowerShell 5.1's `>` writes UTF-16 and PowerShell 7's does not.
#[test]
fn windows_powershell_5_1_redirects_to_a_file_as_utf_16_and_powershell_7_as_utf_8() {
    for (shell, program, utf_16) in [
        ("Windows PowerShell 5.1", powershell_5(), true),
        ("PowerShell 7", powershell_7(), false),
    ] {
        let case = Case::new("unused");
        let script = "'Întrebarea' > (Join-Path (Split-Path -Parent $env:TEXT_FILE) 'out.txt')";
        let (code, said, _) = case.run(
            &program,
            &[POWERSHELL[0], POWERSHELL[1], POWERSHELL[2], script],
        );
        assert_eq!(code, Some(0), "{shell}: {said}");
        let bytes = fs::read(case.at("out.txt")).expect("the redirected file");
        assert_eq!(
            bytes.starts_with(&[0xFF, 0xFE]),
            utf_16,
            "{shell} wrote {bytes:?}"
        );
        assert!(
            utf_16 || bytes.starts_with("Întrebarea".as_bytes()),
            "{shell}: {bytes:?}"
        );
    }
}
