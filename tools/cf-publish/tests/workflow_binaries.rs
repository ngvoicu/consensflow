//! A rule over every workflow: a job that runs a binary it downloaded from
//! another job of the run runs it on the system that job built it on. A binary
//! built on one system does not run on another: the sign job of round 2
//! downloaded the publisher built on Linux and ran it on macOS, which a release
//! would have found only at its first tag (T-818).
//!
//! What a run is, read off the steps of every job (T-831 P3-2, after the rule
//! once let each of these by in silence): a step that makes a file of a
//! downloaded artifact's folder executable (`chmod` of any kind, `install`), or
//! names one as the command it starts (`"$RUNNER_TEMP/tool/cf" …`, `& $tool`,
//! `./cf` after a `cd` into the folder, `VAR=$("$tool" …)`). The folder is the
//! one the download's `path:` names, however the step spells the runner's folders
//! (`${{ runner.temp }}`, `$RUNNER_TEMP`, `${RUNNER_TEMP}`, `$env:RUNNER_TEMP`,
//! `${{ github.workspace }}`, `$GITHUB_WORKSPACE`, either slash), and a path
//! through variables is followed. A line that only reads a downloaded folder
//! (`cp`, `shasum`, `find`) is no run: the sign job holds the Windows files on a
//! Mac to hash and sign them.
//!
//! The rule never skips what it cannot read: a `download-artifact` it cannot
//! tie to one artifact and one folder (a `pattern:`, no `path:`), a line of the
//! text it counts and does not find as a step, a workflow a job calls that it
//! cannot find, a run on a runner whose system it does not know, are failures.
//! Each workflow's own jobs are read once, and a job that calls a reusable
//! workflow (`uses: ./.github/workflows/…`) holds nothing else: the reusable
//! workflow's jobs are its file's.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};

use cf_publish::testing::workflow::{files, job, job_names, live, read, steps, StepText};

/// The workflows to read, by file name: the repository's, or a fixture's.
type Workflows = BTreeMap<String, String>;

/// The repository's own workflows.
fn repository() -> Workflows {
    files()
        .into_iter()
        .map(|file| {
            let text = read(&file);
            (file, text)
        })
        .collect()
}

/// The system a `runs-on` names: the runner's family, as GitHub names its images; none where it
/// names something else (a matrix, a self-hosted runner).
fn system(runs_on: &str) -> Option<&'static str> {
    let image = runs_on.trim().trim_matches(|c| c == '\'' || c == '"');
    if image.starts_with("ubuntu") {
        Some("linux")
    } else if image.starts_with("macos") {
        Some("macos")
    } else if image.starts_with("windows") {
        Some("windows")
    } else {
        None
    }
}

/// `value` without the comment after it.
fn plain(value: &str) -> String {
    let value = value.find(" #").map_or(value, |at| &value[..at]);
    value.trim().to_string()
}

/// The value of `key:` at the job's own level (four spaces in).
fn job_key(job: &str, key: &str) -> Option<String> {
    job.lines()
        .find_map(|line| line.strip_prefix(&format!("    {key}: ")))
        .map(plain)
}

/// How many spaces a line is indented by.
fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The lines under the key `key:` that stands `at` spaces in `text`, up to the first line indented
/// no more than it; none where it has no such key. Comments and blank lines are left out.
fn under(text: &str, at: usize, key: &str) -> Vec<String> {
    let live = live(text);
    let mut lines = live
        .lines()
        .skip_while(|line| indent(line) != at || plain(line) != format!("{key}:"));
    if lines.next().is_none() {
        return Vec::new();
    }
    lines
        .take_while(|line| line.trim().is_empty() || indent(line) > at)
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect()
}

/// The value of a `name: value` line, as YAML reads it: without a comment after it, or quotes.
fn scalar_of(line: &str) -> Option<(String, String)> {
    let (name, value) = line.trim().split_once(':')?;
    let value = plain(value);
    Some((
        name.trim().to_string(),
        value.trim_matches(['"', '\'']).to_string(),
    ))
}

/// The `env:` of the job, or of the workflow where `at` is 0: its variables, in order.
fn env_of(text: &str, at: usize) -> Vec<(String, String)> {
    under(text, at, "env")
        .iter()
        .filter(|line| indent(line) == at + 2)
        .filter_map(|line| scalar_of(line))
        .collect()
}

/// The shell and the working directory `defaults: run:` of the job, or of the workflow where `at`
/// is 0, gives its steps.
fn defaults_of(text: &str, at: usize) -> (Option<String>, Option<String>) {
    let defaults = under(text, at, "defaults").join("\n");
    let run = under(&defaults, at + 2, "run");
    let find = |key: &str| {
        run.iter()
            .filter(|line| indent(line) == at + 4)
            .filter_map(|line| scalar_of(line))
            .find_map(|(name, value)| (name == key).then_some(value))
    };
    (find("shell"), find("working-directory"))
}

/// The part of a workflow's text before its jobs: its own `env:` and `defaults:`.
fn before_the_jobs(text: &str) -> &str {
    text.split("\njobs:").next().unwrap_or(text)
}

/// What a step's `with:` gives `key`.
fn with(step: &StepText, key: &str) -> Option<String> {
    let text = step.text();
    let mut lines = text.lines().skip_while(|line| *line != "        with:");
    lines.next()?;
    lines
        .take_while(|line| line.starts_with("          "))
        .find_map(|line| line.trim().strip_prefix(&format!("{key}:")))
        .map(|value| plain(value).trim_matches(['"', '\'']).to_string())
}

/// The folder an artifact is downloaded into.
struct Download {
    artifact: String,
    folder: String,
}

/// The shell of a step, as far as the rule needs it: how it quotes and escapes.
#[derive(Clone, Copy, PartialEq)]
enum Shell {
    Bash,
    Pwsh,
    Cmd,
}

/// A step's script, with what it runs in.
struct Script {
    text: String,
    shell: Shell,
    directory: Option<String>,
    env: Vec<(String, String)>,
}

/// A job as the rule reads it: the system it runs on, the artifacts it uploads, the folders it
/// downloads named artifacts into, and the scripts it runs.
struct Job {
    workflow: String,
    name: String,
    system: Option<&'static str>,
    uploads: Vec<String>,
    downloads: Vec<Download>,
    scripts: Vec<Script>,
}

/// What a workflow's text holds: its jobs, the workflows its jobs call, and what it holds that
/// the rule could not read.
struct Parsed {
    jobs: Vec<Job>,
    calls: Vec<String>,
    unread: Vec<String>,
}

/// How many of `text`'s lines, comments left out, hold `word`.
fn counted(text: &str, word: &str) -> usize {
    live(text)
        .lines()
        .filter(|line| line.contains(word))
        .count()
}

fn parse(file: &str, text: &str) -> Parsed {
    let mut parsed = Parsed {
        jobs: Vec::new(),
        calls: Vec::new(),
        unread: Vec::new(),
    };
    let (mut uploads, mut runners) = (0, 0);
    let before = before_the_jobs(text);
    let (top_shell, top_directory) = defaults_of(before, 0);
    let top_env = env_of(before, 0);
    for name in job_names(text) {
        let body = job(text, &name);
        if let Some(called) = job_key(&body, "uses") {
            if let Some(called) = called.strip_prefix("./.github/workflows/") {
                parsed.calls.push(called.to_string());
            }
            continue;
        }
        let runs_on = job_key(&body, "runs-on");
        let (job_shell, job_directory) = defaults_of(&body, 4);
        let job_env = env_of(&body, 4);
        runners += usize::from(runs_on.is_some());
        let system = runs_on
            .as_deref()
            .filter(|on| !on.contains("${{"))
            .and_then(system);
        let mut job = Job {
            workflow: file.to_string(),
            name: name.clone(),
            system,
            uploads: Vec::new(),
            downloads: Vec::new(),
            scripts: Vec::new(),
        };
        for step in steps(&body) {
            let text = step.text();
            if text.contains("actions/upload-artifact@") {
                uploads += 1;
                job.uploads
                    .push(with(&step, "name").unwrap_or_else(|| "artifact".to_string()));
            } else if text.contains("actions/download-artifact@") {
                match (with(&step, "name"), with(&step, "path")) {
                    (Some(artifact), Some(path)) if with(&step, "pattern").is_none() => {
                        job.downloads.push(Download {
                            artifact,
                            folder: spelled(&from_the_workspace(&expressions(&path))),
                        });
                    }
                    _ => parsed.unread.push(format!(
                        "{file}:{name} downloads an artifact this rule cannot attribute (a `pattern:`, or no `name:` or `path:`): download each artifact by its name into a folder of its own"
                    )),
                }
            } else if step.runs() {
                let shell = step
                    .field("shell")
                    .map(|shell| plain(&shell))
                    .or_else(|| job_shell.clone())
                    .or_else(|| top_shell.clone());
                let shell = match shell
                    .as_deref()
                    .and_then(|shell| shell.split_whitespace().next())
                {
                    Some("bash" | "sh") => Shell::Bash,
                    Some("pwsh" | "powershell") => Shell::Pwsh,
                    Some("cmd") => Shell::Cmd,
                    _ if system == Some("windows") => Shell::Pwsh,
                    _ => Shell::Bash,
                };
                let mut env = top_env.clone();
                env.extend(job_env.iter().cloned());
                env.extend(step.env());
                job.scripts.push(Script {
                    text: step.script(),
                    shell,
                    directory: step
                        .field("working-directory")
                        .map(|dir| plain(&dir))
                        .or_else(|| job_directory.clone())
                        .or_else(|| top_directory.clone()),
                    env,
                });
            }
        }
        parsed.jobs.push(job);
    }
    // Every line of the text that names what the rule is about is found as a step of a job it
    // reads, or the rule says it does not read the layout: a skipped line is a rule that holds
    // nothing.
    let layout = |found: usize, word: &str, what: &str| {
        let lines = counted(text, word);
        (lines != found).then(|| {
            format!("{file} has {lines} lines with {word}, and the rule reads {found} {what} as steps of a job: the rule does not read this layout")
        })
    };
    let attributed: usize = parsed.jobs.iter().map(|job| job.downloads.len()).sum();
    let found = attributed + parsed.unread.len();
    parsed
        .unread
        .extend(layout(found, "actions/download-artifact@", "downloads"));
    parsed
        .unread
        .extend(layout(uploads, "actions/upload-artifact@", "uploads"));
    let runs_on_lines = live(text)
        .lines()
        .filter(|line| line.starts_with("    runs-on:"))
        .count();
    if runs_on_lines != runners {
        parsed.unread.push(format!(
            "{file} has {runs_on_lines} jobs with a runs-on and the rule reads {runners}: the rule does not read this layout"
        ));
    }
    parsed
}

// --- reading a script ----------------------------------------------------------------------

/// `text` with every occurrence of `from`, in any case, replaced by `to`.
fn replace_ci(text: &str, from: &str, to: &str) -> String {
    let (lower, from_lower) = (text.to_ascii_lowercase(), from.to_ascii_lowercase());
    let mut out = String::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find(&from_lower) {
        out.push_str(&text[at..at + found]);
        out.push_str(to);
        at += found + from.len();
    }
    out.push_str(&text[at..]);
    out
}

fn is_name(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `text` with the runner's folders spelled each as the one bash variable that names it
/// (`${{ runner.temp }}`, `%RUNNER_TEMP%`, `$env:RUNNER_TEMP`, `${RUNNER_TEMP}` are `$RUNNER_TEMP`),
/// any other `${{ … }}` an opaque variable of its own, and `${NAME}`, `${env:NAME}` and
/// `$env:NAME` as `$NAME`.
fn expressions(text: &str) -> String {
    let mut text = text.to_string();
    for (from, to) in [
        ("${{ runner.temp }}", "$RUNNER_TEMP"),
        ("${{runner.temp}}", "$RUNNER_TEMP"),
        ("${{ github.workspace }}", "$GITHUB_WORKSPACE"),
        ("${{github.workspace}}", "$GITHUB_WORKSPACE"),
        ("%RUNNER_TEMP%", "$RUNNER_TEMP"),
        ("%GITHUB_WORKSPACE%", "$GITHUB_WORKSPACE"),
        ("${env:", "${"),
        ("$env:", "$"),
    ] {
        text = replace_ci(&text, from, to);
    }
    while let Some(start) = text.find("${{") {
        let end = text[start..]
            .find("}}")
            .map_or(text.len(), |at| start + at + 2);
        text.replace_range(start..end, "$EXPRESSION");
    }
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) if end > 0 && after[..end].chars().all(is_name) => {
                out.push('$');
                out.push_str(&after[..end]);
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str("${");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    // The two variables GitHub sets are the same whatever their case: PowerShell and cmd say so.
    for name in ["RUNNER_TEMP", "GITHUB_WORKSPACE"] {
        out = replace_ci(&out, &format!("${name}"), &format!("${name}"));
    }
    out
}

/// A path as download-artifact reads it: one that is not from a root is the workspace's.
fn from_the_workspace(path: &str) -> String {
    if absolute(path.trim()) {
        path.to_string()
    } else {
        format!("$GITHUB_WORKSPACE/{path}")
    }
}

/// `word` with each variable it names that is bound replaced by its value.
fn expand(word: &str, bindings: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = word;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find(|c| !is_name(c)).unwrap_or(after.len());
        match bindings.get(&after[..end]) {
            Some(value) if end > 0 => out.push_str(value),
            _ => {
                out.push('$');
                out.push_str(&after[..end]);
            }
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// A path as the rule compares them: `/` between folders, no `.` or `..` folder, no doubled or
/// trailing `/`.
fn spelled(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let mut kept: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "." | "" if !kept.is_empty() => {}
            ".." if kept.len() > 1 && kept.last() != Some(&"..") => {
                kept.pop();
            }
            part => kept.push(part),
        }
    }
    if kept == [""] && path.starts_with('/') {
        return "/".to_string();
    }
    kept.join("/")
}

/// Whether `path` is `folder` or in it, as Windows takes names: whatever the case.
fn inside(path: &str, folder: &str) -> bool {
    let (path, folder) = (path.to_lowercase(), folder.to_lowercase());
    path == folder || path.starts_with(&format!("{folder}/"))
}

/// Whether a path is given from a root: a variable, `/`, a drive, or the home folder.
fn absolute(path: &str) -> bool {
    path.starts_with(['$', '/', '~']) || path.as_bytes().get(1) == Some(&b':')
}

/// One statement of a script: its words, their quotes taken off, and whether the one before it
/// was piped into it.
struct Statement {
    words: Vec<String>,
    piped: bool,
}

/// The statements of a script, split at a line's end, `;`, `&`, `&&`, `|`, `||`, a parenthesis and
/// a backtick (and, in PowerShell, a brace: the statements of a block are the block's), outside quotes; the comments left out, and PowerShell's `&` that starts a statement
/// (the call operator) with them. Quotes are as the shell reads them: cmd has no single quote, and
/// only bash takes a backslash for an escape.
fn statements(script: &str, shell: Shell) -> Vec<Statement> {
    let bash = shell == Shell::Bash;
    let text = if bash {
        script.replace("\\\n", " ")
    } else {
        script.replace("`\n", " ")
    };
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<Statement> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut piped = false;
    let mut quote: Option<char> = None;
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if let Some(open) = quote {
            if c == open {
                quote = None;
            } else if bash
                && open == '"'
                && c == '\\'
                && matches!(chars.get(at + 1), Some('$' | '`' | '"' | '\\'))
            {
                at += 1;
                word.push(chars[at]);
            } else {
                word.push(c);
            }
        } else {
            match c {
                '"' => {
                    quote = Some(c);
                    started = true;
                }
                '\'' if shell != Shell::Cmd => {
                    quote = Some(c);
                    started = true;
                }
                '\\' if bash && at + 1 < chars.len() => {
                    at += 1;
                    word.push(chars[at]);
                    started = true;
                }
                '#' if !started && shell != Shell::Cmd => {
                    while at + 1 < chars.len() && chars[at + 1] != '\n' {
                        at += 1;
                    }
                }
                c if matches!(
                    c,
                    ' ' | '\t' | '\r' | '\n' | ';' | '|' | '(' | ')' | '`' | '&'
                ) || (shell == Shell::Pwsh && matches!(c, '{' | '}')) =>
                {
                    if started {
                        words.push(std::mem::take(&mut word));
                        started = false;
                    }
                    let call = c == '&'
                        && words.is_empty()
                        && chars.get(at + 1).is_some_and(|next| next.is_whitespace());
                    if !matches!(c, ' ' | '\t' | '\r') && !call {
                        // A single `|` pipes into the next statement; `||` does not.
                        let pipe = c == '|'
                            && chars.get(at + 1) != Some(&'|')
                            && (at == 0 || chars[at - 1] != '|');
                        if !words.is_empty() {
                            out.push(Statement {
                                words: std::mem::take(&mut words),
                                piped,
                            });
                            piped = pipe;
                        } else if c != '\n' {
                            piped = pipe;
                        }
                    }
                }
                _ => {
                    word.push(c);
                    started = true;
                }
            }
        }
        at += 1;
    }
    if started {
        words.push(word);
    }
    if !words.is_empty() {
        out.push(Statement { words, piped });
    }
    out
}

/// `text` of a cmd script with its `%NAME%` as the variables bash names: `$NAME`.
fn cmd_variables(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 && after[..end].chars().all(is_name) => {
                out.push('$');
                out.push_str(&after[..end]);
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Words that start a statement and are not its command, or run what follows.
const BEFORE_A_COMMAND: [&str; 20] = [
    "if", "then", "else", "elif", "do", "while", "until", "!", "{", "}", ".", "time", "exec",
    "sudo", "nohup", "command", "env", "export", "local", "declare",
];

/// Statements that are not a command at all.
const NO_COMMAND: [&str; 8] = [
    "for", "case", "select", "function", "esac", "fi", "done", "in",
];

/// Programs that run the files they are given, or are made to be run through.
const RUNS_AN_ARGUMENT: [&str; 7] = [
    "install",
    "start-process",
    "start",
    "saps",
    "invoke-item",
    "ii",
    "call",
];

/// Programs that change the folder the next lines work in.
const CHANGES_FOLDER: [&str; 5] = ["cd", "pushd", "chdir", "set-location", "sl"];

/// Programs that only read or move what they are given, which is no run of it: what `xargs` and
/// `find -exec` start with one of these is not run.
const READS: [&str; 40] = [
    "shasum",
    "sha256sum",
    "sha1sum",
    "md5",
    "md5sum",
    "cat",
    "cp",
    "mv",
    "ls",
    "rm",
    "echo",
    "printf",
    "grep",
    "egrep",
    "wc",
    "stat",
    "file",
    "head",
    "tail",
    "tar",
    "zip",
    "unzip",
    "du",
    "test",
    "true",
    "touch",
    "mkdir",
    "rmdir",
    "dir",
    "type",
    "copy",
    "xcopy",
    "move",
    "del",
    "certutil",
    "findstr",
    "get-filehash",
    "copy-item",
    "get-childitem",
    "remove-item",
];

/// Programs that run the command they are given: for each, the options that are given a value of
/// their own, and how many words come between its options and the command.
const WRAPPERS: [(&str, &[&str], usize); 9] = [
    ("timeout", &["-s", "--signal", "-k", "--kill-after"], 1),
    ("nice", &["-n", "--adjustment"], 0),
    ("caffeinate", &["-t", "-w"], 0),
    ("arch", &["-arch"], 0),
    ("setsid", &[], 0),
    (
        "flock",
        &["-w", "--timeout", "-E", "--conflict-exit-code"],
        1,
    ),
    ("stdbuf", &["-i", "-o", "-e"], 0),
    (
        "xvfb-run",
        &[
            "-s",
            "--server-args",
            "-n",
            "--server-num",
            "-e",
            "--error-file",
            "-f",
            "--auth-file",
            "-w",
            "--wait",
        ],
        0,
    ),
    ("script", &["-t"], 1),
];

/// `xargs` options that are given a value of their own.
const XARGS_TAKES: [&str; 9] = ["-I", "-n", "-P", "-L", "-s", "-d", "-E", "-a", "-J"];

/// The extensions of what cmd runs by a bare name, from the folder it is in.
const CMD_EXTENSIONS: [&str; 4] = [".exe", ".bat", ".cmd", ".com"];

/// `name=value` as bash assigns it.
fn assignment(word: &str) -> Option<(&str, &str)> {
    let (name, value) = word.split_once('=')?;
    (!name.is_empty()
        && name.chars().all(is_name)
        && !name.starts_with(|c: char| c.is_ascii_digit()))
    .then_some((name, value))
}

/// PowerShell's `$name = value`, `$name = Join-Path a b`: the name and what it is bound to.
fn powershell_assignment(
    raw: &[String],
    bindings: &BTreeMap<String, String>,
) -> Option<(String, String)> {
    let name = raw.first()?.strip_prefix('$')?;
    if name.is_empty() || !name.chars().all(is_name) || raw.get(1).map(String::as_str) != Some("=")
    {
        return None;
    }
    let rest: Vec<String> = raw[2..].iter().map(|word| expand(word, bindings)).collect();
    let value = match rest.as_slice() {
        [join, a, b, ..] if join.eq_ignore_ascii_case("join-path") => format!("{a}/{b}"),
        [value, ..] => value.clone(),
        [] => return None,
    };
    Some((name.to_string(), spelled(&value)))
}

/// Whether `word` is what `chmod` is given as its mode (or an option), not a file.
fn is_mode(word: &str) -> bool {
    word.starts_with('-') || word.chars().all(|c| "ugoa+-=rwxXst,01234567".contains(c))
}

/// Whether `arg` names the PowerShell parameter `name` (`-command`), which may be given as any
/// prefix of it that is more than the dash.
fn is_parameter(arg: &str, name: &str) -> bool {
    let arg = arg.to_lowercase();
    arg.len() > 1 && name.starts_with(&arg)
}

/// The name of a program as the rule knows it: its last part, in lower case, without `.exe`.
fn program_of(command: &str) -> String {
    let lowered = command.replace('\\', "/").to_lowercase();
    let last = lowered.rsplit('/').next().unwrap_or(&lowered);
    last.strip_suffix(".exe").unwrap_or(last).to_string()
}

/// Where a script is read: the variables it has bound, the folder it is in, the downloads whose
/// files it runs, and the downloads the earlier statements of the pipeline it is in name.
struct Walk<'a> {
    downloads: &'a [Download],
    bindings: BTreeMap<String, String>,
    cwd: Option<String>,
    found: BTreeSet<usize>,
    named: BTreeSet<usize>,
    /// The relative paths run after a `cd` into a folder the rule cannot resolve.
    unplaced: Vec<String>,
}

impl Walk<'_> {
    /// `path` from the root: a path from a root as it is, any other from the folder the script is in.
    fn at(&self, path: &str) -> Option<String> {
        if absolute(path) {
            Some(spelled(path))
        } else {
            self.cwd
                .as_ref()
                .map(|cwd| spelled(&format!("{cwd}/{path}")))
        }
    }

    /// The downloads `path` is in the folder of, or in a folder of.
    fn inside_of(&self, path: &str) -> BTreeSet<usize> {
        let Some(path) = self.at(path) else {
            return BTreeSet::new();
        };
        (0..self.downloads.len())
            .filter(|index| inside(&path, &self.downloads[*index].folder))
            .collect()
    }

    /// A file at `path` is run. A path from no root, where the folder the script is in is not
    /// known (a `cd` into a variable the rule does not know), is not placed: that is said, never
    /// taken for a file of nothing.
    fn hit(&mut self, path: &str) {
        if !absolute(path) && self.cwd.is_none() {
            self.unplaced.push(path.to_string());
            return;
        }
        let found = self.inside_of(path);
        self.found.extend(found);
    }

    /// A script's text, as the shell reads it.
    fn text(&mut self, text: &str, shell: Shell) {
        let text = if shell == Shell::Cmd {
            cmd_variables(text)
        } else {
            text.to_string()
        };
        let mut previous: Vec<String> = Vec::new();
        for statement in statements(&expressions(&text), shell) {
            if !statement.piped {
                self.named.clear();
            }
            // A string piped into Invoke-Expression is run as the text it is.
            if statement.piped
                && statement.words.len() == 1
                && ["invoke-expression", "iex"].contains(&program_of(&statement.words[0]).as_str())
                && previous.len() == 1
            {
                self.text(&previous[0], Shell::Pwsh);
            }
            previous = statement.words.clone();
            self.statement(statement.words, shell);
        }
    }

    /// One statement.
    fn statement(&mut self, raw: Vec<String>, shell: Shell) {
        if let Some((name, value)) = powershell_assignment(&raw, &self.bindings) {
            self.bindings.insert(name, value);
            return;
        }
        if shell == Shell::Cmd
            && raw
                .first()
                .is_some_and(|set| set.eq_ignore_ascii_case("set"))
        {
            if let Some((name, value)) = raw.get(1).and_then(|word| word.split_once('=')) {
                let value = spelled(&expand(value, &self.bindings));
                self.bindings.insert(name.to_string(), value);
            }
            return;
        }
        let words: Vec<String> = raw
            .iter()
            .map(|word| expand(word, &self.bindings))
            .collect();
        let mut start = 0;
        let mut wrapped = false;
        while start < words.len() {
            let word = words[start].as_str();
            if BEFORE_A_COMMAND.contains(&word) {
                wrapped = true;
            } else if let Some((name, value)) = assignment(word) {
                self.bindings.insert(name.to_string(), spelled(value));
            } else if !(wrapped && word.starts_with('-')) {
                break;
            }
            start += 1;
        }
        if let Some(command) = words.get(start) {
            self.command(command, &words[start + 1..], shell);
        }
        // What this statement names is what the statements piped from it are given.
        let path_like = |word: &String| word.contains(['/', '\\']) || word.starts_with(['$', '.']);
        let mut named: BTreeSet<usize> = words
            .iter()
            .filter(|word| path_like(word))
            .flat_map(|word| self.inside_of(word))
            .collect();
        let program = words.get(start).map(|command| program_of(command));
        if ["ls", "find", "dir", "tree"].contains(&program.as_deref().unwrap_or(""))
            && !words[start..].iter().any(path_like)
        {
            named.extend(self.cwd.iter().flat_map(|cwd| self.inside_of(cwd)));
        }
        self.named.extend(named);
    }

    /// A command and its arguments.
    fn command(&mut self, command: &str, args: &[String], shell: Shell) {
        let program = program_of(command);
        let program = program.as_str();
        if NO_COMMAND.contains(&program) {
            return;
        }
        if let Some((_, takes, before)) = WRAPPERS.iter().find(|(name, ..)| *name == program) {
            // flock and script may be given the command as a string.
            let string = args
                .iter()
                .position(|arg| arg == "-c" || arg == "--command")
                .and_then(|at| args.get(at + 1));
            if let Some(string) = string.filter(|_| ["flock", "script"].contains(&program)) {
                self.text(string, Shell::Bash);
                return;
            }
            let mut at = 0;
            while at < args.len() && args[at].starts_with('-') {
                at += if takes.contains(&args[at].as_str()) {
                    2
                } else {
                    1
                };
            }
            if let Some(wrapped) = args
                .get(at + before..)
                .filter(|wrapped| !wrapped.is_empty())
            {
                self.command(&wrapped[0], &wrapped[1..], shell);
            }
            return;
        }
        match program {
            "eval" => self.text(&args.join(" "), shell),
            "invoke-expression" | "iex" => {
                // Its one parameter, -Command, may be named.
                let text = match args.first() {
                    Some(first) if is_parameter(first, "-command") => &args[1..],
                    _ => args,
                };
                self.text(&text.join(" "), Shell::Pwsh);
            }
            "invoke-command" | "icm" => {
                // A script file given to run is run (a block, in braces, is read as statements).
                let file = args.iter().position(|arg| is_parameter(arg, "-filepath"));
                if let Some(script) = file.and_then(|at| args.get(at + 1)) {
                    self.hit(script);
                }
            }
            "sh" | "bash" | "dash" | "zsh" | "ksh" => {
                let flag = args.iter().position(|arg| {
                    arg.starts_with('-') && !arg.starts_with("--") && arg.contains('c')
                });
                if let Some(script) = flag.and_then(|at| args.get(at + 1)) {
                    self.text(script, Shell::Bash);
                }
            }
            "pwsh" | "powershell" => {
                let flag = args
                    .iter()
                    .position(|arg| ["-c", "-command"].contains(&arg.to_lowercase().as_str()));
                if let Some(at) = flag {
                    self.text(&args[at + 1..].join(" "), Shell::Pwsh);
                }
                // A script given to run is run.
                let file = args
                    .iter()
                    .position(|arg| ["-file", "-f"].contains(&arg.to_lowercase().as_str()));
                if let Some(script) = file.and_then(|at| args.get(at + 1)) {
                    self.hit(script);
                }
            }
            "cmd" => {
                let flag = args.iter().position(|arg| {
                    ["/c", "/k", "//c", "//k"].contains(&arg.to_lowercase().as_str())
                });
                if let Some(at) = flag {
                    self.text(&args[at + 1..].join(" "), Shell::Cmd);
                }
            }
            "xargs" => self.xargs(args, shell),
            "find" => self.find(args, shell),
            "chmod" => args
                .iter()
                .filter(|arg| !is_mode(arg))
                .for_each(|arg| self.hit(arg)),
            program if CHANGES_FOLDER.contains(&program) => {
                let dir = args.iter().find(|arg| !arg.eq_ignore_ascii_case("/d"));
                self.cwd = dir.and_then(|dir| {
                    let dir = spelled(dir);
                    let unknown = dir.contains('$')
                        && !dir.starts_with("$RUNNER_TEMP")
                        && !dir.starts_with("$GITHUB_WORKSPACE");
                    if unknown {
                        None
                    } else {
                        self.at(&dir)
                    }
                });
            }
            program if RUNS_AN_ARGUMENT.contains(&program) => args
                .iter()
                .filter(|arg| !arg.is_empty() && !arg.starts_with('-'))
                .filter(|arg| shell != Shell::Cmd || !arg.starts_with('/'))
                .for_each(|arg| self.hit(arg)),
            _ => {
                let bare = shell == Shell::Cmd
                    && CMD_EXTENSIONS
                        .iter()
                        .any(|extension| command.to_lowercase().ends_with(extension));
                if command.contains(['/', '\\']) || command.starts_with('$') || bare {
                    self.hit(command);
                }
            }
        }
    }

    /// `xargs [options] command args`: the command is run, with what the pipeline gave it (the
    /// folders its earlier statements name) among its arguments, unless it only reads.
    fn xargs(&mut self, args: &[String], shell: Shell) {
        let mut at = 0;
        while at < args.len() && args[at].starts_with('-') {
            at += if XARGS_TAKES.contains(&args[at].as_str()) {
                2
            } else {
                1
            };
        }
        let Some(command) = args.get(at) else {
            return;
        };
        let given = self.named.clone();
        self.statement(args[at..].to_vec(), shell);
        if !READS.contains(&program_of(command).as_str()) {
            self.found.extend(given);
        }
    }

    /// `find roots … -exec command … ;`: the command is run, for each file the roots hold, unless
    /// it only reads.
    fn find(&mut self, args: &[String], shell: Shell) {
        let roots: Vec<&String> = args
            .iter()
            .take_while(|arg| !arg.starts_with('-') && !["(", "!", ","].contains(&arg.as_str()))
            .collect();
        let mut held: BTreeSet<usize> =
            roots.iter().flat_map(|root| self.inside_of(root)).collect();
        if roots.is_empty() {
            held.extend(self.cwd.iter().flat_map(|cwd| self.inside_of(cwd)));
        }
        let mut at = 0;
        while at < args.len() {
            if ["-exec", "-execdir", "-ok", "-okdir"].contains(&args[at].as_str()) {
                let end = args[at + 1..]
                    .iter()
                    .position(|arg| arg == ";" || arg == "+")
                    .map_or(args.len(), |end| at + 1 + end);
                if let Some(command) = args.get(at + 1).filter(|_| end > at + 1) {
                    self.statement(args[at + 1..end].to_vec(), shell);
                    if !READS.contains(&program_of(command).as_str()) {
                        self.found.extend(held.iter().copied());
                    }
                }
                at = end;
            }
            at += 1;
        }
    }
}

/// The downloads (by their place in `downloads`) whose files the script makes run.
fn runs_of(script: &Script, downloads: &[Download]) -> (BTreeSet<usize>, Vec<String>) {
    let mut walk = Walk {
        downloads,
        bindings: BTreeMap::new(),
        cwd: None,
        found: BTreeSet::new(),
        named: BTreeSet::new(),
        unplaced: Vec::new(),
    };
    for (name, value) in &script.env {
        let value = value.trim_matches(['"', '\'']);
        let value = spelled(&expand(&expressions(value), &walk.bindings));
        walk.bindings.insert(name.clone(), value);
    }
    walk.cwd = Some(script.directory.as_deref().map_or_else(
        || "$GITHUB_WORKSPACE".to_string(),
        |dir| spelled(&expand(&expressions(dir), &walk.bindings)),
    ));
    walk.text(&script.text, script.shell);
    (walk.found, walk.unplaced)
}

// --- the rule ------------------------------------------------------------------------------

/// What the rule found in workflows: the downloaded binaries that are run, and what is wrong.
struct Report {
    runs: usize,
    problems: Vec<String>,
}

fn check(workflows: &Workflows) -> Report {
    let parsed: BTreeMap<&str, Parsed> = workflows
        .iter()
        .map(|(file, text)| (file.as_str(), parse(file, text)))
        .collect();
    let mut problems = Vec::new();
    for (file, parsed) in &parsed {
        problems.extend(parsed.unread.iter().cloned());
        for called in &parsed.calls {
            if !workflows.contains_key(called) {
                problems.push(format!("{file} calls {called}, which the rule cannot find"));
            }
        }
    }
    let jobs: Vec<&Job> = parsed.values().flat_map(|parsed| &parsed.jobs).collect();
    let mut built_on: BTreeMap<String, (String, Option<&'static str>)> = BTreeMap::new();
    for job in &jobs {
        for upload in &job.uploads {
            built_on.insert(
                upload.clone(),
                (format!("{}:{}", job.workflow, job.name), job.system),
            );
        }
    }
    let mut runs = 0;
    for job in &jobs {
        let mut run: BTreeSet<usize> = BTreeSet::new();
        for script in &job.scripts {
            let (found, unplaced) = runs_of(script, &job.downloads);
            run.extend(found);
            // With nothing downloaded there is nothing of a downloaded binary to run.
            if !job.downloads.is_empty() {
                for path in unplaced {
                    problems.push(format!(
                        "{}:{} runs {path} after a cd into a folder this rule cannot resolve",
                        job.workflow, job.name
                    ));
                }
            }
        }
        for index in run {
            runs += 1;
            let artifact = &job.downloads[index].artifact;
            let at = format!("{}:{}", job.workflow, job.name);
            let Some(runs_on) = job.system else {
                problems.push(format!(
                    "{at} runs the {artifact} it downloaded on a runner whose system the rule does not know"
                ));
                continue;
            };
            match built_on.get(artifact) {
                None => problems.push(format!("{at} runs {artifact}, which no job uploads")),
                Some((maker, None)) => problems.push(format!(
                    "{at} runs {artifact}, which {maker} uploads from a runner whose system the rule does not know"
                )),
                Some((maker, Some(made_on))) if *made_on != runs_on => problems.push(format!(
                    "{at} runs on {runs_on} the {artifact} that {maker} built on {made_on}"
                )),
                Some(_) => {}
            }
        }
    }
    Report { runs, problems }
}

#[test]
fn a_job_runs_a_downloaded_binary_only_on_the_system_the_job_that_built_it_ran_on() {
    let report = check(&repository());
    assert!(
        report.runs >= 2,
        "the rule found {} downloaded binaries run; the release runs the publisher twice",
        report.runs
    );
    assert!(report.problems.is_empty(), "{}", report.problems.join("\n"));
}

#[test]
fn the_rule_reads_the_systems_and_the_artifacts_it_is_about() {
    let workflows = repository();
    let parsed: Vec<Parsed> = workflows
        .iter()
        .map(|(file, text)| parse(file, text))
        .collect();
    let find = |workflow: &str, name: &str| {
        parsed
            .iter()
            .flat_map(|parsed| &parsed.jobs)
            .find(|job| job.workflow == workflow && job.name == name)
            .unwrap_or_else(|| panic!("no job {workflow}:{name}"))
    };
    assert_eq!(find("release.yml", "publish").system, Some("linux"));
    assert_eq!(find("release.yml", "sign").system, Some("macos"));
    assert_eq!(find("windows-build.yml", "windows").system, Some("windows"));
    assert!(find("windows-build.yml", "windows")
        .uploads
        .contains(&"ConsensFlow-windows".to_string()));
    // A Windows job that sets bash as its steps' shell has them read as bash, except the steps
    // that name PowerShell.
    let windows = find("windows-build.yml", "windows");
    assert!(windows
        .scripts
        .iter()
        .any(|script| script.shell == Shell::Bash));
    assert!(windows
        .scripts
        .iter()
        .any(|script| script.shell == Shell::Pwsh));
    let sign = find("release.yml", "sign");
    let downloads: Vec<&str> = sign
        .downloads
        .iter()
        .map(|download| download.artifact.as_str())
        .collect();
    assert_eq!(
        downloads,
        ["ConsensFlow-mac", "ConsensFlow-windows", "cf-publish-macos"]
    );
}

// --- what a script's reader holds ----------------------------------------------------------

/// Whether `script` makes a file of `$RUNNER_TEMP/x` run.
fn runs_x(script: &str, shell: Shell) -> bool {
    let script = Script {
        text: script.to_string(),
        shell,
        directory: None,
        env: Vec::new(),
    };
    let downloads = [Download {
        artifact: "x".to_string(),
        folder: "$RUNNER_TEMP/x".to_string(),
    }];
    !runs_of(&script, &downloads).0.is_empty()
}

#[test]
fn a_script_runs_a_downloaded_file_when_it_makes_it_executable_or_starts_it() {
    for script in [
        "chmod +x \"$RUNNER_TEMP/x/cf\"",
        "chmod 755 $RUNNER_TEMP/x/cf",
        "chmod -R u+x,g+x \"$RUNNER_TEMP/x\"",
        "install -m 755 \"$RUNNER_TEMP/x/cf\" /usr/local/bin/",
        "\"$RUNNER_TEMP/x/cf\" --help",
        "signed=$(\"$RUNNER_TEMP/x/cf\" sums --dir .)",
        "FOO=1 \"$RUNNER_TEMP/x/cf\"",
        "exec \"$RUNNER_TEMP/x/cf\"",
        "sudo -E \"$RUNNER_TEMP/x/cf\"",
        "if \"$RUNNER_TEMP/x/cf\" check; then echo yes; fi",
        "echo hi && \"$RUNNER_TEMP/x/cf\"",
        "tool=\"$RUNNER_TEMP/x/cf\"; \"$tool\"",
        "dir=$RUNNER_TEMP/x\ntool=\"$dir/cf\"\nchmod +x \"$tool\"",
        "export tool=\"$RUNNER_TEMP/x/cf\"\n\"$tool\"",
        "cd \"$RUNNER_TEMP/x\" && ./cf",
        "cd \"$RUNNER_TEMP\"\ncd x\nchmod 700 cf",
        "pushd \"$RUNNER_TEMP/x/\"\n./cf",
        "\"$RUNNER_TEMP/x/../x/cf\"",
        "\"${RUNNER_TEMP}/x/cf\"",
        "\"${{ runner.temp }}/x/cf\"",
        "run \\\n  \"$RUNNER_TEMP/x/cf\"; chmod +x \\\n  \"$RUNNER_TEMP/x/cf\"",
    ] {
        assert!(runs_x(script, Shell::Bash), "{script}");
    }
    for script in [
        "& \"$env:RUNNER_TEMP\\x\\cf.exe\" --version",
        "& \"${env:RUNNER_TEMP}\\x\\cf.exe\"",
        "$tool = Join-Path $env:RUNNER_TEMP 'x\\cf.exe'\n& $tool",
        "$folder = Join-Path $env:RUNNER_TEMP 'x'\n$tool = Join-Path $folder 'cf.exe'\nStart-Process $tool",
        "Set-Location $env:RUNNER_TEMP\\x\n.\\cf.exe --version",
        "\"%RUNNER_TEMP%\\x\\cf.exe\"",
    ] {
        assert!(runs_x(script, Shell::Pwsh), "{script}");
    }
}

#[test]
fn a_script_that_only_reads_a_downloaded_folder_runs_nothing() {
    for script in [
        "cp -R \"$RUNNER_TEMP/x/.\" \"$RUNNER_TEMP/dist/\"",
        "shasum -a 256 \"$RUNNER_TEMP/x/cf\"",
        "echo \"$RUNNER_TEMP/x/cf\"",
        "ls \"$RUNNER_TEMP/x\"",
        "test -x \"$RUNNER_TEMP/x/cf\"",
        "cd \"$RUNNER_TEMP/x\" && find . -type f | sort | while IFS= read -r file; do shasum -a 256 \"$file\"; done",
        "out=\"$RUNNER_TEMP/x\"\nlisting=$(cd \"$out\" && find . -type f)",
        "# chmod +x \"$RUNNER_TEMP/x/cf\"",
        "chmod +x \"$RUNNER_TEMP/other/cf\"",
        "chmod +x \"$RUNNER_TEMP/xy/cf\"",
        "chmod +x other",
        "\"$RUNNER_TEMP/y/cf\" --help",
        "for job in a b; do out=\"$RUNNER_TEMP/built/$job\"; done",
        "echo \"chmod +x $RUNNER_TEMP/x/cf\"",
        "mkdir \"$RUNNER_TEMP/x/more\"",
    ] {
        assert!(!runs_x(script, Shell::Bash), "{script}");
    }
    for script in [
        "Copy-Item \"$env:RUNNER_TEMP\\x\\cf.exe\" dist",
        "Get-FileHash \"$env:RUNNER_TEMP\\x\\cf.exe\"",
        "Write-Host \"$env:RUNNER_TEMP\\x\\cf.exe\"",
    ] {
        assert!(!runs_x(script, Shell::Pwsh), "{script}");
    }
}

// --- what the rule has to see: one fixture for each shape it once let by ---------------------

/// A workflow of a job that builds `tool` on Linux, and one, `use`, on `runs_on` with `steps`
/// (at six spaces) whose header line ends with `header`, as a fixture needs.
fn fixture(runs_on: &str, header: &str, steps: &str) -> Workflows {
    configured("", runs_on, header, "", steps)
}

/// The same, with `top` (a workflow's own `env:` or `defaults:`) before its jobs and `job` (the
/// `use` job's own, at four spaces) before its steps.
fn configured(top: &str, runs_on: &str, header: &str, job: &str, steps: &str) -> Workflows {
    let text = format!(
        "name: Fixture\non: push\n{top}jobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo build --release\n      - uses: actions/upload-artifact@v7\n        with:\n          name: tool\n          path: target/release/tool\n\n  use:{header}\n    runs-on: {runs_on}\n{job}    needs: build\n    steps:\n{steps}"
    );
    BTreeMap::from([("fixture.yml".to_string(), text)])
}

/// The step that downloads `tool` into `path`.
fn download(path: &str) -> String {
    format!("      - uses: actions/download-artifact@v8\n        with:\n          name: tool\n          path: {path}\n")
}

/// A fixture whose `use` job downloads `tool` into `path`, then runs `script` (bash, or `shell`).
fn running(runs_on: &str, path: &str, shell: &str, script: &str) -> Workflows {
    let body: String = script
        .lines()
        .map(|line| format!("          {line}\n"))
        .collect();
    fixture(
        runs_on,
        "",
        &format!(
            "{}      - name: Run it\n        shell: {shell}\n        run: |\n{body}",
            download(path)
        ),
    )
}

const TEMP_TOOL: &str = "${{ runner.temp }}/tool";

/// Asserts the rule finds, in `workflows`, a problem that says `words`.
fn assert_caught(workflows: &Workflows, words: &str) {
    let report = check(workflows);
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains(words)),
        "the rule says {:?}, which does not say {words:?}",
        report.problems
    );
}

const LINUX_TOOL_ON_MAC: &str = "runs on macos the tool that fixture.yml:build built on linux";
const LINUX_TOOL_ON_WINDOWS: &str =
    "runs on windows the tool that fixture.yml:build built on linux";

#[test]
fn a_windows_job_runs_a_downloaded_binary_by_its_path_with_no_chmod() {
    let bash = running(
        "windows-latest",
        TEMP_TOOL,
        "bash",
        "\"$RUNNER_TEMP/tool/tool.exe\" --version",
    );
    assert_caught(&bash, LINUX_TOOL_ON_WINDOWS);
    let pwsh = running(
        "windows-latest",
        TEMP_TOOL,
        "pwsh",
        "& \"$env:RUNNER_TEMP\\tool\\tool.exe\" --version",
    );
    assert_caught(&pwsh, LINUX_TOOL_ON_WINDOWS);
}

#[test]
fn a_download_with_a_pattern_is_not_one_the_rule_can_attribute() {
    let merged = fixture(
        "macos-latest",
        "",
        "      - uses: actions/download-artifact@v8\n        with:\n          pattern: tool-*\n          path: ${{ runner.temp }}/tool\n          merge-multiple: true\n      - run: chmod +x \"$RUNNER_TEMP/tool/tool\"\n",
    );
    assert_caught(&merged, "cannot attribute");
}

#[test]
fn a_download_with_no_path_is_not_one_the_rule_can_attribute() {
    let nowhere = fixture(
        "macos-latest",
        "",
        "      - uses: actions/download-artifact@v8\n        with:\n          name: tool\n      - run: chmod +x tool\n",
    );
    assert_caught(&nowhere, "cannot attribute");
}

#[test]
fn every_way_to_make_a_downloaded_file_run_is_a_run() {
    for script in [
        "chmod 755 \"$RUNNER_TEMP/tool/tool\"",
        "chmod u+x \"$RUNNER_TEMP/tool/tool\"",
        "chmod a+x \"$RUNNER_TEMP/tool/tool\"",
        "chmod 0755 \"$RUNNER_TEMP/tool/tool\"",
        "install -m 755 \"$RUNNER_TEMP/tool/tool\" /usr/local/bin/tool",
    ] {
        assert_caught(
            &running("macos-latest", TEMP_TOOL, "bash", script),
            LINUX_TOOL_ON_MAC,
        );
    }
}

#[test]
fn a_path_through_a_second_variable_is_the_path() {
    let script = "folder=\"$RUNNER_TEMP/tool\"\ntool=\"$folder/tool\"\nchmod +x \"$tool\"";
    assert_caught(
        &running("macos-latest", TEMP_TOOL, "bash", script),
        LINUX_TOOL_ON_MAC,
    );
    let pwsh = "$folder = Join-Path $env:RUNNER_TEMP 'tool'\n$tool = Join-Path $folder 'tool.exe'\n& $tool --version";
    assert_caught(
        &running("windows-latest", TEMP_TOOL, "pwsh", pwsh),
        LINUX_TOOL_ON_WINDOWS,
    );
}

#[test]
fn the_runner_s_folders_are_the_same_folders_however_they_are_spelled() {
    for (path, script) in [
        (
            "${{ github.workspace }}/tool",
            "chmod +x \"$GITHUB_WORKSPACE/tool/tool\"",
        ),
        (
            "${{ github.workspace }}/tool",
            "chmod +x \"${{ github.workspace }}/tool/tool\"",
        ),
        (TEMP_TOOL, "chmod +x \"${RUNNER_TEMP}/tool/tool\""),
        (TEMP_TOOL, "chmod +x \"${{ runner.temp }}/tool/tool\""),
        (
            "${{ runner.temp }}/tool/",
            "chmod +x \"$RUNNER_TEMP/tool/./tool\"",
        ),
        (
            "${{ runner.temp }}\\tool",
            "chmod +x \"$RUNNER_TEMP\\tool\\tool\"",
        ),
    ] {
        assert_caught(
            &running("macos-latest", path, "bash", script),
            LINUX_TOOL_ON_MAC,
        );
    }
    for script in [
        "& \"$env:RUNNER_TEMP\\tool\\tool.exe\"",
        "& \"${env:RUNNER_TEMP}\\tool\\tool.exe\"",
        "& \"${{ runner.temp }}\\tool\\tool.exe\"",
    ] {
        assert_caught(
            &running("windows-latest", TEMP_TOOL, "pwsh", script),
            LINUX_TOOL_ON_WINDOWS,
        );
    }
}

#[test]
fn a_file_run_after_a_cd_into_its_folder_is_run() {
    for script in [
        "cd \"$RUNNER_TEMP/tool\"\nchmod +x tool",
        "cd \"$RUNNER_TEMP/tool\"\n./tool --version",
        "cd \"$RUNNER_TEMP\" && cd tool && ./tool",
    ] {
        assert_caught(
            &running("macos-latest", TEMP_TOOL, "bash", script),
            LINUX_TOOL_ON_MAC,
        );
    }
    let pwsh = "Set-Location \"$env:RUNNER_TEMP\\tool\"\n.\\tool.exe --version";
    assert_caught(
        &running("windows-latest", TEMP_TOOL, "pwsh", pwsh),
        LINUX_TOOL_ON_WINDOWS,
    );
}

#[test]
fn a_job_header_with_a_comment_is_a_job() {
    let commented = fixture(
        "macos-latest",
        " # runs the tool",
        &format!(
            "{}      - run: chmod +x \"$RUNNER_TEMP/tool/tool\"\n",
            download(TEMP_TOOL)
        ),
    );
    assert_caught(&commented, LINUX_TOOL_ON_MAC);
}

#[test]
fn an_indented_comment_inside_a_job_does_not_end_it() {
    let commented = fixture(
        "macos-latest",
        "",
        &format!("  # the steps, which run the tool\n{}      - run: chmod +x \"$RUNNER_TEMP/tool/tool\"\n", download(TEMP_TOOL)),
    );
    assert_caught(&commented, LINUX_TOOL_ON_MAC);
}

#[test]
fn a_run_written_as_a_quoted_yaml_value_is_read_as_its_script() {
    for run in [
        "      - run: 'chmod +x \"$RUNNER_TEMP/tool/tool\"'\n",
        "      - run: \"chmod +x \\\"$RUNNER_TEMP/tool/tool\\\"\"\n",
        "      - name: Make it run\n        run: 'chmod +x \"$RUNNER_TEMP/tool/tool\"'\n",
        "      - name: Run it\n        run: '\"$RUNNER_TEMP/tool/tool\" --version' # a comment\n",
    ] {
        let quoted = fixture("macos-latest", "", &format!("{}{run}", download(TEMP_TOOL)));
        assert_caught(&quoted, LINUX_TOOL_ON_MAC);
    }
}

// --- what the rule once did not read (T-831 P3-4, T-840) --------------------------------------

#[test]
fn a_relative_path_of_a_download_is_the_workspace_s() {
    for (path, script) in [
        ("tools", "chmod +x tools/tool"),
        ("tools/", "chmod +x ./tools/tool"),
        ("./tools", "\"$GITHUB_WORKSPACE/tools/tool\" --version"),
        ("tools", "cd tools && ./tool"),
        ("tools", "chmod +x \"${{ github.workspace }}/tools/tool\""),
    ] {
        assert_caught(
            &running("macos-latest", path, "bash", script),
            LINUX_TOOL_ON_MAC,
        );
    }
    let other = check(&running(
        "macos-latest",
        "tools",
        "bash",
        "chmod +x other/tool\nchmod +x tools2/tool",
    ));
    assert_eq!(other.runs, 0, "{:?}", other.problems);
}

#[test]
fn an_env_of_the_job_is_an_env_of_its_steps() {
    let steps = format!("{}      - run: chmod +x \"$TOOL\"\n", download(TEMP_TOOL));
    let job = configured(
        "",
        "macos-latest",
        "",
        "    env:\n      TOOL: ${{ runner.temp }}/tool/tool\n",
        &steps,
    );
    assert_caught(&job, LINUX_TOOL_ON_MAC);
    let workflow = configured(
        "env:\n  TOOL: ${{ runner.temp }}/tool/tool\n",
        "macos-latest",
        "",
        "",
        &steps,
    );
    assert_caught(&workflow, LINUX_TOOL_ON_MAC);
}

#[test]
fn a_working_directory_of_the_job_s_defaults_is_its_steps() {
    let steps = format!("{}      - run: ./tool --version\n", download(TEMP_TOOL));
    let job = configured(
        "",
        "macos-latest",
        "",
        "    defaults:\n      run:\n        working-directory: ${{ runner.temp }}/tool\n",
        &steps,
    );
    assert_caught(&job, LINUX_TOOL_ON_MAC);
    let workflow = configured(
        "defaults:\n  run:\n    working-directory: ${{ runner.temp }}/tool\n",
        "macos-latest",
        "",
        "",
        &steps,
    );
    assert_caught(&workflow, LINUX_TOOL_ON_MAC);
}

#[test]
fn a_shell_of_the_defaults_is_the_shell_of_its_steps() {
    // Under cmd a bare program is run from the folder the script is in, which no other shell does.
    let steps = format!(
        "{}      - run: |\n          cd /d %RUNNER_TEMP%\\tool\n          tool.exe --version\n",
        download(TEMP_TOOL)
    );
    let job = configured(
        "",
        "windows-latest",
        "",
        "    defaults:\n      run:\n        shell: cmd\n",
        &steps,
    );
    assert_caught(&job, LINUX_TOOL_ON_WINDOWS);
    let workflow = configured(
        "defaults:\n  run:\n    shell: cmd\n",
        "windows-latest",
        "",
        "",
        &steps,
    );
    assert_caught(&workflow, LINUX_TOOL_ON_WINDOWS);
    // A step's own shell is first, and where none is given PowerShell reads `tool.exe` as a
    // command of its PATH, which runs nothing of the folder.
    let own = format!(
        "{}      - shell: pwsh\n        run: |\n          Set-Location $env:RUNNER_TEMP\\tool\n          tool.exe --version\n",
        download(TEMP_TOOL)
    );
    let pwsh = configured(
        "defaults:\n  run:\n    shell: cmd\n",
        "windows-latest",
        "",
        "",
        &own,
    );
    assert_eq!(check(&pwsh).runs, 0, "{:?}", check(&pwsh).problems);
}

#[test]
fn a_key_a_step_begins_with_is_read_as_any_other() {
    for first in [
        "      - working-directory: ${{ runner.temp }}/tool\n        run: ./tool --version\n",
        "      - env:\n          TOOL: ${{ runner.temp }}/tool/tool\n        run: chmod +x \"$TOOL\"\n",
        "      - shell: bash\n        run: chmod +x \"$RUNNER_TEMP/tool/tool\"\n",
    ] {
        let steps = format!("{}{first}", download(TEMP_TOOL));
        assert_caught(
            &configured(
                "defaults:\n  run:\n    shell: cmd\n",
                "macos-latest",
                "",
                "",
                &steps,
            ),
            LINUX_TOOL_ON_MAC,
        );
    }
}

#[test]
fn eval_and_a_shell_given_a_string_run_what_the_string_does() {
    for script in [
        "eval \"$RUNNER_TEMP/tool/tool --version\"",
        "eval chmod +x \"$RUNNER_TEMP/tool/tool\"",
        "sh -c '\"$RUNNER_TEMP/tool/tool\" --version'",
        "bash -ec 'chmod +x \"$RUNNER_TEMP/tool/tool\"'",
        "x=\"$RUNNER_TEMP/tool/tool\"; eval \"$x\"",
    ] {
        assert_caught(
            &running("macos-latest", TEMP_TOOL, "bash", script),
            LINUX_TOOL_ON_MAC,
        );
    }
    let pwsh = "powershell -NoProfile -Command \"& '$env:RUNNER_TEMP\\tool\\tool.exe' --version\"";
    assert_caught(
        &running("windows-latest", TEMP_TOOL, "pwsh", pwsh),
        LINUX_TOOL_ON_WINDOWS,
    );
}

#[test]
fn xargs_runs_what_it_is_given() {
    for script in [
        "find \"$RUNNER_TEMP/tool\" -type f | xargs chmod +x",
        "ls \"$RUNNER_TEMP/tool\" | xargs -I{} chmod 755 \"$RUNNER_TEMP/tool/{}\"",
        "printf '%s\\n' a b | xargs -n 1 \"$RUNNER_TEMP/tool/tool\"",
        "echo \"$RUNNER_TEMP/tool/tool\" | xargs -n1 sh -c 'chmod +x \"$0\"'",
        "find \"$RUNNER_TEMP/tool\" -type f -print0 | xargs -0 chmod u+x",
    ] {
        assert_caught(
            &running("macos-latest", TEMP_TOOL, "bash", script),
            LINUX_TOOL_ON_MAC,
        );
    }
    let read = check(&running(
        "macos-latest",
        TEMP_TOOL,
        "bash",
        "find \"$RUNNER_TEMP/tool\" -type f | xargs shasum -a 256\nfind \"$RUNNER_TEMP/tool\" -type f | xargs ls -l",
    ));
    assert_eq!(read.runs, 0, "{:?}", read.problems);
}

#[test]
fn find_exec_runs_what_it_is_given() {
    for script in [
        "find \"$RUNNER_TEMP/tool\" -type f -exec chmod +x {} +",
        "find \"$RUNNER_TEMP/tool\" -name tool -exec {} --version \\;",
        "find \"$RUNNER_TEMP/tool\" -name tool -execdir ./tool --version \\;",
        "cd \"$RUNNER_TEMP/tool\" && find . -type f -exec chmod 755 {} \\;",
    ] {
        assert_caught(
            &running("macos-latest", TEMP_TOOL, "bash", script),
            LINUX_TOOL_ON_MAC,
        );
    }
    let read = check(&running(
        "macos-latest",
        TEMP_TOOL,
        "bash",
        "find \"$RUNNER_TEMP/tool\" -type f -exec shasum -a 256 {} +\nfind \"$RUNNER_TEMP/tool\" -type f -exec ls -l {} \\;",
    ));
    assert_eq!(read.runs, 0, "{:?}", read.problems);
}

#[test]
fn cmd_c_runs_what_it_is_given() {
    for (shell, script) in [
        ("bash", "cmd /c \"$RUNNER_TEMP/tool/tool.exe\" --version"),
        (
            "bash",
            "cmd.exe //c \"$RUNNER_TEMP\\\\tool\\\\tool.exe --version\"",
        ),
        (
            "pwsh",
            "cmd /c \"$env:RUNNER_TEMP\\tool\\tool.exe --version\"",
        ),
        ("pwsh", "cmd /d /s /c \"%RUNNER_TEMP%\\tool\\tool.exe\""),
        ("cmd", "cmd /c %RUNNER_TEMP%\\tool\\tool.exe"),
    ] {
        assert_caught(
            &running("windows-latest", TEMP_TOOL, shell, script),
            LINUX_TOOL_ON_WINDOWS,
        );
    }
}

#[test]
fn under_cmd_a_bare_program_of_the_folder_one_is_in_is_run() {
    for script in [
        "cd /d %RUNNER_TEMP%\\tool\ntool.exe --version",
        "cd /d \"%RUNNER_TEMP%\\tool\" && tool.exe",
        "pushd %RUNNER_TEMP%\\tool\ncall tool.exe",
        "set \"TOOLS=%RUNNER_TEMP%\\tool\"\ncd /d %TOOLS%\nstart \"\" /wait tool.exe",
        "%RUNNER_TEMP%\\tool\\tool.exe --version",
    ] {
        assert_caught(
            &running("windows-latest", TEMP_TOOL, "cmd", script),
            LINUX_TOOL_ON_WINDOWS,
        );
    }
    let reads = check(&running(
        "windows-latest",
        TEMP_TOOL,
        "cmd",
        "cd /d %RUNNER_TEMP%\\tool\ndir\ncopy tool.exe ..\\dist\\\necho tool.exe\ncertutil -hashfile tool.exe SHA256\ndel old.exe",
    ));
    assert_eq!(reads.runs, 0, "{:?}", reads.problems);
}

// --- what the rule once did not read, again (T-847 P3-5) ----------------------------------------

#[test]
fn a_wrapper_program_runs_its_argument() {
    for script in [
        "timeout 5 \"$RUNNER_TEMP/tool/tool\" --version",
        "timeout -s KILL 5 \"$RUNNER_TEMP/tool/tool\"",
        "nice -n 5 \"$RUNNER_TEMP/tool/tool\"",
        "caffeinate -i \"$RUNNER_TEMP/tool/tool\"",
        "caffeinate -t 60 \"$RUNNER_TEMP/tool/tool\"",
        "arch -arm64 \"$RUNNER_TEMP/tool/tool\"",
        "setsid \"$RUNNER_TEMP/tool/tool\"",
        "flock \"$RUNNER_TEMP/lock\" \"$RUNNER_TEMP/tool/tool\"",
        "flock -c 'chmod +x \"$RUNNER_TEMP/tool/tool\"' \"$RUNNER_TEMP/lock\"",
        "stdbuf -oL \"$RUNNER_TEMP/tool/tool\"",
        "stdbuf -o L \"$RUNNER_TEMP/tool/tool\"",
        "xvfb-run -a \"$RUNNER_TEMP/tool/tool\"",
        "script -q /dev/null \"$RUNNER_TEMP/tool/tool\" --version",
        "script -c 'chmod +x \"$RUNNER_TEMP/tool/tool\"' /dev/null",
        "timeout 5 nice -n 5 chmod +x \"$RUNNER_TEMP/tool/tool\"",
    ] {
        assert_caught(
            &running("macos-latest", TEMP_TOOL, "bash", script),
            LINUX_TOOL_ON_MAC,
        );
    }
    // A wrapper around a program that only reads runs nothing of what it is given.
    let read = check(&running(
        "macos-latest",
        TEMP_TOOL,
        "bash",
        "timeout 5 shasum -a 256 \"$RUNNER_TEMP/tool/tool\"\nnice ls -l \"$RUNNER_TEMP/tool\"\nflock \"$RUNNER_TEMP/lock\" cp \"$RUNNER_TEMP/tool/tool\" dist",
    ));
    assert_eq!(read.runs, 0, "{:?}", read.problems);
}

#[test]
fn invoke_expression_and_invoke_command_run_what_they_are_given() {
    for script in [
        "Invoke-Expression \"chmod +x $env:RUNNER_TEMP/tool/tool\"",
        "iex \"$env:RUNNER_TEMP/tool/tool --version\"",
        "Invoke-Command -ScriptBlock { chmod +x \"$env:RUNNER_TEMP/tool/tool\" }",
        "icm { chmod 755 \"$env:RUNNER_TEMP/tool/tool\" }",
        "pwsh -File \"$env:RUNNER_TEMP/tool/tool.ps1\"",
        "powershell -NoProfile -File $env:RUNNER_TEMP/tool/tool.ps1 -Arg 1",
        "Invoke-Expression -Command \"chmod +x $env:RUNNER_TEMP/tool/tool\"",
        "iex -c \"$env:RUNNER_TEMP/tool/tool --version\"",
        "\"chmod +x $env:RUNNER_TEMP/tool/tool\" | iex",
        "'$env:RUNNER_TEMP/tool/tool --version' | Invoke-Expression",
        "Invoke-Command -FilePath \"$env:RUNNER_TEMP/tool/tool.ps1\"",
        "icm -FilePath $env:RUNNER_TEMP/tool/tool.ps1 -ArgumentList 1",
    ] {
        assert_caught(
            &running("macos-latest", TEMP_TOOL, "pwsh", script),
            LINUX_TOOL_ON_MAC,
        );
    }
    // A string given to Invoke-Expression that runs nothing downloaded, and a script of another folder.
    let elsewhere = check(&running(
        "macos-latest",
        TEMP_TOOL,
        "pwsh",
        "\"Get-Date\" | iex\nInvoke-Expression -Command \"Get-Date\"\nInvoke-Command -FilePath \"$env:RUNNER_TEMP/other/tool.ps1\"",
    ));
    assert_eq!(elsewhere.runs, 0, "{:?}", elsewhere.problems);
}

#[test]
fn a_run_after_a_cd_into_a_folder_the_rule_cannot_resolve_is_a_failure_not_nothing() {
    for script in [
        "cd \"$somewhere\"\n./tool --version",
        "cd \"$somewhere\" && chmod +x tool",
        "pushd \"$NOT_KNOWN/bin\"\nbin/tool",
        "for out in a b; do cd \"$out\"; ./tool; done",
    ] {
        assert_caught(
            &running("macos-latest", TEMP_TOOL, "bash", script),
            "after a cd into a folder this rule cannot resolve",
        );
    }
    // Nothing relative is run after it, or the folder is known again: nothing to say.
    for script in [
        "cd \"$somewhere\"\nls\nfind . -type f | sort",
        "cd \"$somewhere\"\ncd \"$RUNNER_TEMP/other\"\n./tool",
        "cd \"$somewhere\"\n\"$RUNNER_TEMP/other/tool\" --version",
    ] {
        let said = check(&running("macos-latest", TEMP_TOOL, "bash", script));
        assert!(said.problems.is_empty(), "{script}: {:?}", said.problems);
    }
    // And a job with no download has nothing of a downloaded binary to run.
    let none = BTreeMap::from([(
        "fixture.yml".to_string(),
        "name: Fixture\non: push\njobs:\n  use:\n    runs-on: macos-latest\n    steps:\n      - run: |\n          cd \"$somewhere\"\n          ./tool\n".to_string(),
    )]);
    assert!(
        check(&none).problems.is_empty(),
        "{:?}",
        check(&none).problems
    );
}

// --- and what it leaves alone, and what it says it cannot read -------------------------------

#[test]
fn a_binary_run_where_it_was_built_is_fine_and_so_is_one_that_is_only_read() {
    let same = check(&running(
        "ubuntu-latest",
        TEMP_TOOL,
        "bash",
        "chmod +x \"$RUNNER_TEMP/tool/tool\"\n\"$RUNNER_TEMP/tool/tool\"",
    ));
    assert_eq!(
        (same.runs, same.problems.is_empty()),
        (1, true),
        "{:?}",
        same.problems
    );
    let read = check(&running(
        "windows-latest",
        TEMP_TOOL,
        "bash",
        "cp -R \"$RUNNER_TEMP/tool/.\" dist\nshasum -a 256 \"$RUNNER_TEMP/tool/tool\"\ncd \"$RUNNER_TEMP/tool\" && find . -type f",
    ));
    assert_eq!(
        (read.runs, read.problems.is_empty()),
        (0, true),
        "{:?}",
        read.problems
    );
}

#[test]
fn a_download_in_a_layout_the_rule_does_not_read_is_a_failure_not_a_skip() {
    let indented = BTreeMap::from([(
        "fixture.yml".to_string(),
        "name: Fixture\non: push\njobs:\n  use:\n    runs-on: macos-latest\n    steps:\n    - uses: actions/download-artifact@v8\n      with:\n        name: tool\n        path: ${{ runner.temp }}/tool\n".to_string(),
    )]);
    assert_caught(&indented, "the rule reads 0 downloads as steps");
}

#[test]
fn a_runner_the_rule_does_not_know_is_a_failure_where_a_binary_is_run_on_it() {
    let matrix = running(
        "${{ matrix.os }}",
        TEMP_TOOL,
        "bash",
        "chmod +x \"$RUNNER_TEMP/tool/tool\"",
    );
    assert_caught(&matrix, "whose system the rule does not know");
}

#[test]
fn a_workflow_a_job_calls_and_the_rule_cannot_find_is_a_failure() {
    let calls = BTreeMap::from([(
        "fixture.yml".to_string(),
        "name: Fixture\non: push\njobs:\n  call:\n    uses: ./.github/workflows/gone.yml\n"
            .to_string(),
    )]);
    assert_caught(&calls, "calls gone.yml, which the rule cannot find");
}
