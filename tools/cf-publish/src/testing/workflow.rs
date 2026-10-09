//! The workflows' text, read for what the tests hold it to: the steps whose
//! scripts bash runs, a job's lines, a step's environment. A hand run of a
//! workflow shows a step's syntax only once it gets there (the publish step
//! never, short of a tag), so the tests read the steps off the text and run
//! them as written.
//!
//! These read the workflows' layout as the repository writes it: a job at two
//! spaces, a step at six, its keys at eight.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{path_dirs, Ran, TempDir};

/// The text of the workflow `file` (`release.yml`).
pub fn read(file: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(".github")
        .join("workflows")
        .join(file);
    fs::read_to_string(&path).unwrap_or_else(|cause| panic!("{}: {cause}", path.display()))
}

/// The workflow files, by name.
pub fn files() -> Vec<String> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(".github")
        .join("workflows");
    let mut names: Vec<String> = fs::read_dir(folder)
        .expect("the workflows")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".yml"))
        .collect();
    names.sort();
    names
}

/// The release workflow's steps by name, and the places it names, as the tests ask for them.
pub mod release {
    /// The step that publishes the release and moves its feeds.
    pub const PUBLISH: &str = "Publish the release, then its update feeds";
    /// The step that reads the feeds back.
    pub const CHECK: &str = "The feeds serve this release, and its archive downloads";
    /// The Mac job's early question to the old feeds.
    pub const PREREQUISITES: &str = "The old feeds serve the bridge, for a release after it";
    /// The Mac job's notes, and the plan of the feeds.
    pub const NOTES: &str = "Notes, and the update feed for this build";
    /// The publish job's check of the publisher against its hash.
    pub const HASH: &str = "The publisher is the binary that was built";
    /// The Mac job's build of the rule of the feeds.
    pub const BUILD_RULE: &str = "Build cf-publish, the rule of the feeds";
    /// The publisher job's build of the binary the publish job runs.
    pub const BUILD_PUBLISHER: &str = "Build cf-publish, and say what it hashes to";
    /// Where the publish job runs the publisher from.
    pub const PUBLISHER: &str = "$RUNNER_TEMP/publisher/cf-publish";
    /// Where GitHub serves a release's files, in the steps' words.
    pub const DOWNLOADS: &str = "https://github.com/$GITHUB_REPOSITORY/releases/download";
}

/// `text` but its comment lines.
pub fn live(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// How many spaces a line is indented by.
fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// A step that bash runs: its name (its script's first line where it has none)
/// and its script.
#[derive(Debug)]
pub struct Step {
    pub name: String,
    pub script: String,
}

/// The `run` of each step of a workflow that bash runs, read off the text.
pub fn bash_steps(text: &str) -> Vec<Step> {
    let lines: Vec<&str> = text.split('\n').collect();
    let is_step = |line: &str| line.starts_with("      - ");
    let starts: Vec<usize> = (0..lines.len()).filter(|at| is_step(lines[*at])).collect();
    let run_of = |line: &str| {
        line.strip_prefix("      - run: ")
            .or_else(|| line.strip_prefix("        run: "))
            .map(str::to_string)
    };
    let name_of = |line: &str| {
        line.strip_prefix("      - name: ")
            .or_else(|| line.strip_prefix("        name: "))
            .map(str::to_string)
    };
    let mut steps = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(lines.len());
        let body = &lines[*start..end];
        if body.contains(&"        shell: pwsh") {
            continue;
        }
        let Some(run) = body.iter().position(|line| run_of(line).is_some()) else {
            continue;
        };
        let first = run_of(body[run]).unwrap_or_default();
        let name = body
            .iter()
            .find_map(|line| name_of(line))
            .unwrap_or_else(|| first.clone());
        if first != "|" {
            steps.push(Step {
                name,
                script: first,
            });
            continue;
        }
        steps.push(Step {
            name,
            script: block_after(&body[run + 1..]),
        });
    }
    steps
}

/// The lines of a block scalar, dedented by the first line's indentation, up to
/// the first line indented less.
fn block_after(lines: &[&str]) -> String {
    let mut script = Vec::new();
    let mut indent = None;
    for line in lines {
        let own = indent_of(line);
        if !line.trim().is_empty() {
            let at = *indent.get_or_insert(own);
            if own < at {
                break;
            }
        }
        script.push(if line.trim().is_empty() {
            String::new()
        } else {
            line[indent.unwrap_or(0)..].to_string()
        });
    }
    script.join("\n")
}

/// The text of the job `name`, to the next job.
pub fn job(text: &str, name: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let header = format!("  {name}:");
    let start = lines
        .iter()
        .position(|line| *line == header)
        .unwrap_or_else(|| panic!("the workflow has no job {name}"));
    let end = lines[start + 1..]
        .iter()
        .position(|line| indent_of(line) == 2 && !line.trim().is_empty())
        .map_or(lines.len(), |at| start + 1 + at);
    lines[start..end].join("\n")
}

/// A step of a job, found by its name.
pub struct StepText {
    lines: Vec<String>,
}

/// The step named `name` in `text` (a job's text, or the whole workflow's).
pub fn step(text: &str, name: &str) -> StepText {
    let lines: Vec<&str> = text.split('\n').collect();
    let wanted = format!("- name: {name}");
    let start = lines
        .iter()
        .position(|line| line.trim() == wanted)
        .unwrap_or_else(|| panic!("the workflow has no step named {name}"));
    let end = lines[start + 1..]
        .iter()
        .position(|line| {
            line.starts_with("      - ") || (!line.trim().is_empty() && indent_of(line) < 6)
        })
        .map_or(lines.len(), |at| start + 1 + at);
    StepText {
        lines: lines[start..end]
            .iter()
            .map(|line| (*line).to_string())
            .collect(),
    }
}

impl StepText {
    /// All of it.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// The script of its `run:`, a block (`run: |`) or a line.
    pub fn script(&self) -> String {
        let lines: Vec<&str> = self.lines.iter().map(String::as_str).collect();
        let run = lines
            .iter()
            .position(|line| line.starts_with("        run: "))
            .expect("the step has a run");
        match lines[run].trim_start_matches("        run: ") {
            "|" => block_after(&lines[run + 1..]),
            line => line.to_string(),
        }
    }

    /// The value of its key `key` (`working-directory`), as written.
    pub fn field(&self, key: &str) -> Option<String> {
        let prefix = format!("        {key}: ");
        self.lines
            .iter()
            .find_map(|line| line.strip_prefix(&prefix).map(str::to_string))
    }

    /// The variables of its `env:`, as written.
    pub fn env(&self) -> Vec<(String, String)> {
        let Some(at) = self.lines.iter().position(|line| line == "        env:") else {
            return Vec::new();
        };
        self.lines[at + 1..]
            .iter()
            .take_while(|line| line.starts_with("          "))
            .filter_map(|line| {
                let (name, value) = line.trim().split_once(':')?;
                Some((name.to_string(), value.trim().to_string()))
            })
            .collect()
    }
}

/// `text` with each `${{ … }}` expression of `known` replaced by its value.
pub fn expand(text: &str, known: &[(&str, &str)]) -> String {
    known
        .iter()
        .fold(text.to_string(), |text, (expression, value)| {
            text.replace(&format!("${{{{ {expression} }}}}"), value)
        })
}

/// The first directory of the PATH that holds `program`.
pub fn find_program(program: &str) -> Option<PathBuf> {
    path_dirs()
        .into_iter()
        .find(|dir| dir.join(program).is_file())
}

/// Runs `script` with bash, in `cwd`: the environment is `env` and nothing
/// else but a PATH of `path_first`, the folder of the `node` this machine has,
/// and the system's (`/usr/bin`, `/bin`).
#[allow(clippy::disallowed_methods)] // The test starts what it tests.
pub fn bash(script: &str, cwd: &Path, env: &[(&str, &str)], path_first: &[&Path]) -> Ran {
    let root = TempDir::new("step");
    let file = root.path().join("step.sh");
    fs::write(&file, script).expect("the script");
    let mut dirs: Vec<PathBuf> = path_first.iter().map(|dir| dir.to_path_buf()).collect();
    dirs.extend(find_program("node"));
    dirs.extend([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
    let path: OsString = std::env::join_paths(dirs).expect("a PATH");
    let output = Command::new("bash")
        .arg(&file)
        .current_dir(cwd)
        .env_clear()
        .envs(env.iter().copied())
        .env("PATH", path)
        .output()
        .expect("bash runs");
    Ran {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Whether bash is here to run a script.
///
/// Never on Windows: there a program is looked for in the system folder before
/// `PATH`, so `bash` is WSL's launcher, not Git's bash, and parses nothing. A
/// script parses alike everywhere, and the Mac's run of these tests checks it.
#[allow(clippy::disallowed_methods)] // The test starts what it tests.
pub fn has_bash() -> bool {
    cfg!(unix) && Command::new("bash").arg("--version").output().is_ok()
}

/// Parses `script` with `bash -n`: whether it does, and what bash said if not.
#[allow(clippy::disallowed_methods)] // The test starts what it tests.
pub fn parses_as_bash(script: &str) -> Result<(), String> {
    let root = TempDir::new("parse");
    let file = root.path().join("step.sh");
    fs::write(&file, script).expect("the script");
    let checked = Command::new("bash")
        .arg("-n")
        .arg(&file)
        .output()
        .expect("bash runs");
    if checked.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&checked.stderr).into_owned())
    }
}
