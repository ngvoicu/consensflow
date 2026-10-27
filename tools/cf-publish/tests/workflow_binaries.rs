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

/// The shell a job's `defaults: run:` gives its steps.
fn default_shell(job: &str) -> Option<String> {
    let mut lines = job.lines().skip_while(|line| *line != "    defaults:");
    lines.next()?;
    lines
        .take_while(|line| line.starts_with("     "))
        .find_map(|line| line.trim().strip_prefix("shell:").map(plain))
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
    Windows,
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
    for name in job_names(text) {
        let body = job(text, &name);
        if let Some(called) = job_key(&body, "uses") {
            if let Some(called) = called.strip_prefix("./.github/workflows/") {
                parsed.calls.push(called.to_string());
            }
            continue;
        }
        let runs_on = job_key(&body, "runs-on");
        let default_shell = default_shell(&body);
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
                            folder: spelled(&expressions(&path)),
                        });
                    }
                    _ => parsed.unread.push(format!(
                        "{file}:{name} downloads an artifact this rule cannot attribute (a `pattern:`, or no `name:` or `path:`): download each artifact by its name into a folder of its own"
                    )),
                }
            } else if step.runs() {
                let shell = step.field("shell").map(|shell| plain(&shell));
                let windows = match shell.or_else(|| default_shell.clone()).as_deref() {
                    Some(shell) => ["pwsh", "powershell", "cmd"].contains(&shell),
                    None => system == Some("windows"),
                };
                job.scripts.push(Script {
                    text: step.script(),
                    shell: if windows { Shell::Windows } else { Shell::Bash },
                    directory: step.field("working-directory").map(|dir| plain(&dir)),
                    env: step.env(),
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

/// The statements of a script, each as its words with their quotes taken off: split at a line's
/// end, `;`, `&`, `&&`, `|`, `||`, a parenthesis and a backtick, outside quotes; the comments left
/// out, and PowerShell's `&` that starts a statement (the call operator) with them.
fn statements(script: &str, shell: Shell) -> Vec<Vec<String>> {
    let bash = shell == Shell::Bash;
    let text = if bash {
        script.replace("\\\n", " ")
    } else {
        script.replace("`\n", " ")
    };
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut started = false;
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
                '\'' | '"' => {
                    quote = Some(c);
                    started = true;
                }
                '\\' if bash && at + 1 < chars.len() => {
                    at += 1;
                    word.push(chars[at]);
                    started = true;
                }
                '#' if !started => {
                    while at + 1 < chars.len() && chars[at + 1] != '\n' {
                        at += 1;
                    }
                }
                ' ' | '\t' | '\r' | '\n' | ';' | '|' | '(' | ')' | '`' | '&' => {
                    if started {
                        words.push(std::mem::take(&mut word));
                        started = false;
                    }
                    let call = c == '&'
                        && words.is_empty()
                        && chars.get(at + 1).is_some_and(|next| next.is_whitespace());
                    if !matches!(c, ' ' | '\t' | '\r') && !call && !words.is_empty() {
                        out.push(std::mem::take(&mut words));
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
        out.push(words);
    }
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

/// The downloads (by their place in `downloads`) whose files the script makes run.
fn runs_of(script: &Script, downloads: &[Download]) -> BTreeSet<usize> {
    let mut bindings: BTreeMap<String, String> = BTreeMap::new();
    for (name, value) in &script.env {
        let value = spelled(&expand(&expressions(value), &bindings));
        bindings.insert(name.clone(), value);
    }
    let mut cwd: Option<String> = Some(script.directory.as_deref().map_or_else(
        || "$GITHUB_WORKSPACE".to_string(),
        |dir| spelled(&expand(&expressions(dir), &bindings)),
    ));
    let mut found = BTreeSet::new();
    let at = |path: &str, cwd: &Option<String>| {
        if absolute(path) {
            Some(spelled(path))
        } else {
            cwd.as_ref().map(|cwd| spelled(&format!("{cwd}/{path}")))
        }
    };
    for raw in statements(&expressions(&script.text), script.shell) {
        if let Some((name, value)) = powershell_assignment(&raw, &bindings) {
            bindings.insert(name, value);
            continue;
        }
        let words: Vec<String> = raw.iter().map(|word| expand(word, &bindings)).collect();
        let mut start = 0;
        let mut wrapped = false;
        while start < words.len() {
            let word = words[start].as_str();
            if BEFORE_A_COMMAND.contains(&word) {
                wrapped = true;
            } else if let Some((name, value)) = assignment(word) {
                bindings.insert(name.to_string(), spelled(value));
            } else if !(wrapped && word.starts_with('-')) {
                break;
            }
            start += 1;
        }
        let Some(command) = words.get(start) else {
            continue;
        };
        let args = &words[start + 1..];
        let lowered = command.to_lowercase();
        let program = lowered.rsplit('/').next().unwrap_or(&lowered);
        let mut hit = |path: &str| {
            if let Some(path) = at(path, &cwd) {
                for (index, download) in downloads.iter().enumerate() {
                    if inside(&path, &download.folder) {
                        found.insert(index);
                    }
                }
            }
        };
        if NO_COMMAND.contains(&program) {
            continue;
        } else if CHANGES_FOLDER.contains(&program) {
            cwd = args.first().and_then(|dir| {
                let dir = spelled(dir);
                let unknown = dir.contains('$')
                    && !dir.starts_with("$RUNNER_TEMP")
                    && !dir.starts_with("$GITHUB_WORKSPACE");
                if unknown {
                    None
                } else {
                    at(&dir, &cwd)
                }
            });
        } else if program == "chmod" {
            args.iter()
                .filter(|arg| !is_mode(arg))
                .for_each(|arg| hit(arg));
        } else if RUNS_AN_ARGUMENT.contains(&program) {
            args.iter()
                .filter(|arg| !arg.starts_with('-'))
                .for_each(|arg| hit(arg));
        } else if command.contains(['/', '\\']) || command.starts_with('$') {
            hit(command);
        }
    }
    found
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
        let run: BTreeSet<usize> = job
            .scripts
            .iter()
            .flat_map(|script| runs_of(script, &job.downloads))
            .collect();
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
        .any(|script| script.shell == Shell::Windows));
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
    !runs_of(&script, &downloads).is_empty()
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
        assert!(runs_x(script, Shell::Windows), "{script}");
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
        assert!(!runs_x(script, Shell::Windows), "{script}");
    }
}

// --- what the rule has to see: one fixture for each shape it once let by ---------------------

/// A workflow of a job that builds `tool` on Linux, and one, `use`, on `runs_on` with `steps`
/// (at six spaces) whose header line ends with `header`, as a fixture needs.
fn fixture(runs_on: &str, header: &str, steps: &str) -> Workflows {
    let text = format!(
        "name: Fixture\non: push\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo build --release\n      - uses: actions/upload-artifact@v7\n        with:\n          name: tool\n          path: target/release/tool\n\n  use:{header}\n    runs-on: {runs_on}\n    needs: build\n    steps:\n{steps}"
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
