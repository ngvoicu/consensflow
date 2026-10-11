//! What the opt-in live tests share. A live test runs real harnesses on the
//! machine's own logins and spends real quota, so it is ignored unless asked
//! for (`cargo xtask live <name>`), and it needs the machine to be set up for
//! it: the harnesses installed and logged in. The rest is as the rig's cases:
//! the `cf` and the pane host built from this checkout, in a home of the
//! case's own that is never `~/.consensflow`.
//!
//! The harnesses are the machine's: [`installed`] finds a harness's command and
//! the home its login is read from, and [`rig`] is a rig whose windows find
//! them. Nothing is copied: a window reads the login where it is, as it does
//! for the person it belongs to (and writes its own sessions there, as it does
//! for them).
//!
//! What a live run says as it goes, and how it waits, is shared too: [`Log`]
//! says what the run is doing with the seconds since it began, [`wait`] says
//! what it waits for and for how long before it does, and [`text`] and [`cut`]
//! make what the product answered fit a line.

use std::fmt::Display;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::process::{own_var, Run};
use crate::rig::{Config, Rig};
use crate::{files, Error, Result};

/// The variable that leaves a live run's home and project where they are, for a
/// person to look at.
pub const KEEP: &str = "CONSENSFLOW_LIVE_KEEP";

/// Whether the run is to leave what it made: [`KEEP`] is set to something that
/// is not empty or `0`.
pub fn keep_requested() -> bool {
    kept(own_var(KEEP).as_deref())
}

fn kept(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

/// The variables that limit a live run of the approval test: to the projects
/// whose chief is one of the harnesses named (`claude,pi`), and to the steps
/// named (`worker,question`), each a list of words.
pub const HARNESSES: &str = "CONSENSFLOW_LIVE_HARNESSES";
pub const STEPS: &str = "CONSENSFLOW_LIVE_STEPS";

/// What the variable `name` is set to, when it is set to something.
pub fn requested(name: &str) -> Option<String> {
    own_var(name).filter(|value| !value.trim().is_empty())
}

/// What a live run says as it goes, on the error output (which a test's
/// capture does not hold back), with the seconds since it began.
pub struct Log {
    name: String,
    started: Instant,
}

impl Log {
    /// A run called `name` (`live designer`), beginning now.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            started: Instant::now(),
        }
    }

    /// The same run, saying `scope` after its name: one project of it.
    #[must_use]
    pub fn scoped(&self, scope: impl Display) -> Self {
        Self {
            name: format!("{} {scope}", self.name),
            started: self.started,
        }
    }

    /// Says `text`, with the seconds since the run began.
    pub fn say(&self, text: impl Display) {
        let seconds = self.started.elapsed().as_secs_f64();
        let _ = writeln!(io::stderr(), "[{seconds:>6.1} s] {}: {text}", self.name);
    }

    /// `title`, then `body` a line at a time, indented.
    pub fn block(&self, title: &str, body: &str) {
        let indented: Vec<String> = body.lines().map(|line| format!("    {line}")).collect();
        self.say(format!("{title}:\n{}", indented.join("\n")));
    }
}

/// A JSON string as the text it is; anything else as JSON writes it.
pub fn text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// `text` on one line, and no longer than `most` characters.
pub fn cut(text: &str, most: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > most {
        let head: String = line.chars().take(most).collect();
        format!("{head}…")
    } else {
        line
    }
}

/// A read of the daemon as indented JSON, or why it could not be made.
pub fn pretty(read: Result<Value>) -> String {
    match read {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        Err(failed) => format!("(could not be read: {failed})"),
    }
}

/// How many of the daemon's log's lines that are warnings or errors a report shows.
const WARNINGS: usize = 20;

/// The lines of the daemon's log that are warnings or errors, the first few.
pub fn warnings(log: &str) -> Vec<&str> {
    log.lines()
        .filter(|line| line.contains(" warn ") || line.contains(" error "))
        .take(WARNINGS)
        .collect()
}

/// How much of an item of a transcript a report shows, how many of a
/// transcript's last items, and how much of an item's id (which tells two
/// items that read alike apart).
const SHOWN: usize = 400;
const ITEMS: usize = 20;
const ID: usize = 14;

/// What the window that had the task `task` of the project `project` recorded of
/// it, as the page's drawer shows it: each item by its role, the last few.
pub fn transcript(rig: &Rig, project: i64, task: i64) -> String {
    let read = rig.page(
        "task.transcript",
        serde_json::json!({ "project": project, "task": task }),
    );
    let items = match read {
        Ok(answer) => answer["items"].as_array().cloned().unwrap_or_default(),
        Err(failed) => return format!("(could not be read: {failed})"),
    };
    if items.is_empty() {
        return "(nothing recorded)".to_owned();
    }
    let skipped = items.len().saturating_sub(ITEMS);
    let mut lines = Vec::new();
    if skipped > 0 {
        lines.push(format!("({skipped} earlier items not shown)"));
    }
    lines.extend(items.iter().skip(skipped).map(|item| {
        format!(
            "[{} {}] {}",
            text(&item["role"]),
            cut(&text(&item["id"]), ID),
            cut(&text(&item["text"]), SHOWN)
        )
    }));
    lines.join("\n")
}

/// How often the daemon is asked how it is getting on while a run waits.
const LOOK: Duration = Duration::from_secs(1);

/// `rig.wait_for`, looking once a second (each look asks the daemon), which
/// says what it waits for, and for how long, before it does.
pub fn wait(
    rig: &Rig,
    log: &Log,
    what: &str,
    within: Duration,
    mut check: impl FnMut() -> Result<bool>,
) -> Result {
    log.say(format!("waiting up to {} s for {what}", within.as_secs()));
    let mut looked: Option<Instant> = None;
    rig.wait_for(what, within, || {
        if looked.is_some_and(|at| at.elapsed() < LOOK) {
            return Ok(false);
        }
        looked = Some(Instant::now());
        check()
    })
}

/// Where the program `name` is, on the `PATH` of this run, as a shell finds it:
/// the first folder that holds a file of its name (on Windows, of its name with
/// one of the extensions of `PATHEXT`). A machine that has none is an error that
/// says so.
pub fn command(name: &str) -> Result<PathBuf> {
    let path = own_var("PATH").unwrap_or_default();
    find(name, &path, own_var("PATHEXT").as_deref()).ok_or_else(|| Error::Program {
        action: "find",
        program: name.to_owned(),
        source: io::Error::new(
            io::ErrorKind::NotFound,
            "it is in no folder of the PATH of this run: install it, or put its folder on the PATH",
        ),
    })
}

/// [`command`] on the `path` and the extensions it is given.
fn find(command: &str, path: &str, extensions: Option<&str>) -> Option<PathBuf> {
    let suffixes: Vec<&str> = if cfg!(windows) {
        extensions
            .unwrap_or(".COM;.EXE;.BAT;.CMD")
            .split(';')
            .filter(|extension| !extension.is_empty())
            .collect()
    } else {
        vec![""]
    };
    std::env::split_paths(path).find_map(|folder| {
        suffixes
            .iter()
            .map(|suffix| folder.join(format!("{command}{suffix}")))
            .find(|candidate| candidate.is_file())
    })
}

/// Makes `folder` a git repository, as `git init` does, in the folders made for
/// it: what a live test's demo project begins as.
pub fn git_repository(folder: &Path) -> Result {
    files::make_dir(folder)?;
    let ran = Run::new(command("git")?)
        .args(["init", "--quiet"])
        .cwd(folder)
        .inheriting_env()
        .run()?;
    if ran.code != Some(0) {
        return Err(Error::Program {
            action: "run",
            program: "git init".to_owned(),
            source: io::Error::other(ran.to_string()),
        });
    }
    Ok(())
}

/// A harness the live tests run: the five of ConsensFlow's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Harness {
    Claude,
    OpenCode,
    Pi,
    Devin,
    Codex,
}

impl Harness {
    /// Every harness, in the order the live approval test rotates them through
    /// its positions: each is the chief of one project and the one after it
    /// is that project's worker.
    pub const ALL: [Harness; 5] = [
        Harness::Claude,
        Harness::OpenCode,
        Harness::Pi,
        Harness::Devin,
        Harness::Codex,
    ];

    /// What it is called on a command line, and what its command is called.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::OpenCode => "opencode",
            Self::Pi => "pi",
            Self::Devin => "devin",
            Self::Codex => "codex",
        }
    }

    /// The harness a word on a command line names.
    pub fn from_name(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|harness| harness.name() == word)
    }

    /// What the roster calls its kind.
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Claude => "claude-code",
            other => other.name(),
        }
    }

    /// Where its login is, below the home it reads it from: the first that is
    /// there says the machine has been set up for it. Codex's home is its own
    /// folder, which is all there is to look for.
    const fn login_marks(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[".claude.json", ".claude"],
            Self::OpenCode => &[".local/share/opencode", ".config/opencode"],
            Self::Pi => &[".pi"],
            Self::Devin => &[".config/devin", ".local/share/devin"],
            Self::Codex => &[""],
        }
    }

    /// What to do about a machine that has no login for it.
    const fn log_in(self) -> &'static str {
        match self {
            Self::Claude => "log in by running `claude` once",
            Self::OpenCode => "log in by running `opencode` once",
            Self::Pi => "log in by running `pi` once",
            Self::Devin => "log in with `devin auth login`",
            Self::Codex => "log in with `codex login`, or name its home in CODEX_HOME",
        }
    }
}

/// A harness installed on this machine, as its windows run it: its command,
/// the home its login is read from, and the folder of Node, which one that npm
/// installed needs on the `PATH` to start (on Windows its command is a shim
/// that starts Node).
///
/// A window reads its login by variables of its environment, which the rig
/// sets to a home of its own: Codex's is `CODEX_HOME`, and its `home` is that
/// folder; every other one's is the user's own home (`HOME`, with the folders
/// of the XDG layout below it), and its `home` is that.
#[derive(Debug, Clone)]
pub struct Installed {
    harness: Harness,
    command: PathBuf,
    home: PathBuf,
    node: Option<PathBuf>,
}

impl Installed {
    /// A `harness` whose command is `command` and whose login is read from `home`.
    pub fn at(harness: Harness, command: impl Into<PathBuf>, home: impl Into<PathBuf>) -> Self {
        Self {
            harness,
            command: command.into(),
            home: home.into(),
            node: None,
        }
    }

    /// This harness, whose windows also find Node in the folder `node`.
    #[must_use]
    pub fn with_node(mut self, node: impl Into<PathBuf>) -> Self {
        self.node = Some(node.into());
        self
    }

    /// Which harness it is.
    pub fn harness(&self) -> Harness {
        self.harness
    }

    /// Where its command is.
    pub fn command(&self) -> &Path {
        &self.command
    }

    /// The home its login is read from.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The version it says it is, as its first line says it; `unknown` when it
    /// says none.
    pub fn version(&self) -> String {
        let word = if self.harness == Harness::Devin {
            "version"
        } else {
            "--version"
        };
        Run::new(&self.command)
            .arg(word)
            .inheriting_env()
            .run()
            .ok()
            .filter(|ran| ran.code == Some(0))
            .and_then(|ran| ran.stdout.lines().next().map(|line| line.trim().to_owned()))
            .filter(|line| !line.is_empty())
            .unwrap_or_else(|| "unknown".to_owned())
    }

    /// The folders its windows need on their `PATH`: its command's, and Node's
    /// where that is another.
    fn folders(&self) -> Vec<&Path> {
        let folder = self.command.parent().unwrap_or_else(|| Path::new(""));
        let mut folders = vec![folder];
        if let Some(node) = self.node.as_deref().filter(|node| *node != folder) {
            folders.push(node);
        }
        folders
    }

    /// The variables that make its windows read the machine's login, where the
    /// rig would have them read its own home's: `machine` says what a variable
    /// is on this machine.
    fn login(&self, machine: &dyn Fn(&str) -> Option<String>) -> Vec<(String, String)> {
        let home = self.home.to_string_lossy().into_owned();
        if self.harness == Harness::Codex {
            return vec![("CODEX_HOME".to_owned(), home)];
        }
        let mut vars = vec![("HOME".to_owned(), home.clone())];
        if cfg!(windows) {
            vars.push(("USERPROFILE".to_owned(), home.clone()));
        }
        // The rig puts the config folder below its own home: where the machine
        // keeps it, or where the XDG layout does, is where these read theirs.
        let config = machine("XDG_CONFIG_HOME")
            .filter(|folder| !folder.is_empty())
            .unwrap_or_else(|| self.home.join(".config").to_string_lossy().into_owned());
        vars.push(("XDG_CONFIG_HOME".to_owned(), config));
        // The others default below the home, which is the machine's now; one
        // the machine names is its own.
        for name in ["XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"] {
            if let Some(folder) = machine(name).filter(|folder| !folder.is_empty()) {
                vars.push((name.to_owned(), folder));
            }
        }
        vars
    }

    /// A rig whose windows run the stand-in `stand_in` for Claude and find this
    /// harness, logged in as the machine is.
    pub fn rig(&self, stand_in: impl Into<PathBuf>) -> Config {
        rig(std::slice::from_ref(self), stand_in)
    }
}

/// A rig whose windows run the stand-in `stand_in` for Claude and find every
/// harness of `installs`, logged in as the machine is: each one's command's
/// folder, and Node's where it is another, are on their `PATH` (in the order
/// given, none twice), and the variables they read their logins by are the
/// machine's. Claude Code reads its config where `CLAUDE_CONFIG_DIR` does not
/// point, so a rig that has it has the variable unset.
pub fn rig(installs: &[Installed], stand_in: impl Into<PathBuf>) -> Config {
    let machine = |name: &str| own_var(name);
    let mut config = Config::new(stand_in);
    let mut folders: Vec<&Path> = Vec::new();
    for install in installs {
        for folder in install.folders() {
            if !folders.contains(&folder) {
                folders.push(folder);
                config = config.also_on_path(folder);
            }
        }
        for (name, value) in install.login(&machine) {
            config = config.var(&name, value);
        }
        if install.harness == Harness::Claude {
            config = config.unset("CLAUDE_CONFIG_DIR");
        }
    }
    config
}

/// The flags that keep a scripted Claude window from reaching the human's MCP
/// servers, connectors and browser, which it would in full-permission mode on
/// their own setup. The product leaves all of them on for every agent, and
/// test runs are the only place they are shut out.
const ISOLATION: [&str; 2] = ["--strict-mcp-config", "--no-chrome"];

/// A program called `claude` that runs the real Claude Code at `claude` with
/// [`ISOLATION`] before whatever words it is given, made in `folder`: the
/// stand-in a rig puts first on the `PATH` of its windows is then this one,
/// and every Claude window of the run starts through it. Where it is.
pub fn isolated_claude(folder: &Path, claude: &Path) -> Result<PathBuf> {
    if cfg!(windows) {
        return Err(Error::Program {
            action: "isolate",
            program: claude.to_string_lossy().into_owned(),
            source: io::Error::new(
                io::ErrorKind::Unsupported,
                "a live run that starts Claude Code through a wrapper is for macOS and Linux",
            ),
        });
    }
    let program = folder.join("claude-isolated");
    files::write_executable(&program, isolating_script(claude))?;
    Ok(program)
}

/// The shell script that is [`isolated_claude`].
fn isolating_script(claude: &Path) -> String {
    format!(
        "#!/bin/sh\n# Written by a live test: its Claude windows reach no MCP server and no browser.\nexec {} {} \"$@\"\n",
        shell_word(&claude.to_string_lossy()),
        ISOLATION.join(" ")
    )
}

/// `word` as one word of a shell script, whatever it holds.
fn shell_word(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// The machine's `harness`, found as a person's shell finds it, with the home
/// its login is read from. A machine without it, or without the home it logs
/// in at, is an error that says what to do.
pub fn installed(harness: Harness) -> Result<Installed> {
    let name = harness.name();
    let command = command(name)?;
    let home = home_of(
        harness,
        own_var("CODEX_HOME"),
        own_var("HOME").or_else(|| own_var("USERPROFILE")),
    )
    .ok_or_else(|| Error::Program {
        action: "find the home of",
        program: name.to_owned(),
        source: io::Error::new(
            io::ErrorKind::NotFound,
            "this run has neither a home nor CODEX_HOME to look for its login in",
        ),
    })?;
    if !has_login(harness, &home) {
        return Err(Error::File {
            action: "find",
            path: home,
            source: io::Error::new(
                io::ErrorKind::NotFound,
                format!("{name} keeps its login there: {}", harness.log_in()),
            ),
        });
    }
    let installed = Installed::at(harness, command, home);
    // A machine with no Node has a harness that needs none (a native one).
    Ok(match command_folder("node") {
        Some(node) => installed.with_node(node),
        None => installed,
    })
}

/// The machine's Codex: [`installed`] for the harness the image designer runs on.
pub fn codex() -> Result<Installed> {
    installed(Harness::Codex)
}

/// Whether `home` holds what `harness` keeps its login in.
fn has_login(harness: Harness, home: &Path) -> bool {
    harness
        .login_marks()
        .iter()
        .any(|mark| home.join(mark).exists())
}

/// The folder the program `name` is in, if it is on the `PATH`.
fn command_folder(name: &str) -> Option<PathBuf> {
    command(name).ok()?.parent().map(Path::to_path_buf)
}

/// The home `harness` reads its login from: Codex's is `CODEX_HOME` when it is
/// set, otherwise `.codex` in the user's home; every other one's is the user's
/// home. Made whole against the folder the run is in.
fn home_of(
    harness: Harness,
    codex_home: Option<String>,
    user_home: Option<String>,
) -> Option<PathBuf> {
    let named = |value: Option<String>| value.filter(|value| !value.is_empty());
    let home = match (harness, named(codex_home)) {
        (Harness::Codex, Some(home)) => PathBuf::from(home),
        (Harness::Codex, None) => Path::new(&named(user_home)?).join(".codex"),
        (_, _) => PathBuf::from(named(user_home)?),
    };
    Some(std::path::absolute(&home).unwrap_or(home))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::*;

    /// The name a command's file has on this platform.
    fn file_of(command: &str) -> String {
        if cfg!(windows) {
            format!("{command}.cmd")
        } else {
            command.to_owned()
        }
    }

    fn path_of(folders: &[&Path]) -> String {
        std::env::join_paths(folders)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn a_command_is_the_first_file_of_its_name_on_the_path() {
        let (first, second, empty) = (
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let name = file_of("codex");
        fs::write(second.path().join(&name), "second").unwrap();
        let path = path_of(&[empty.path(), first.path(), second.path()]);
        // (On Windows the extension is the one `PATHEXT` names, as it names it.)
        assert_eq!(
            find("codex", &path, Some(".cmd")),
            Some(second.path().join(&name))
        );
        // Another folder has one before it: that one is the command.
        fs::write(first.path().join(&name), "first").unwrap();
        assert_eq!(
            find("codex", &path, Some(".cmd")),
            Some(first.path().join(&name))
        );
    }

    #[test]
    fn a_folder_of_the_name_is_no_command_and_a_command_that_is_not_there_is_none() {
        let folder = tempfile::tempdir().unwrap();
        fs::create_dir(folder.path().join(file_of("codex"))).unwrap();
        let path = path_of(&[folder.path(), Path::new("no-such-folder")]);
        assert_eq!(find("codex", &path, None), None);
        assert_eq!(find("claude", &path, None), None);
        assert_eq!(find("codex", "", None), None);
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_a_command_is_the_file_with_an_extension_of_pathext_in_its_order() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("codex.cmd"), "").unwrap();
        fs::write(folder.path().join("codex.exe"), "").unwrap();
        fs::write(folder.path().join("codex"), "").unwrap();
        let path = path_of(&[folder.path()]);
        assert_eq!(
            find("codex", &path, Some(".CMD;.EXE")),
            Some(folder.path().join("codex.CMD"))
        );
        assert_eq!(
            find("codex", &path, Some(".EXE;.CMD")),
            Some(folder.path().join("codex.EXE"))
        );
        assert_eq!(find("codex", &path, Some(".BAT")), None, "no bare name");
    }

    #[test]
    fn codex_logs_in_at_its_home_when_named_and_in_the_dot_codex_of_the_users_otherwise() {
        let home = |codex: Option<&str>, user: Option<&str>| {
            home_of(
                Harness::Codex,
                codex.map(str::to_owned),
                user.map(str::to_owned),
            )
        };
        let given = std::env::temp_dir().join("elsewhere");
        let user = std::env::temp_dir().join("user");
        let (given_text, user_text) = (given.to_string_lossy(), user.to_string_lossy());
        assert_eq!(home(Some(&given_text), Some(&user_text)), Some(given));
        assert_eq!(home(None, Some(&user_text)), Some(user.join(".codex")));
        // A variable that is set to nothing is not set.
        assert_eq!(home(Some(""), Some(&user_text)), Some(user.join(".codex")));
        assert_eq!(home(None, None), None);
        assert_eq!(home(None, Some("")), None);
    }

    #[test]
    fn every_other_harness_logs_in_at_the_users_home_whatever_codex_home_says() {
        let user = std::env::temp_dir().join("user");
        let user_text = user.to_string_lossy().into_owned();
        for harness in Harness::ALL.into_iter().filter(|h| *h != Harness::Codex) {
            let home = |codex: Option<&str>, user: Option<&str>| {
                home_of(harness, codex.map(str::to_owned), user.map(str::to_owned))
            };
            assert_eq!(
                home(Some("elsewhere"), Some(&user_text)),
                Some(user.clone())
            );
            assert_eq!(home(None, Some(&user_text)), Some(user.clone()));
            assert_eq!(home(Some("elsewhere"), None), None, "{harness:?}");
            assert_eq!(home(None, Some("")), None, "{harness:?}");
        }
    }

    #[test]
    fn a_home_named_by_a_relative_path_is_made_whole_against_the_folder_of_the_run() {
        let home = home_of(Harness::Codex, Some("login".to_owned()), None).unwrap();
        assert!(home.is_absolute(), "{}", home.display());
        assert!(home.ends_with("login"));
    }

    #[test]
    fn a_live_run_keeps_what_it_made_when_the_variable_says_anything_but_nothing_or_zero() {
        assert!(!kept(None));
        assert!(!kept(Some("")));
        assert!(!kept(Some("0")));
        assert!(kept(Some("1")));
        assert!(kept(Some("yes")));
    }

    /// A run of the workspace's tests that found a live test not ignored would
    /// spend the machine's quota, on every developer's machine that has the
    /// harnesses, and fail on every one that has not.
    #[test]
    fn every_live_test_is_ignored_unless_asked_for() {
        let folder = crate::checkout::path("crates/cf-e2e/tests");
        let mut live_files = 0;
        for entry in fs::read_dir(&folder).unwrap() {
            let file = entry.unwrap().path();
            let name = file.file_name().unwrap().to_string_lossy().into_owned();
            if !name.starts_with("live_") || !name.ends_with(".rs") {
                continue;
            }
            live_files += 1;
            let source = fs::read_to_string(&file).unwrap();
            // What a live file tests of its own helpers, below its `#[cfg(test)]`,
            // runs nothing live.
            let live = source.split("#[cfg(test)]").next().unwrap();
            let (tests, ignored) = (
                live.matches("#[test]").count(),
                live.matches("#[ignore").count(),
            );
            assert!(tests > 0, "{name} holds no live test");
            assert_eq!(
                tests, ignored,
                "{name}: a live test is ignored unless asked for"
            );
        }
        assert!(live_files > 0, "no live test file in {}", folder.display());
    }

    #[test]
    fn a_program_that_is_on_no_folder_of_the_path_is_an_error_that_names_it() {
        let failed = command("no-such-program-for-the-live-tests").unwrap_err();
        assert!(
            matches!(&failed, Error::Program { action: "find", program, .. }
                if program == "no-such-program-for-the-live-tests"),
            "{failed}"
        );
        assert!(
            failed.to_string().starts_with(
                "could not find `no-such-program-for-the-live-tests`: it is in no folder"
            ),
            "{failed}"
        );
    }

    #[test]
    fn an_installed_harness_is_its_command_and_the_home_it_logs_in_at() {
        let command = Path::new("bin").join(file_of("codex"));
        let codex = Installed::at(Harness::Codex, &command, "login");
        assert_eq!(codex.harness(), Harness::Codex);
        assert_eq!(codex.command(), command);
        assert_eq!(codex.home(), Path::new("login"));
    }

    #[cfg(unix)]
    #[test]
    fn a_harness_says_its_version_by_the_word_it_is_asked_with_and_unknown_when_it_says_none() {
        let pi = Installed::at(Harness::Pi, "/bin/echo", "home");
        assert_eq!(pi.version(), "--version");
        // Devin's is a command of its own, not a flag.
        let devin = Installed::at(Harness::Devin, "/bin/echo", "home");
        assert_eq!(devin.version(), "version");
        let failing = Installed::at(Harness::Pi, "/usr/bin/false", "home");
        assert_eq!(failing.version(), "unknown");
        let gone = Installed::at(Harness::Pi, "/no/such/program", "home");
        assert_eq!(gone.version(), "unknown");
    }

    #[test]
    fn the_harnesses_are_named_as_a_command_line_and_the_roster_name_them() {
        let names: Vec<_> = Harness::ALL.iter().map(|harness| harness.name()).collect();
        assert_eq!(names, ["claude", "opencode", "pi", "devin", "codex"]);
        for harness in Harness::ALL {
            assert_eq!(Harness::from_name(harness.name()), Some(harness));
        }
        assert_eq!(Harness::from_name("Claude"), None);
        assert_eq!(Harness::from_name("claude-code"), None);
        assert_eq!(Harness::from_name(""), None);
        assert_eq!(Harness::Claude.kind(), "claude-code");
        assert_eq!(Harness::Pi.kind(), "pi");
        assert_eq!(Harness::Codex.kind(), "codex");
    }

    #[test]
    fn a_home_holds_a_login_when_it_holds_the_place_the_harness_keeps_it() {
        let home = tempfile::tempdir().unwrap();
        for harness in Harness::ALL.into_iter().filter(|h| *h != Harness::Codex) {
            assert!(!has_login(harness, home.path()), "{harness:?}");
        }
        fs::write(home.path().join(".claude.json"), "{}").unwrap();
        assert!(has_login(Harness::Claude, home.path()));
        assert!(!has_login(Harness::Pi, home.path()));
        fs::create_dir_all(home.path().join(".config").join("opencode")).unwrap();
        assert!(has_login(Harness::OpenCode, home.path()));
        // Codex's home is the folder itself: any folder that is there.
        assert!(has_login(Harness::Codex, home.path()));
        assert!(!has_login(Harness::Codex, &home.path().join("missing")));
    }

    #[test]
    fn a_window_reads_codexs_login_by_codex_home_and_every_others_by_the_machines_home() {
        let nothing = |_: &str| None;
        let codex = Installed::at(Harness::Codex, "bin/codex", "login");
        assert_eq!(
            codex.login(&nothing),
            [("CODEX_HOME".to_owned(), "login".to_owned())]
        );
        let pi = Installed::at(Harness::Pi, "bin/pi", "machine");
        let vars = pi.login(&nothing);
        let var = |name: &str| {
            vars.iter()
                .rev()
                .find(|(given, _)| given == name)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(var("HOME").as_deref(), Some("machine"));
        let config = Path::new("machine").join(".config");
        assert_eq!(
            var("XDG_CONFIG_HOME").as_deref(),
            Some(config.to_string_lossy().as_ref())
        );
        assert_eq!(var("XDG_DATA_HOME"), None, "the default is below the home");
        assert_eq!(var("CODEX_HOME"), None);
        // What the machine names for itself is what its windows read.
        let machine = |name: &str| match name {
            "XDG_CONFIG_HOME" => Some("/x/config".to_owned()),
            "XDG_DATA_HOME" => Some("/x/data".to_owned()),
            "XDG_CACHE_HOME" => Some(String::new()),
            _ => None,
        };
        let vars = pi.login(&machine);
        let var = |name: &str| {
            vars.iter()
                .rev()
                .find(|(given, _)| given == name)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(var("XDG_CONFIG_HOME").as_deref(), Some("/x/config"));
        assert_eq!(var("XDG_DATA_HOME").as_deref(), Some("/x/data"));
        assert_eq!(
            var("XDG_CACHE_HOME"),
            None,
            "a variable set to nothing is not set"
        );
    }

    #[test]
    fn a_harness_needs_its_commands_folder_and_nodes_where_that_is_another() {
        let pi = Installed::at(Harness::Pi, "/opt/bin/pi", "home");
        assert_eq!(pi.folders(), [Path::new("/opt/bin")]);
        let with_node = pi.clone().with_node("/node/bin");
        assert_eq!(
            with_node.folders(),
            [Path::new("/opt/bin"), Path::new("/node/bin")]
        );
        let together = pi.with_node("/opt/bin");
        assert_eq!(together.folders(), [Path::new("/opt/bin")]);
    }

    #[test]
    fn claude_is_started_through_a_wrapper_that_adds_the_flags_and_keeps_every_word_given() {
        let script = isolating_script(Path::new("/home/me/.local/bin/claude"));
        assert_eq!(
            script,
            "#!/bin/sh\n# Written by a live test: its Claude windows reach no MCP server and no browser.\n\
             exec '/home/me/.local/bin/claude' --strict-mcp-config --no-chrome \"$@\"\n"
        );
        // A path with a quote in it is still one word.
        assert_eq!(shell_word("it's"), r"'it'\''s'");
        assert_eq!(shell_word("a b"), "'a b'");
    }

    #[cfg(unix)]
    #[test]
    fn the_wrapper_is_a_program_that_runs_the_claude_it_was_made_for_with_the_flags_first() {
        let folder = tempfile::tempdir().unwrap();
        let program = isolated_claude(folder.path(), Path::new("/bin/echo")).unwrap();
        assert_eq!(program, folder.path().join("claude-isolated"));
        let ran = crate::process::Run::new(&program)
            .args(["--settings", "a b"])
            .run()
            .unwrap();
        assert_eq!(
            ran.stdout,
            "--strict-mcp-config --no-chrome --settings a b\n"
        );
    }

    #[test]
    fn a_rig_for_several_harnesses_has_each_ones_folders_once_and_claude_without_its_config_dir() {
        let claude = Installed::at(Harness::Claude, "/opt/bin/claude", "/machine");
        let pi = Installed::at(Harness::Pi, "/opt/bin/pi", "/machine").with_node("/node/bin");
        let codex = Installed::at(Harness::Codex, "/codex/bin/codex", "/machine/.codex");
        let debug = format!("{:?}", rig(&[claude, pi, codex], "/the/stand-in"));
        // The folders the rig puts on the PATH, in order, none twice.
        let on_path = debug
            .split("path: [")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .unwrap();
        assert_eq!(
            on_path.replace(['"', ' '], ""),
            "/opt/bin,/node/bin,/codex/bin"
        );
        assert!(debug.contains("unset: [\"CLAUDE_CONFIG_DIR\"]"), "{debug}");
        assert!(
            debug.contains("(\"CODEX_HOME\", \"/machine/.codex\")"),
            "{debug}"
        );
        // A rig that has no Claude leaves the variable the stand-in uses.
        let plain = format!(
            "{:?}",
            rig(
                &[Installed::at(Harness::Pi, "/p/pi", "/m")],
                "/the/stand-in"
            )
        );
        assert!(plain.contains("unset: []"), "{plain}");
    }

    #[test]
    fn only_the_warnings_and_errors_of_a_log_are_kept_and_at_most_so_many() {
        let log = "t info start\nt warn slow pass\nt error it broke\nt info done\n";
        assert_eq!(warnings(log), ["t warn slow pass", "t error it broke"]);
        assert!(warnings("t info start\n").is_empty());
        let many: String = (0..30).map(|at| format!("t warn {at}\n")).collect();
        assert_eq!(warnings(&many).len(), WARNINGS);
    }

    #[test]
    fn what_cannot_be_read_says_so_and_what_can_is_indented_json() {
        assert_eq!(pretty(Ok(json!({ "a": 1 }))), "{\n  \"a\": 1\n}");
        let failed = pretty(Err(Error::Daemon("no answer".to_owned())));
        assert_eq!(failed, "(could not be read: no answer)");
    }

    #[test]
    fn what_the_product_answered_fits_a_line_cut_where_it_is_too_long() {
        assert_eq!(cut("a\n  b\t c", 10), "a b c");
        assert_eq!(cut("abcdef", 3), "abc…");
        assert_eq!(cut("abc", 3), "abc");
        // Counted in characters, not bytes.
        assert_eq!(cut("ééé", 2), "éé…");
        // A JSON string as the text it is; anything else as JSON writes it.
        assert_eq!(text(&json!("a b")), "a b");
        assert_eq!(text(&json!(7)), "7");
        assert_eq!(text(&json!(null)), "null");
        assert_eq!(text(&json!({ "a": 1 })), r#"{"a":1}"#);
    }
}
