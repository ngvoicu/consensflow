//! Live: which harness asks whether to trust a folder before it starts, how the
//! question looks, and whether ConsensFlow answers it. It is opt-in (`cargo xtask
//! live folder-trust`, `npm run live:folder-trust`): it opens real windows of every
//! harness named, and a project with a real chief of each that asks, so it is
//! ignored unless asked for and is no part of `check` or of CI.
//!
//! dip-agentos's workers stopped at Claude Code's "Do you trust this folder?" after
//! `git init` (2026-10-19), and the owner's decision of 2026-10-20 has ConsensFlow
//! answer it in the project's own folder. Its first runs (2026-10-21: Claude Code
//! 2.1.305, Codex 0.163.0, Pi 1.3.0, OpenCode 1.19.3, Devin 3000.11.92) found:
//!
//! - Claude Code asks in a git repository it was never told to trust, whatever the
//!   folders above it, and in a git repository below a trusted one; not in a plain
//!   folder below a trusted one, nor in a plain folder of a trusted repository. (A
//!   plain folder with nothing trusted above it asks as well: the owner's own config
//!   trusts their home, so on it none ever does. The run's config is seeded as the
//!   owner's would be, the live folders' root trusted, T-629 and T-681, so a plain
//!   folder of the run asks nothing.) Its
//!   question opens on "No, exit"; Down, then Enter, says yes, which it writes down
//!   for the repository's root (its question names the window's own folder), so a
//!   window opened anywhere in it again does not ask.
//! - Claude Code asks again, in the same dialog, of a folder trusted by a folder above
//!   it whose own settings grant more than its trust (2.1.309, T-354: a plain folder
//!   below a trusted one, its `.claude/settings.json` pre-approving a tool; so would a
//!   server of its `.mcp.json` with a header helper, which a window shut out of the
//!   owner's MCP servers never reads). Its no reads "No, continue without these
//!   permissions" and goes on without them; its yes lets them apply without asking, and
//!   records as trusted the repository's root, or the folder out of one (T-381). The
//!   owner's decision of 2026-10-23 has ConsensFlow answer it as the first question: yes
//!   where that folder is the project's or below it, the window then at its prompt;
//!   elsewhere (a folder of a repository made after the folder was answered, whose yes
//!   would trust the repository) the engine must press nothing, and the window must
//!   still ask after.
//! - Claude Code, once a folder is trusted, asks of a server of its `.mcp.json` it was
//!   never told to use in a dialog of its own (2.1.309, T-383): "New MCP server found in
//!   this project", its `❯` on "Continue without using this MCP server", Up moving it to
//!   "Use this MCP server", which keeps the server's name in the folder's
//!   `.claude/settings.local.json` and starts it. It reads the `.mcp.json` of every
//!   folder above as well. A window shut out of MCP servers (`--strict-mcp-config`)
//!   never asks, nor one whose person accepted full-permission mode in their own
//!   settings (the owner's do): the run's cases with a server open their windows
//!   without that flag, their connectors shut out otherwise. The owner's decision of
//!   2026-10-23 has ConsensFlow use the server in the project's own folder (a new
//!   repository: the folder answered, then the server used, the window at its prompt;
//!   with two servers, both used through "Enable selected"; opened again, nothing
//!   asked); where a folder above the project's has a `.mcp.json` of its own the engine
//!   must press nothing into the dialog, and the window must still ask after.
//! - Codex opened alone (`codex --dangerously-bypass-approvals-and-sandbox`) asks in
//!   the same folders before it starts a thread ("Trust this folder?", its `›` on
//!   "Trust and continue", which Enter takes, a yes for the repository's root, which
//!   it names). Codex as ConsensFlow opens it, its window on the broker's server
//!   (`cf codex-session`), asks nothing, chief and worker alike: there is nothing to
//!   answer, and a Codex that starts asking there is what the projects part catches.
//! - Pi, OpenCode and Devin (`--respect-workspace-trust false`) never ask.
//!
//! Two parts. **windows**: the window of each harness whose bare window is the app's
//! (Claude Code, Pi, OpenCode, Devin: Codex's is not), opened as the app opens one (its
//! own command line for a new conversation; a Claude window shut out of the owner's MCP
//! servers and browser, and not asked to trust its folder first), in folders of its own
//! made fresh for the run, below the live folders: a plain folder, and a plain folder
//! below it whose settings pre-approve a tool once it is answered; a plain folder
//! answered, then made a folder of a new repository, its settings pre-approving a tool;
//! a new repository
//! (`git init`), and the same again once answered; a repository, then a plain folder
//! and a new repository below it once it is trusted; a plain folder of a repository
//! nobody trusted; a linked worktree (`git worktree add`) of a repository nobody
//! trusted; and a submodule (`git submodule add`) of one. Its screen is read as the
//! engine reads it (the last lines the pane host sends, through the harness code's own
//! reader, `harness-ask trust`); a window that asks is answered with the keys the
//! engine presses, and must then be at its prompt as the engine reads it (for Claude
//! Code, the reader its watch ends on: a prompt drawn otherwise fails the run). Claude
//! Code must keep the yes for the folder the engine says a yes trusts, and for no other
//! (read from its own record of folders, which the run never writes): in a linked
//! worktree that is the checkout it belongs to, in a submodule the submodule itself. A
//! screen that speaks of trust the reader does not read as the question, and a harness
//! that asks where it should not or does not where it should, are failures.
//! For Claude Code the part also ends three windows in a new repository and reads what
//! the pane host kept of each screen, the `tail` of its `pane.exit` (what the engine reads
//! of a window that ended before its next look, `yes_at_exit`) through the same reader:
//! answered No (Enter on "No, exit", and Escape), a window ends with no prompt on that
//! screen, so no yes is read from it; answered yes and closed at its prompt, the screen
//! shows the prompt and not the question's choices, which is a yes.
//! **projects**: for Claude Code and Codex, a project through the daemon in a new
//! repository nobody trusted: its chief a real window of the harness; a worker of it
//! given a task beside a stand-in chief; and a worker whose first task is done in a
//! plain folder, which is then made a repository, and whose follow-up opens its window
//! again on its conversation. The chief must come up idle, the tasks be done and the
//! follow-up's window be a resumed one, and the human's notes must say that ConsensFlow
//! answered yes once for Claude Code, and never for Codex. **resumed**: a Claude window
//! opened again on a long conversation of the run's own (a chief's after a restart), in a
//! new repository nobody trusted; its question answered as the engine answers it, its
//! screen must show neither the question nor its prompt for less than the 12 s the engine
//! waits before it counts the window past the question. Its conversations are written
//! where the rig's Claude Code keeps its own, in the run's config folder (about 540 MB),
//! and taken away as each window ends; the rest goes with the rig.
//!
//! The run writes what it measured and its checks to `report.txt` beside its folders
//! (it says where), besides its error output.
//!
//! Claude Code's windows run on a config folder of the run's own, with a copy of the
//! machine's login ([`cf_e2e::live::login`]): what it writes of a yes is in that
//! folder, which goes with each rig, and nothing of it reaches the owner's `~/.claude`.
//! A fresh folder has trusted nothing, so the questions are the run's own. Codex writes
//! nothing here.
//!
//! ```text
//! cargo xtask live folder-trust                       every harness, both parts
//! cargo xtask live folder-trust claude codex          the ones named
//! cargo xtask live folder-trust --part windows        one part
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::machine::live_folder_in;
use cf_e2e::live::members::{member, member_as, named, stand_in_chief, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Open, Window};
use cf_e2e::live::{self, agents, ask, Harness, Installed, Log};
use cf_e2e::process::Run;
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};
use regex::Regex;
use serde_json::json;

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows: no
/// window of this test runs it, but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The folder of the run's windows, below the live folders: a word that is no
/// harness's, so a screen that names the folder speaks of no trust.
const FOLDER: &str = "asks-probe";

/// How long a window has to draw, and to draw what follows a key.
const DRAWS: Duration = Duration::from_secs(60);

/// How long the daemon and the pane host have to end when a rig is closed: a real
/// harness's window takes longer than the stand-ins'.
const CLOSES: Duration = Duration::from_secs(30);

/// How many keys a question is given before it counts as not answered: the engine's own.
const MOST_KEYS: usize = 6;

/// The lines of a window's screen the engine reads: the pane host's tail.
const TAIL: usize = 12;

/// The parts of the run.
const PARTS: [&str; 3] = ["windows", "projects", "resumed"];

/// The folders the resumed part opens its windows in end with this and a number.
const RESUMED_FOLDER: &str = "trust-resumed";

/// How long the engine lets a window that asked show neither the question nor its
/// prompt before it counts it past the question: its own (`GONE_LOOKS` looks of 500 ms).
const GONE: Duration = Duration::from_secs(12);

/// The conversations the resumed part opens a window again on: how many turns before
/// its last compaction and after it, and how many kilobytes each turn's command printed.
/// A chief's after days of work: 470 MB, 13,000 messages, compacted (this project's
/// chief, 2026-10-20), four messages a turn here; and one a tenth as long.
const RESUMED: [(&str, usize, usize, usize); 2] = [
    ("a tenth of a chief's", 240, 85, 140),
    ("a chief's", 2_400, 850, 140),
];

/// The `.mcp.json` of the cases with a server: one that starts and ends at once; and of
/// the case with two.
const SERVER: &str = r#"{"mcpServers":{"probe":{"command":"/usr/bin/true"}}}"#;
const SERVERS: &str =
    r#"{"mcpServers":{"probe":{"command":"/usr/bin/true"},"probe2":{"command":"/usr/bin/true"}}}"#;

/// A folder a window is opened in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    /// A plain folder.
    Fresh,
    /// A plain folder below the answered plain one, its `.claude/settings.json`
    /// pre-approving a tool.
    Granted,
    /// A plain folder answered, then made a folder of a new repository, its
    /// `.claude/settings.json` pre-approving a tool: a yes would trust the repository.
    GrantedInRepository,
    /// A new repository.
    Repository,
    /// The same repository, opened again once answered.
    Again,
    /// A repository, answered: the parent of the two below.
    Parent,
    /// A plain folder of the answered repository.
    Below,
    /// A new repository below the answered one.
    RepositoryBelow,
    /// A plain folder of a repository nobody trusted.
    Untrusted,
    /// A linked worktree of a repository nobody trusted (`git worktree add`).
    Worktree,
    /// A submodule of a repository nobody trusted (`git submodule add`).
    Submodule,
    /// A new repository whose `.mcp.json` names a server.
    Server,
    /// The same repository, opened again once its server is used.
    ServerAgain,
    /// A new repository whose `.mcp.json` names two servers.
    Servers,
    /// A plain folder below one whose `.mcp.json` names a server.
    ServerAbove,
}

impl Case {
    const ALL: [Case; 15] = [
        Case::Fresh,
        Case::Granted,
        Case::GrantedInRepository,
        Case::Repository,
        Case::Again,
        Case::Parent,
        Case::Below,
        Case::RepositoryBelow,
        Case::Untrusted,
        Case::Worktree,
        Case::Submodule,
        Case::Server,
        Case::ServerAgain,
        Case::Servers,
        Case::ServerAbove,
    ];

    const fn name(self) -> &'static str {
        match self {
            Case::Fresh => "fresh",
            Case::Granted => "plain below with pre-approved tools",
            Case::GrantedInRepository => "answered, then in a repository, with pre-approved tools",
            Case::Repository => "repository",
            Case::Again => "repository again",
            Case::Parent => "parent",
            Case::Below => "plain below",
            Case::RepositoryBelow => "repository below",
            Case::Untrusted => "untrusted's folder",
            Case::Worktree => "linked worktree",
            Case::Submodule => "submodule",
            Case::Server => "repository with a server",
            Case::ServerAgain => "repository with a server again",
            Case::Servers => "repository with two servers",
            Case::ServerAbove => "plain below a folder with a server",
        }
    }

    /// Its folder below the harness's folder of the run, and the repository made
    /// for it, where it has one of its own.
    const fn folder(self) -> (&'static str, Option<&'static str>) {
        match self {
            Case::Fresh => ("fresh", None),
            Case::Granted => ("fresh/granted", None),
            // Its repository is made once the folder is answered (`look`).
            Case::GrantedInRepository => ("granted-repository/app", None),
            Case::Repository | Case::Again => ("repository", Some("repository")),
            Case::Parent => ("parent", Some("parent")),
            Case::Below => ("parent/below", None),
            Case::RepositoryBelow => ("parent/repository", Some("parent/repository")),
            Case::Untrusted => ("untrusted/folder", Some("untrusted")),
            // Its repository is the checkout it belongs to, made with it (`worktree`).
            Case::Worktree => ("worktree", None),
            // Its repository is made with it inside another (`submodule`).
            Case::Submodule => ("superproject/sub", None),
            Case::Server | Case::ServerAgain => ("server", Some("server")),
            Case::Servers => ("servers", Some("servers")),
            // Its server is the folder above's (`look`).
            Case::ServerAbove => ("server-above/app", None),
        }
    }
}

/// Whether `harness` must ask in `case`: Claude Code in a folder nobody answered above
/// (a plain one too: the config of the run trusts nothing), in a repository nobody
/// answered, and of a trusted folder's grants; nobody else, ever.
fn must_ask(harness: Harness, case: Case) -> bool {
    must_grant(harness, case)
        || harness == Harness::Claude
            && matches!(
                case,
                Case::Repository
                    | Case::Parent
                    | Case::RepositoryBelow
                    | Case::Untrusted
                    | Case::Worktree
                    | Case::Submodule
                    | Case::Server
                    | Case::Servers
                    | Case::ServerAbove
            )
}

/// Whether `harness` must ask of a server of a `.mcp.json` in `case`, once the folder is
/// answered: Claude Code where the folder or one above it names one.
fn must_serve(harness: Harness, case: Case) -> bool {
    harness == Harness::Claude && matches!(case, Case::Server | Case::Servers | Case::ServerAbove)
}

/// The servers a case's folder, or the folder above it, names: what a window of it reads
/// once it is opened with the project's servers.
fn servers_of(case: Case) -> Option<&'static str> {
    match case {
        Case::Server | Case::ServerAgain | Case::ServerAbove => Some(SERVER),
        Case::Servers => Some(SERVERS),
        _ => None,
    }
}

/// Whether `harness` must ask in `case` of what its folder's settings grant: Claude
/// Code where the folder is trusted by one above it and its settings pre-approve a tool.
fn must_grant(harness: Harness, case: Case) -> bool {
    harness == Harness::Claude && matches!(case, Case::Granted | Case::GrantedInRepository)
}

/// Opens a window of `install` in the folder `name` as `id`, answers what it asks as the
/// engine answers it in the project's folder, and closes it: whether it came to its
/// prompt.
fn answer_as_the_engine(
    rig: &Rig,
    install: &Installed,
    name: &str,
    id: &str,
    log: &Log,
) -> Result<bool> {
    let harness = install.harness();
    let window = Open::new(rig, install, name, id)
        .without_trust()
        .open(log)?;
    window.still(DRAWS);
    let mut screen = screen_of(rig, &window);
    for _ in 0..MOST_KEYS {
        let Some(key) = engine_key(harness, &screen)? else {
            break;
        };
        window.press(&key)?;
        sleep(Duration::from_millis(300));
        window.still(DRAWS);
        screen = screen_of(rig, &window);
    }
    let at_prompt = !window.closed() && engine_reads_prompt(harness, &screen)?;
    window.kill();
    sleep(Duration::from_secs(1));
    Ok(at_prompt)
}

/// Writes into Claude Code's record of folders `record` (the run's own config's
/// `.claude.json`) the yes a person said once to `directory`, as Claude Code keeps it
/// (`projects[<folder>].hasTrustDialogAccepted`), where it holds none: what a person's own
/// config has of a plain folder they were asked of. No window of the run is open then.
fn said_yes_once(record: &Path, directory: &Path) -> Result {
    let mut held: serde_json::Value = fs::read_to_string(record)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({}));
    let folder = live::claude_project_key(
        &fs::canonicalize(directory)
            .unwrap_or_else(|_| directory.to_path_buf())
            .to_string_lossy(),
    );
    if !held["projects"].is_object() {
        held["projects"] = json!({});
    }
    held["projects"][&folder]["hasTrustDialogAccepted"] = json!(true);
    fs::write(record, held.to_string()).map_err(|source| Error::File {
        action: "write",
        path: record.to_path_buf(),
        source,
    })
}

/// Writes the `.claude/settings.json` of `directory`, which pre-approves a tool.
fn pre_approve(directory: &Path) -> Result {
    let settings = directory.join(".claude");
    cf_e2e::files::make_dir(&settings)?;
    let granted = json!({ "permissions": { "allow": ["Bash(ls:*)"] } });
    fs::write(settings.join("settings.json"), granted.to_string()).map_err(|source| Error::File {
        action: "write",
        path: settings.join("settings.json"),
        source,
    })
}

/// Whether a screen speaks of trust at all: the word, whatever reads it.
fn speaks_of_trust(screen: &[String]) -> bool {
    static TRUST: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    cf_e2e::pattern::once(&TRUST, r"(?i)\btrust").is_match(&screen.join("\n"))
}

/// The key the engine presses next into a window of `harness` showing `screen`: none
/// where it reads no question there.
fn engine_key(harness: Harness, screen: &[String]) -> Result<Option<Vec<u8>>> {
    let read = engine_read(harness, screen)?;
    Ok(match read["key"].as_str() {
        Some("enter") => Some(b"\r".to_vec()),
        Some("down") => Some(b"\x1b[B".to_vec()),
        Some("up") => Some(b"\x1b[A".to_vec()),
        _ => None,
    })
}

/// Whether the engine reads the question in `screen`, a window of `harness`'s, whether
/// or not it can place its choice.
fn engine_asks(harness: Harness, screen: &[String]) -> Result<bool> {
    Ok(engine_read(harness, screen)?["asks"] == true)
}

/// What the engine reads in `screen`, a window of `harness`'s: whether it shows the
/// question, the key it presses next, and whether the screen is the harness's prompt.
fn engine_read(harness: Harness, screen: &[String]) -> Result<serde_json::Value> {
    ask::question(&json!({ "op": "trust", "kind": harness.kind(), "screen": screen }))
}

/// Whether the engine reads `screen` as the prompt of a window of `harness`: for
/// Claude Code, by the reader its watch ends on; the others are read at their prompt
/// where nothing on their screen speaks of trust.
fn engine_reads_prompt(harness: Harness, screen: &[String]) -> Result<bool> {
    Ok(if harness == Harness::Claude {
        engine_read(harness, screen)?["prompt"] == true
    } else {
        !speaks_of_trust(screen)
    })
}

/// The folder the engine says a yes trusts, for a window opened in the project's
/// folder `directory`, whether it is the project's, and whether no folder above the
/// project's has servers of its own: whether the engine uses a server of the project's.
fn predicted(directory: &Path) -> Result<(String, bool, bool)> {
    let read = ask::question(&json!({
        "op": "trust", "kind": Harness::Claude.kind(), "screen": [], "directory": directory,
    }))?;
    let trusts = &read["trusts"];
    Ok((
        trusts["folder"].as_str().unwrap_or_default().to_owned(),
        trusts["inside"] == true,
        trusts["servers_above"].is_null(),
    ))
}

/// The servers the local settings of `directory` say Claude Code was told to use.
fn servers_used(directory: &Path) -> serde_json::Value {
    fs::read_to_string(directory.join(".claude").join("settings.local.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .map(|settings| settings["enabledMcpjsonServers"].clone())
        .unwrap_or_default()
}

/// The window's screen once it shows a question or its prompt, as the engine reads them,
/// or after `DRAWS`: Claude Code draws the dialog of a new server a moment after the
/// question about the folder is answered.
fn settled(rig: &Rig, window: &Window, harness: Harness) -> Result<Vec<String>> {
    let until = Instant::now() + DRAWS;
    loop {
        let screen = screen_of(rig, window);
        if Instant::now() > until
            || window.closed()
            || engine_asks(harness, &screen)?
            || engine_reads_prompt(harness, &screen)?
        {
            return Ok(screen);
        }
        sleep(Duration::from_millis(500));
    }
}

/// Where Claude Code keeps its own record of folders: `.claude.json` in the config folder
/// the rig gave its windows, and in the user's home where it gave none.
fn claude_record(rig: &Rig, install: &Installed) -> PathBuf {
    match rig
        .var("CLAUDE_CONFIG_DIR")
        .filter(|config| !config.is_empty())
    {
        Some(config) => Path::new(config).join(".claude.json"),
        None => install.home().join(".claude.json"),
    }
}

/// Claude Code's config folder: the one the rig gave its windows, and `~/.claude` where
/// it gave none.
fn claude_config(rig: &Rig, install: &Installed) -> PathBuf {
    match rig
        .var("CLAUDE_CONFIG_DIR")
        .filter(|config| !config.is_empty())
    {
        Some(config) => PathBuf::from(config),
        None => install.home().join(".claude"),
    }
}

/// The folders Claude Code keeps a yes for in its own record of them (`record`, its
/// `.claude.json`), read and never written: below `under` alone, each as Claude Code
/// writes it ([`live::claude_project_key`]: on Windows with `/` for `\`).
fn claude_trusted(record: &Path, under: &str) -> Vec<String> {
    let under = live::claude_project_key(under);
    let record: serde_json::Value = fs::read_to_string(record)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    record["projects"]
        .as_object()
        .map(|projects| {
            projects
                .iter()
                .filter(|(folder, entry)| {
                    folder.starts_with(&under) && entry["hasTrustDialogAccepted"] == true
                })
                .map(|(folder, _)| folder.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// What Claude Code's own status files (`sessions/<pid>.json` in its config folder `config`,
/// [`claude_config`]) say of
/// the window whose process is `pid` or that runs in `directory`: the engine reads a window
/// at rest only where it says idle, so one that says idle while it asks whether to trust
/// its folder is held only by the watch for the question (Calliope's review of T-432,
/// T-520). Read and never written.
fn claude_status(config: &Path, pid: Option<u32>, directory: &Path) -> Vec<String> {
    let real = fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    let Ok(entries) = fs::read_dir(config.join("sessions")) else {
        return vec!["no status files".to_owned()];
    };
    entries
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(|status| {
            let ours = pid.is_some_and(|pid| status["pid"] == pid);
            let here = status["cwd"].as_str().is_some_and(|cwd| {
                fs::canonicalize(cwd).unwrap_or_else(|_| PathBuf::from(cwd)) == real
            });
            ours || here
        })
        .map(|status| live::text(&status["status"]))
        .collect()
}

/// `git` with `words` in `folder`, which must do what it is asked.
fn git(folder: &Path, words: &[&str]) -> Result {
    let ran = Run::new(Path::new("git"))
        .arg("-C")
        .arg(folder)
        .args([
            "-c",
            "user.name=probe",
            "-c",
            "user.email=probe@example.invalid",
        ])
        .args(words)
        .inheriting_env()
        .run()?;
    if ran.code == Some(0) {
        Ok(())
    } else {
        Err(Error::Daemon(format!(
            "git {words:?} in {}: {ran}",
            folder.display()
        )))
    }
}

/// The last lines of the window's screen, as the engine asks the pane host for them.
fn screen_of(rig: &Rig, window: &Window) -> Vec<String> {
    rig.host(
        "pane.snapshot",
        json!({ "id": window.id(), "generation": 1, "tail": TAIL }),
    )
    .ok()
    .and_then(|answer| answer["tail"].as_array().cloned())
    .unwrap_or_default()
    .iter()
    .map(live::text)
    .collect()
}

/// What one window came to.
struct Seen {
    asked: bool,
    keys: usize,
    /// Whether all held: it asked where it must and nowhere else, the engine read
    /// what it asked, and the window is at its prompt after the answer.
    held: bool,
    lines: Vec<String>,
}

/// Opens `harness`'s window in `case`'s folder below `base` and answers what it asks
/// as the engine does.
fn look(rig: &Rig, install: &Installed, base: &str, case: Case, log: &Log) -> Result<Seen> {
    let harness = install.harness();
    let home = PathBuf::from(rig.var("HOME").unwrap_or_default());
    let (folder, repository) = case.folder();
    let name = format!("{base}/{folder}");
    if case == Case::Worktree {
        let main = live_folder_in(&home, &format!("{base}/worktree-main"))?;
        git(&main, &["init", "-q"])?;
        git(&main, &["commit", "-q", "--allow-empty", "-m", "first"])?;
        let tree = home.join(".consensflow-candidate/live").join(&name);
        git(&main, &["worktree", "add", "-q", &tree.to_string_lossy()])?;
    }
    if case == Case::Submodule {
        let source = live_folder_in(&home, &format!("{base}/submodule-source"))?;
        git(&source, &["init", "-q"])?;
        git(&source, &["commit", "-q", "--allow-empty", "-m", "first"])?;
        let superproject = live_folder_in(&home, &format!("{base}/superproject"))?;
        git(&superproject, &["init", "-q"])?;
        git(
            &superproject,
            &["commit", "-q", "--allow-empty", "-m", "first"],
        )?;
        git(
            &superproject,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                &source.to_string_lossy(),
                "sub",
            ],
        )?;
    }
    let directory = live_folder_in(&home, &name)?;
    let id = format!(
        "{}-{}",
        harness.name(),
        case.name().replace([' ', '\'', ','], "-")
    );
    if case == Case::GrantedInRepository {
        // The plain folder is answered, then the folder above it made a repository. Below
        // the live folders' root, which the run's config trusts as the owner's trusts their
        // home, a plain folder asks nothing: the yes a person said there once is what their
        // config holds of it, and the run's is given it so (T-681).
        if !answer_as_the_engine(rig, install, &name, &format!("{id}-before"), log)? {
            return Err(Error::Daemon(format!(
                "{name}: not at its prompt once answered"
            )));
        }
        said_yes_once(&claude_record(rig, install), &directory)?;
        git(
            &live_folder_in(&home, &format!("{base}/granted-repository"))?,
            &["init", "-q"],
        )?;
    }
    if must_grant(harness, case) {
        pre_approve(&directory)?;
    }
    if let (Some(servers), true) = (servers_of(case), must_serve(harness, case)) {
        let names = if case == Case::ServerAbove {
            live_folder_in(&home, &format!("{base}/server-above"))?
        } else {
            directory.clone()
        };
        fs::write(names.join(".mcp.json"), servers).map_err(|source| Error::File {
            action: "write",
            path: names.join(".mcp.json"),
            source,
        })?;
    }
    if let (Some(repository), false) = (repository, matches!(case, Case::Again | Case::ServerAgain))
    {
        let at = live_folder_in(&home, &format!("{base}/{repository}"))?;
        let made = Run::new(Path::new("git"))
            .args(["init", "-q"])
            .arg(&at)
            .inheriting_env()
            .run()?;
        if made.code != Some(0) {
            return Err(Error::Daemon(format!("git init {}: {made}", at.display())));
        }
    }
    // What Claude Code keeps a yes for below the run's folders, before the window.
    let under = live_folder_in(&home, FOLDER)?
        .to_string_lossy()
        .into_owned();
    let record = claude_record(rig, install);
    let trusted_before = claude_trusted(&record, &under);
    let opening = Open::new(rig, install, &name, &id).without_trust();
    let window = if harness == Harness::Claude && servers_of(case).is_some() {
        opening.with_the_projects_servers()
    } else {
        opening
    }
    .open(log)?;
    let drew = window.still(DRAWS);
    let first = screen_of(rig, &window);
    let mut lines = vec![format!("drew: {drew}")];
    lines.extend(first.iter().map(|line| format!("  {line}")));
    let asked = engine_asks(harness, &first)?;
    // Whether what it asked first is of its folder: one trusted from above may ask only
    // of the project's servers.
    let asked_the_folder = asked && engine_read(harness, &first)?["servers"] != true;
    // The question of the folder's grants, which the engine answers only where the
    // folder its yes records is the project's.
    let grants = asked && engine_read(harness, &first)?["grants"] == true;
    let (folder, inside, servers_alone) = predicted(&directory)?;
    if asked && harness == Harness::Claude {
        let pid = window.process().map(|process| process.pid);
        lines.push(format!(
            "Claude Code's status while it asks: {:?}",
            claude_status(&claude_config(rig, install), pid, &directory)
        ));
    }
    let mut keys = 0;
    // Whether the dialog of a new server was read, and the keys pressed into it.
    let mut served = false;
    let mut server_keys = 0;
    let mut screen = first.clone();
    while let Some(key) = engine_key(harness, &screen)? {
        let serving = engine_read(harness, &screen)?["servers"] == true;
        served |= serving;
        if keys == MOST_KEYS || grants && !inside || serving && !(inside && servers_alone) {
            break;
        }
        window.press(&key)?;
        keys += 1;
        server_keys += usize::from(serving);
        sleep(Duration::from_millis(300));
        window.still(DRAWS);
        screen = settled(rig, &window, harness)?;
    }
    let answered = !engine_asks(harness, &screen)?;
    let unread = speaks_of_trust(&first) && !asked;
    // A window that asked drew its question: on Windows the output of a window that asks never
    // held still for the minute `drew` waited (Claude Code 2.1.310 on zeewin and zeewin2,
    // 2026-10-28; the same window's prompt does), so its question is the drawing.
    let at_prompt =
        (drew || asked) && !window.closed() && answered && engine_reads_prompt(harness, &screen)?;
    if grants {
        lines.push(format!(
            "the question of the folder's grants, a yes trusting {folder} (the project's: {inside}); {keys} keys; still asks after: {}",
            !answered
        ));
    }
    let used = servers_used(&directory);
    if must_serve(harness, case) {
        lines.push(format!(
            "the dialog of a new server read: {served}; the project's alone: {}; {server_keys} keys into it; still asks after: {}; the folder's local settings use {used}",
            inside && servers_alone,
            !answered
        ));
    }
    if asked {
        lines.push(format!("answered with {keys} keys; after them:"));
        lines.extend(screen.iter().map(|line| format!("  {line}")));
    }
    if unread {
        lines.push(
            "FAIL its screen speaks of trust, and the engine reads no question there".to_owned(),
        );
    }
    window.kill();
    sleep(Duration::from_secs(1));
    // Claude Code keeps the yes for the folder the engine says a yes trusts, and
    // nothing else: what tells the engine whether a yes is the project's. Where it asked
    // of the folder's grants and the engine did not answer, it keeps nothing.
    let mut kept_where_said = true;
    if case == Case::GrantedInRepository {
        let kept: Vec<String> = claude_trusted(&record, &under)
            .into_iter()
            .filter(|folder| !trusted_before.contains(folder))
            .collect();
        kept_where_said = kept.is_empty();
        lines.push(format!("Claude Code kept a yes for {kept:?}"));
    } else if asked && harness == Harness::Claude {
        let kept: Vec<String> = claude_trusted(&record, &under)
            .into_iter()
            .filter(|folder| !trusted_before.contains(folder))
            .collect();
        // A window whose folder is trusted from above asks only of the servers: no yes of
        // its folder was said, and none is kept.
        kept_where_said = if asked_the_folder {
            kept == [live::claude_project_key(&folder)]
        } else {
            kept.is_empty()
        };
        lines.push(format!(
            "the engine says a yes trusts {folder} (the project's: {inside}); Claude Code kept it for {kept:?}"
        ));
    }
    // A question of the folder's grants is answered in the project's folder, the
    // window then at its prompt; elsewhere it is pressed nothing, and still asked once
    // the engine is done with it.
    let held = match (must_grant(harness, case), inside) {
        (true, true) => grants && keys > 0 && at_prompt,
        (true, false) => grants && keys == 0 && !answered,
        // The dialog of a new server, once the folder is answered: the server used where
        // it is the project's alone, the window then at its prompt; elsewhere nothing
        // pressed into it, and still asked.
        (false, _) if must_serve(harness, case) => {
            asked
                && served
                && if inside && servers_alone {
                    server_keys > 0
                        && at_prompt
                        && used
                            == if case == Case::Servers {
                                json!(["probe", "probe2"])
                            } else {
                                json!(["probe"])
                            }
                } else {
                    server_keys == 0 && !answered && used.is_null()
                }
        }
        (false, _) => !grants && !served && asked == must_ask(harness, case) && at_prompt,
    };
    Ok(Seen {
        asked,
        keys,
        held: held && !unread && kept_where_said,
        lines,
    })
}

/// The windows part for `install`: each case, in folders of the run's own.
fn windows(install: &Installed, stamp: &str, log: &Log) -> Result<Vec<Check>> {
    let harness = install.harness();
    let rig = Rig::start(live::rig(std::slice::from_ref(install), FAKE_AGENT))?;
    let base = format!("{FOLDER}/{stamp}/{}", harness.name());
    let mut checks = Vec::new();
    for case in Case::ALL {
        let seen = look(&rig, install, &base, case, log)?;
        log.block(
            &format!(
                "== {} {}: asked {}",
                harness.name(),
                case.name(),
                seen.asked
            ),
            &seen.lines.join("\n"),
        );
        checks.push(Check::new(
            harness.name(),
            case.name(),
            seen.held,
            if must_serve(harness, case) {
                format!("asked; then of a new server; {} keys in all", seen.keys)
            } else if must_grant(harness, case) && seen.keys == 0 {
                "asked of its grants; held, no key pressed".to_owned()
            } else if must_grant(harness, case) {
                format!("asked of its grants; answered with {} keys", seen.keys)
            } else if seen.asked {
                format!("asked; answered with {} keys", seen.keys)
            } else {
                "did not ask".to_owned()
            },
        ));
    }
    if harness == Harness::Claude {
        checks.extend(ends(&rig, install, &base, log)?);
    }
    // The rig is closed, not dropped: what the run did in the owner's setup fails it.
    rig.close_within(CLOSES)?;
    Ok(checks)
}

/// How a window of [`ends`] is ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    /// Enter on "No, exit".
    NoByEnter,
    /// Escape.
    NoByEscape,
    /// The engine's keys to a yes, then the window closed at its prompt.
    YesThenClosed,
}

/// The ways a window of [`ends`] ends, and how a check says it.
const ENDINGS: [(Ending, &str); 3] = [
    (Ending::NoByEnter, "answered No by Enter"),
    (Ending::NoByEscape, "answered No by Escape"),
    (
        Ending::YesThenClosed,
        "answered yes, then closed at its prompt",
    ),
];

/// Claude Code's windows that end in a new repository nobody trusted, and what the pane
/// host kept of their screens: the `tail` of the `pane.exit` it told, which the engine
/// reads when a window ended before its next look (`yes_at_exit`). Read through the
/// engine's own reader, the screen of a window answered No (by Enter, and by Escape) is no
/// prompt, so no yes is read from it; that of a window answered yes and closed at its
/// prompt is the prompt, with the question's choices gone from it, which is a yes.
fn ends(rig: &Rig, install: &Installed, base: &str, log: &Log) -> Result<Vec<Check>> {
    let harness = install.harness();
    let home = PathBuf::from(rig.var("HOME").unwrap_or_default());
    let mut checks = Vec::new();
    for (index, (ending, what)) in ENDINGS.into_iter().enumerate() {
        let name = format!("{base}/ends-{index}");
        let directory = live_folder_in(&home, &name)?;
        git(&directory, &["init", "-q"])?;
        let id = format!("{}-ends-{index}", harness.name());
        let window = Open::new(rig, install, &name, &id)
            .without_trust()
            .open(log)?;
        window.still(DRAWS);
        let mut screen = screen_of(rig, &window);
        let asked = engine_asks(harness, &screen)?;
        if asked {
            match ending {
                Ending::NoByEnter => {
                    window.press(b"\r")?;
                }
                Ending::NoByEscape => {
                    window.press(b"\x1b")?;
                }
                Ending::YesThenClosed => {
                    for _ in 0..MOST_KEYS {
                        let Some(key) = engine_key(harness, &screen)? else {
                            break;
                        };
                        window.press(&key)?;
                        sleep(Duration::from_millis(300));
                        window.still(DRAWS);
                        screen = screen_of(rig, &window);
                    }
                    if engine_reads_prompt(harness, &screen)? {
                        window.kill();
                    }
                }
            }
        }
        let began = Instant::now();
        while !window.closed() && began.elapsed() < CLOSES {
            sleep(Duration::from_millis(200));
        }
        let exit = rig
            .exits()
            .into_iter()
            .find(|exit| exit["id"] == window.id());
        let kept: Vec<String> = exit
            .as_ref()
            .and_then(|exit| exit["tail"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(live::text)
            .collect();
        let read = engine_read(harness, &kept)?;
        let (prompt, names) = (read["prompt"] == true, read["names"] == true);
        let held = asked
            && exit.is_some()
            && if ending == Ending::YesThenClosed {
                prompt && !names
            } else {
                !prompt
            };
        let mut lines = vec![format!(
            "asked: {asked}; ended: {}; the engine reads the kept screen as a prompt: {prompt}; as naming the question's choices: {names}",
            exit.is_some()
        )];
        lines.extend(kept.iter().map(|line| format!("  {line}")));
        log.block(
            &format!("== {} ends {what}", harness.name()),
            &lines.join("\n"),
        );
        window.kill();
        checks.push(Check::new(
            harness.name(),
            &format!("ends {what}"),
            held,
            format!(
                "asked: {asked}; ended: {}; kept screen read as a prompt: {prompt}, naming the choices: {names}",
                exit.is_some()
            ),
        ));
    }
    Ok(checks)
}

/// The folder Claude Code keeps the transcripts of a window opened in `workspace` in,
/// below its config folder `config`: the folder's path with every character that is
/// no letter or digit a dash.
fn claude_project_folder(config: &Path, workspace: &Path) -> PathBuf {
    let named: String = workspace
        .to_string_lossy()
        .chars()
        .map(|at| if at.is_ascii_alphanumeric() { at } else { '-' })
        .collect();
    config.join("projects").join(named)
}

/// A folder of records the run wrote in the rig's Claude config folder, taken away when
/// this goes, however the run goes on from there: a chief's conversation is 540 MB.
struct Written(PathBuf);

impl Drop for Written {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The run's report file, beside its folders: what the run measured and its checks,
/// which stay when the error output scrolls away.
fn report_file(stamp: &str) -> Result<PathBuf> {
    Ok(live_folder_in(
        &cf_e2e::live::machine::home()?,
        &format!("{FOLDER}/{stamp}"),
    )?
    .join("report.txt"))
}

/// Adds `text` to the report file `report`.
fn to_report(report: &Path, text: &str) -> Result {
    use std::io::Write;
    let written = |source| Error::File {
        action: "write",
        path: report.to_path_buf(),
        source,
    };
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(report)
        .and_then(|mut file| file.write_all(format!("{text}\n").as_bytes()))
        .map_err(written)
}

/// Writes a conversation of `before` turns, a compaction, then `after` turns, in
/// Claude Code's own record (2.1.29x's shape: `crates/cf-harness/tests/fixtures/claude`)
/// for the session `session` in `workspace`, at `file`: each turn a message, a command
/// that printed `printed` kilobytes, and an answer. Its size in bytes.
fn write_conversation(
    file: &Path,
    session: &str,
    workspace: &Path,
    (before, after, printed): (usize, usize, usize),
) -> Result<u64> {
    use std::io::Write;
    let cwd = workspace.to_string_lossy();
    let output = "made 1 change in src/lib.rs\n".repeat(printed * 1024 / 28);
    let written = |source| Error::File {
        action: "write",
        path: file.to_path_buf(),
        source,
    };
    let mut out = std::io::BufWriter::new(fs::File::create(file).map_err(written)?);
    let mut parent: Option<String> = None;
    let mut line = |entry: serde_json::Value, parent: &mut Option<String>| -> Result {
        let mut entry = entry;
        let uuid = uuid::Uuid::new_v4().to_string();
        let fields = json!({
            "parentUuid": parent, "isSidechain": false, "uuid": uuid,
            "timestamp": "2026-10-20T08:00:00.000Z", "userType": "external",
            "entrypoint": "cli", "cwd": cwd, "sessionId": session,
            "version": "2.1.296", "gitBranch": "main",
        });
        if let (Some(entry), Some(fields)) = (entry.as_object_mut(), fields.as_object()) {
            for (name, value) in fields {
                entry.entry(name.clone()).or_insert_with(|| value.clone());
            }
        }
        *parent = Some(uuid);
        writeln!(out, "{entry}").map_err(written)?;
        Ok(())
    };
    let assistant = |content: serde_json::Value| {
        json!({ "type": "assistant", "message": {
            "id": uuid::Uuid::new_v4().to_string(), "model": "claude-sonnet-5-5", "role": "assistant",
            "type": "message", "stop_reason": "end_turn", "stop_sequence": null,
            "usage": { "input_tokens": 1200, "output_tokens": 300 }, "content": content,
        }})
    };
    for turn in 0..before + after {
        if turn == before {
            let boundary = json!({
                "parentUuid": null, "logicalParentUuid": parent, "type": "system",
                "subtype": "compact_boundary", "content": "Conversation compacted", "level": "info",
                "compactMetadata": { "trigger": "auto", "preTokens": 960_000 },
            });
            let mut none = None;
            line(boundary, &mut none)?;
            parent = none;
            line(
                json!({ "type": "user", "isCompactSummary": true, "message": {
                    "role": "user", "content": "This session continues one that ran out of context. The work so far: the folder-trust rounds.",
                }}),
                &mut parent,
            )?;
        }
        line(
            json!({ "type": "user", "message": {
                "role": "user", "content": format!("[ConsensFlow m-{turn} · T-{turn} · result from @worker]\nRound {turn} is done: the gate passed, the branch is pushed."),
            }}),
            &mut parent,
        )?;
        let call = format!("toolu_{turn:08}");
        line(
            assistant(json!([{ "type": "tool_use", "id": call, "name": "Bash",
            "input": { "command": "cargo xtask check", "description": "Run the gate" } }])),
            &mut parent,
        )?;
        line(
            json!({ "type": "user", "message": { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": call, "content": output, "is_error": false },
            ]}}),
            &mut parent,
        )?;
        line(
            assistant(
                json!([{ "type": "text", "text": format!("Round {turn} landed; the next task goes to the board.") }]),
            ),
            &mut parent,
        )?;
    }
    out.flush().map_err(written)?;
    drop(out);
    Ok(fs::metadata(file).map_err(written)?.len())
}

/// The resumed part: a Claude window opened again on a long conversation, as the app
/// opens a chief after a restart, in a new repository nobody trusted. Its question is
/// answered as the engine answers it, and how long its screen shows neither the question
/// nor its prompt as the engine reads them is measured: the engine lets [`GONE`] pass so
/// before it counts a window that asked as past the question, and a yes is the human's
/// only once at the prompt.
fn resumed(install: &Installed, stamp: &str, log: &Log, report: &Path) -> Result<Vec<Check>> {
    let rig = Rig::start(live::rig(std::slice::from_ref(install), FAKE_AGENT))?;
    let home = PathBuf::from(rig.var("HOME").unwrap_or_default());
    let mut checks = Vec::new();
    for (index, (what, before, after, printed)) in RESUMED.into_iter().enumerate() {
        let name = format!("{FOLDER}/{stamp}/claude/{RESUMED_FOLDER}-{index}");
        let workspace = live_folder_in(&home, &name)?;
        git(&workspace, &["init", "-q"])?;
        let session = uuid::Uuid::new_v4().to_string();
        let real = fs::canonicalize(&workspace).unwrap_or_else(|_| workspace.clone());
        let records = claude_project_folder(&claude_config(&rig, install), &real);
        cf_e2e::files::make_dir(&records)?;
        // Taken away at the end of this window's turn of the loop, or as the run unwinds.
        let _written = Written(records.clone());
        let size = write_conversation(
            &records.join(format!("{session}.jsonl")),
            &session,
            &real,
            (before, after, printed),
        )?;
        let id = format!("claude-resumed-{index}");
        let window = Open::new(&rig, install, &name, &id)
            .resume(&session)
            .without_trust()
            .open(log)?;
        // A look every 200 ms (the engine's are 500 ms apart) until the prompt, for up to
        // ten minutes: a key pressed as the engine presses one, into a screen that moved
        // since the last key or stayed as it was for 5 s, and the screen's states timed.
        let opened = Instant::now();
        let mut timeline = Vec::new();
        let mut state = String::new();
        let mut keys = 0;
        let mut pressed_on: Option<(Vec<String>, Instant)> = None;
        let (mut asked, mut last_enter, mut at_prompt) = (false, None, None);
        // How long the screen showed neither the question nor the prompt once it asked:
        // what the engine waits [`GONE`] for.
        let (mut neither_since, mut longest_neither) = (None::<Instant>, Duration::ZERO);
        while opened.elapsed() < Duration::from_secs(600) && !window.closed() {
            let screen = screen_of(&rig, &window);
            let read = engine_read(Harness::Claude, &screen)?;
            let key = engine_key(Harness::Claude, &screen)?;
            let now = match (read["prompt"] == true, read["asks"] == true, key.as_deref()) {
                (true, _, _) => "its prompt",
                (_, true, Some(b"\r")) => "the question, its cursor on yes",
                (_, true, Some(_)) => "the question, its cursor on no",
                (_, true, None) => "the question, its cursor not placed",
                _ if screen.iter().all(|line| line.trim().is_empty()) => "nothing drawn",
                _ => "neither the question nor its prompt",
            };
            if now != state {
                timeline.push(format!("{:>7} ms {now}", opened.elapsed().as_millis()));
                now.clone_into(&mut state);
            }
            if read["prompt"] == true {
                at_prompt = Some(Instant::now());
                break;
            }
            if read["asks"] == true {
                asked = true;
                if let Some(since) = neither_since.take() {
                    longest_neither = longest_neither.max(since.elapsed());
                }
                let press = match &pressed_on {
                    Some((on, at)) if *on == screen => at.elapsed() >= Duration::from_secs(5),
                    _ => true,
                };
                if let (Some(key), true, true) = (key, press, keys < MOST_KEYS) {
                    window.press(&key)?;
                    keys += 1;
                    let enter = key == b"\r";
                    timeline.push(format!(
                        "{:>7} ms pressed {}",
                        opened.elapsed().as_millis(),
                        if enter { "Enter" } else { "Down" }
                    ));
                    if enter {
                        last_enter = Some(Instant::now());
                    }
                    pressed_on = Some((screen, Instant::now()));
                }
            } else if asked {
                neither_since.get_or_insert_with(Instant::now);
            }
            sleep(Duration::from_millis(200));
        }
        if let (Some(since), Some(prompt)) = (neither_since, at_prompt) {
            longest_neither = longest_neither.max(prompt.duration_since(since));
        }
        let to_prompt = last_enter
            .zip(at_prompt)
            .map(|(enter, prompt)| prompt.duration_since(enter));
        let mut lines = vec![format!(
            "{what} conversation: {} MB; asked: {asked}; keys: {keys}; from the last Enter to the prompt: {}; the longest it showed neither once asked: {} ms",
            size / 1_000_000,
            to_prompt.map_or("never".to_owned(), |took| format!("{} ms", took.as_millis())),
            longest_neither.as_millis()
        )];
        lines.extend(timeline);
        lines.extend(
            screen_of(&rig, &window)
                .iter()
                .map(|line| format!("  {line}")),
        );
        window.kill();
        sleep(Duration::from_secs(1));
        let title = format!("== claude resumed on {what} conversation");
        log.block(&title, &lines.join("\n"));
        to_report(report, &format!("{title}\n{}\n", lines.join("\n")))?;
        checks.push(Check::new(
            "claude",
            &format!("resumed on {what} conversation"),
            asked && at_prompt.is_some() && longest_neither < GONE,
            format!(
                "{} MB; {keys} keys; at its prompt {} after the last Enter; neither question nor prompt for at most {} ms, of the engine's {} ms",
                size / 1_000_000,
                to_prompt.map_or("never".to_owned(), |took| format!("{} ms", took.as_millis())),
                longest_neither.as_millis(),
                GONE.as_millis()
            ),
        ));
    }
    // The rig is closed, not dropped: what the run did in the owner's setup fails it.
    rig.close_within(CLOSES)?;
    Ok(checks)
}

/// The notes to the human that speak of trusting the project's folder, which they
/// name as the system names it.
fn trust_notes(members: &Members) -> Result<Vec<String>> {
    let folder = fs::canonicalize(members.folder())
        .unwrap_or_else(|_| members.folder().to_path_buf())
        .to_string_lossy()
        .into_owned();
    Ok(members
        .project()
        .inbox("human")?
        .iter()
        .filter_map(|message| message["body"].as_str())
        .filter(|body| body.contains("trust") && body.contains(&folder))
        .map(str::to_owned)
        .collect())
}

/// How long a worker's task has to be done.
const DONE: Duration = Duration::from_secs(300);

/// The roles a project of the projects part puts the harness in: its chief, a
/// worker, and a worker whose follow-up opens its window again on its conversation
/// after the folder was made a repository (dip-agentos's T-13, T-14 and T-15).
const ROLES: [&str; 3] = ["chief", "worker", "follow-up"];

/// The words of `cf task add` that give a task to a member of `tier`.
fn plan_words(tier: &str) -> Vec<String> {
    cf_e2e::live::members::tier_flag(tier)
}

/// A worker's task, T-`number`, given by the stand-in chief with the words of `cf task
/// add` before `brief`, done within [`DONE`]: whether it was.
fn done_task(members: &Members, number: i64, words: &[String], brief: &str) -> Result<bool> {
    let mut asked: Vec<&str> = vec!["task", "add"];
    asked.extend(words.iter().map(String::as_str));
    asked.push(brief);
    let added = members.as_chief(&asked)?;
    if added.code != Some(0) {
        return Err(Error::Daemon(format!("cf task add: {added}")));
    }
    Ok(live::until(DONE, Duration::from_secs(1), || {
        members
            .task(number)
            .ok()
            .filter(|task| task["state"] == "done")
    })
    .is_some())
}

/// The projects part for `harness` as `role`: a project in a new repository nobody
/// trusted (a follow-up's is made one after the first task). The chief must come up idle, a worker's tasks be done, a follow-up's
/// window be opened again on its conversation, and the notes speak of a yes once
/// for Claude Code and never for a harness that does not ask.
fn project(harness: Harness, role: &str, stamp: &str, log: &Log) -> Check {
    let who = format!("{} {role}", harness.name());
    let folder = format!("{FOLDER}-project/{stamp}-{}-{role}", harness.name());
    let plan = match role {
        "chief" => Plan::new(&folder, member_as(harness, "chief")).in_a_new_repository(),
        "worker" => Plan::new(&folder, stand_in_chief())
            .with(member(harness))
            .in_a_new_repository(),
        _ => Plan::new(&folder, stand_in_chief()).with(member(harness)),
    };
    let scoped = log.scoped(&who);
    let came = Members::open(&plan, Path::new(FAKE_AGENT), &scoped).and_then(|mut members| {
        let done = if role == "chief" {
            true
        } else {
            let agent = agents::id(harness);
            let tier = members.add(&agent, &[])?;
            let mut words = plan_words(&tier);
            let first = done_task(&members, 1, &words, "Reply with exactly: TRUST_OK")?;
            if role == "worker" || !first {
                first
            } else {
                git(members.folder(), &["init", "-q"])?;
                let opened = members.windows_opened_for(&agent).len();
                words = vec!["--after".to_owned(), "T-1".to_owned()];
                let again = done_task(&members, 2, &words, "Reply with exactly: AGAIN_OK")?;
                let resumed = members.windows_opened_for(&agent)[opened..]
                    .iter()
                    .any(|frame| {
                        frame["argv"].as_array().is_some_and(|argv| {
                            argv.iter()
                                .any(|word| word == "--resume" || word == "resume")
                        })
                    });
                again && resumed
            }
        };
        let notes = trust_notes(&members)?;
        let tail = members.tail(&format!("p{}-chief", members.project_id()));
        members.close()?;
        Ok((done, notes, tail))
    });
    let expected = usize::from(harness == Harness::Claude);
    match came {
        Ok((true, notes, _))
            if notes.len() == expected
                && notes.iter().all(|note| note.contains("answered yes")) =>
        {
            Check::passed(
                &who,
                "project",
                format!(
                    "{} in a new repository; notes that ConsensFlow answered yes: {expected}",
                    if role == "chief" { "came up idle" } else { "its task done" }
                ),
            )
        }
        Ok((done, notes, tail)) => Check::failed(
            &who,
            "project",
            format!(
                "{}; notes of the folder's trust: {} (expected {expected}); the chief's screen: {tail}",
                if done {
                    "its work done"
                } else {
                    "its tasks never done, or a follow-up's window not opened again"
                },
                notes.len()
            ),
        ),
        Err(failed) => Check::failed(&who, "project", failed.to_string()),
    }
}

#[test]
#[ignore = "opt-in: opens real windows of every harness named; cargo xtask live folder-trust (npm run live:folder-trust)"]
fn which_harness_asks_to_trust_a_folder_and_consensflow_answers_it_in_the_projects_own() -> Result {
    let log = Log::begin("live folder-trust")?;
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let harnesses = named(
        &live::list("harness"),
        &Harness::ALL,
        Harness::name,
        "harness",
        &Harness::ALL,
    )?;
    let parts = named(&live::list("part"), &PARTS, |part| part, "part", &PARTS)?;
    let stamp = jiff::Timestamp::now().as_second().to_string();
    let report = report_file(&stamp)?;
    log.say(format!("the run's report: {}", report.display()));
    let mut checks = Vec::new();
    for harness in harnesses {
        let install = log.found(harness)?;
        // Codex's bare window is not the app's: its question is no window ConsensFlow opens.
        if parts.contains(&"windows") && harness != Harness::Codex {
            checks.extend(windows(&install, &stamp, &log)?);
        }
        if parts.contains(&"projects") && matches!(harness, Harness::Claude | Harness::Codex) {
            for role in ROLES {
                checks.push(project(harness, role, &stamp, &log));
            }
        }
        if parts.contains(&"resumed") && harness == Harness::Claude {
            checks.extend(resumed(&install, &stamp, &log, &report)?);
        }
    }
    to_report(&report, &report::table(&checks))?;
    report::conclude(&log, "which harness asks, and what came of it", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_must_ask_in_a_repository_nobody_answered_and_of_grants_and_servers_and_no_harness_ever_elsewhere(
    ) {
        let asking: Vec<&str> = Case::ALL
            .into_iter()
            .filter(|case| must_ask(Harness::Claude, *case))
            .map(Case::name)
            .collect();
        assert_eq!(
            asking,
            [
                "plain below with pre-approved tools",
                "answered, then in a repository, with pre-approved tools",
                "repository",
                "parent",
                "repository below",
                "untrusted's folder",
                "linked worktree",
                "submodule",
                "repository with a server",
                "repository with two servers",
                "plain below a folder with a server"
            ]
        );
        for case in Case::ALL {
            for harness in [
                Harness::Codex,
                Harness::Pi,
                Harness::OpenCode,
                Harness::Devin,
            ] {
                assert!(!must_ask(harness, case), "{harness:?} {case:?}");
            }
        }
    }

    #[test]
    fn a_screen_speaks_of_trust_by_the_word_and_the_runs_folder_does_not() {
        let lines = |text: &str| text.lines().map(str::to_owned).collect::<Vec<_>>();
        assert!(speaks_of_trust(&lines("Do you TRUST this folder?")));
        assert!(speaks_of_trust(&lines(
            "Trusting will apply to the repository root"
        )));
        assert!(!speaks_of_trust(&lines(&format!(
            "~/.consensflow-candidate/live/{FOLDER}/1/claude/fresh"
        ))));
        assert!(!speaks_of_trust(&lines("an untrusted folder")));
    }

    #[test]
    fn what_a_window_of_the_resumed_part_wrote_goes_with_it() {
        // However its turn of the loop ends.
        let config = tempfile::tempdir().unwrap();
        let written = config.path().join("projects").join("written");
        fs::create_dir_all(&written).unwrap();
        fs::write(written.join("s.jsonl"), "{}").unwrap();
        drop(Written(written.clone()));
        assert!(!written.exists());
    }

    #[test]
    fn each_case_has_a_folder_of_its_own_but_the_one_opened_again() {
        let folders: Vec<&str> = Case::ALL.map(|case| case.folder().0).to_vec();
        assert_eq!(
            folders
                .iter()
                .filter(|folder| **folder == "repository")
                .count(),
            2
        );
        assert_eq!(Case::Again.folder().1, Some("repository"));
        assert_eq!(Case::ServerAgain.folder(), Case::Server.folder());
        assert_eq!(Case::Below.folder().1, None);
    }
}
