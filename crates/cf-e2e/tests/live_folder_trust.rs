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
//!   folder below a trusted one, nor in a plain folder of a trusted repository. Its
//!   question opens on "No, exit"; Down, then Enter, says yes, which it writes down
//!   for the repository's root (its question names the window's own folder), so a
//!   window opened anywhere in it again does not ask.
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
//! made fresh for the run, below the live folders: a plain folder; a new repository
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
//! **projects**: for Claude Code and Codex, a project through the daemon in a new
//! repository nobody trusted: its chief a real window of the harness; a worker of it
//! given a task beside a stand-in chief; and a worker whose first task is done in a
//! plain folder, which is then made a repository, and whose follow-up opens its window
//! again on its conversation. The chief must come up idle, the tasks be done and the
//! follow-up's window be a resumed one, and the human's notes must say that ConsensFlow
//! answered yes once for Claude Code, and never for Codex.
//!
//! What Claude Code writes of a yes is taken back with `claude purge` at the end;
//! the run says which folders it could not have it forget. Codex writes nothing here.
//!
//! ```text
//! cargo xtask live folder-trust                       every harness, both parts
//! cargo xtask live folder-trust claude codex          the ones named
//! cargo xtask live folder-trust --part windows        one part
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::cleanup::forget_in_claude;
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

/// How many keys a question is given before it counts as not answered: the engine's own.
const MOST_KEYS: usize = 6;

/// The lines of a window's screen the engine reads: the pane host's tail.
const TAIL: usize = 12;

/// The parts of the run.
const PARTS: [&str; 2] = ["windows", "projects"];

/// A folder a window is opened in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    /// A plain folder.
    Fresh,
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
}

impl Case {
    const ALL: [Case; 9] = [
        Case::Fresh,
        Case::Repository,
        Case::Again,
        Case::Parent,
        Case::Below,
        Case::RepositoryBelow,
        Case::Untrusted,
        Case::Worktree,
        Case::Submodule,
    ];

    const fn name(self) -> &'static str {
        match self {
            Case::Fresh => "fresh",
            Case::Repository => "repository",
            Case::Again => "repository again",
            Case::Parent => "parent",
            Case::Below => "plain below",
            Case::RepositoryBelow => "repository below",
            Case::Untrusted => "untrusted's folder",
            Case::Worktree => "linked worktree",
            Case::Submodule => "submodule",
        }
    }

    /// Its folder below the harness's folder of the run, and the repository made
    /// for it, where it has one of its own.
    const fn folder(self) -> (&'static str, Option<&'static str>) {
        match self {
            Case::Fresh => ("fresh", None),
            Case::Repository | Case::Again => ("repository", Some("repository")),
            Case::Parent => ("parent", Some("parent")),
            Case::Below => ("parent/below", None),
            Case::RepositoryBelow => ("parent/repository", Some("parent/repository")),
            Case::Untrusted => ("untrusted/folder", Some("untrusted")),
            // Its repository is the checkout it belongs to, made with it (`worktree`).
            Case::Worktree => ("worktree", None),
            // Its repository is made with it inside another (`submodule`).
            Case::Submodule => ("superproject/sub", None),
        }
    }
}

/// Whether `harness` must ask in `case`: Claude Code in a repository nobody
/// answered; nobody else, ever.
fn must_ask(harness: Harness, case: Case) -> bool {
    harness == Harness::Claude
        && matches!(
            case,
            Case::Repository
                | Case::Parent
                | Case::RepositoryBelow
                | Case::Untrusted
                | Case::Worktree
                | Case::Submodule
        )
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
        _ => None,
    })
}

/// What the engine reads in `screen`, a window of `harness`'s: the key it presses
/// next, and whether the screen is the harness's prompt.
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
/// folder `directory`, and whether it is the project's.
fn predicted(directory: &Path) -> Result<(String, bool)> {
    let read = ask::question(&json!({
        "op": "trust", "kind": Harness::Claude.kind(), "screen": [], "directory": directory,
    }))?;
    let trusts = &read["trusts"];
    Ok((
        trusts["folder"].as_str().unwrap_or_default().to_owned(),
        trusts["inside"] == true,
    ))
}

/// The folders Claude Code keeps a yes for in its own record of them (`.claude.json`
/// in `home`), read and never written: below `under` alone.
fn claude_trusted(home: &Path, under: &str) -> Vec<String> {
    let record: serde_json::Value = fs::read_to_string(home.join(".claude.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    record["projects"]
        .as_object()
        .map(|projects| {
            projects
                .iter()
                .filter(|(folder, entry)| {
                    folder.starts_with(under) && entry["hasTrustDialogAccepted"] == true
                })
                .map(|(folder, _)| folder.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// What Claude Code's own status files in `home` (`sessions/<pid>.json`) say of the
/// window whose process is `pid` or that runs in `directory`: the engine reads a window
/// at rest only where it says idle, so one that says idle while it asks whether to trust
/// its folder is held only by the watch for the question (Calliope's review of T-432,
/// T-520). Read and never written.
fn claude_status(home: &Path, pid: Option<u32>, directory: &Path) -> Vec<String> {
    let real = fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    let Ok(entries) = fs::read_dir(home.join("sessions")) else {
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
    if let (Some(repository), false) = (repository, case == Case::Again) {
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
    let id = format!(
        "{}-{}",
        harness.name(),
        case.name().replace([' ', '\''], "-")
    );
    // What Claude Code keeps a yes for below the run's folders, before the window.
    let under = live_folder_in(&home, FOLDER)?
        .to_string_lossy()
        .into_owned();
    let trusted_before = claude_trusted(install.home(), &under);
    let window = Open::new(rig, install, &name, &id)
        .without_trust()
        .open(log)?;
    let drew = window.still(DRAWS);
    let first = screen_of(rig, &window);
    let mut lines = vec![format!("drew: {drew}")];
    lines.extend(first.iter().map(|line| format!("  {line}")));
    let asked = engine_key(harness, &first)?.is_some();
    if asked && harness == Harness::Claude {
        let pid = window.process().map(|process| process.pid);
        lines.push(format!(
            "Claude Code's status while it asks: {:?}",
            claude_status(install.home(), pid, &directory)
        ));
    }
    let mut keys = 0;
    let mut screen = first.clone();
    while let Some(key) = engine_key(harness, &screen)? {
        if keys == MOST_KEYS {
            break;
        }
        window.press(&key)?;
        keys += 1;
        sleep(Duration::from_millis(300));
        window.still(DRAWS);
        screen = screen_of(rig, &window);
    }
    let answered = engine_key(harness, &screen)?.is_none();
    let unread = speaks_of_trust(&first) && !asked;
    let at_prompt = drew && !window.closed() && answered && engine_reads_prompt(harness, &screen)?;
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
    // nothing else: what tells the engine whether a yes is the project's.
    let mut kept_where_said = true;
    if asked && harness == Harness::Claude {
        let (folder, inside) = predicted(&directory)?;
        let kept: Vec<String> = claude_trusted(install.home(), &under)
            .into_iter()
            .filter(|folder| !trusted_before.contains(folder))
            .collect();
        kept_where_said = kept == [folder.clone()];
        lines.push(format!(
            "the engine says a yes trusts {folder} (the project's: {inside}); Claude Code kept it for {kept:?}"
        ));
    }
    Ok(Seen {
        asked,
        keys,
        held: asked == must_ask(harness, case) && at_prompt && !unread && kept_where_said,
        lines,
    })
}

/// The windows part for `install`: each case, in folders of the run's own, which are
/// added to `folders`.
fn windows(
    install: &Installed,
    stamp: &str,
    log: &Log,
    folders: &mut Vec<String>,
) -> Result<Vec<Check>> {
    let harness = install.harness();
    let rig = Rig::start(live::rig(std::slice::from_ref(install), FAKE_AGENT))?;
    let base = format!("{FOLDER}/{stamp}/{}", harness.name());
    let home = PathBuf::from(rig.var("HOME").unwrap_or_default());
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
            if seen.asked {
                format!("asked; answered with {} keys", seen.keys)
            } else {
                "did not ask".to_owned()
            },
        ));
    }
    let made = Case::ALL.into_iter().map(|case| case.folder().0).chain([
        "untrusted",
        "worktree-main",
        "superproject",
        "submodule-source",
    ]);
    for folder in made {
        let path = live_folder_in(&home, &format!("{base}/{folder}"))?;
        folders.push(path.to_string_lossy().into_owned());
    }
    rig.close()?;
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
/// trusted (a follow-up's is made one after the first task), whose folder is added
/// to `folders`. The chief must come up idle, a worker's tasks be done, a follow-up's
/// window be opened again on its conversation, and the notes speak of a yes once
/// for Claude Code and never for a harness that does not ask.
fn project(
    harness: Harness,
    role: &str,
    stamp: &str,
    log: &Log,
    folders: &mut Vec<String>,
) -> Check {
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
        folders.push(members.folder().to_string_lossy().into_owned());
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
    let mut checks = Vec::new();
    let mut claude = None;
    for harness in harnesses {
        let install = log.found(harness)?;
        let mut folders = Vec::new();
        // Codex's bare window is not the app's: its question is no window ConsensFlow opens.
        if parts.contains(&"windows") && harness != Harness::Codex {
            checks.extend(windows(&install, &stamp, &log, &mut folders)?);
        }
        if parts.contains(&"projects") && matches!(harness, Harness::Claude | Harness::Codex) {
            for role in ROLES {
                checks.push(project(harness, role, &stamp, &log, &mut folders));
            }
        }
        if harness == Harness::Claude {
            claude = Some((install, folders));
        }
    }
    // Claude Code forgets the run's folders, and the trust it wrote down for them.
    if let Some((claude, folders)) = claude {
        let forgot = forget_in_claude(claude.command(), claude.home(), &folders);
        let kept: Vec<&String> = folders
            .iter()
            .filter(|folder| !forgot.contains(folder))
            .collect();
        log.say(format!(
            "Claude Code forgot {} of the run's {} folders; not: {kept:?}",
            forgot.len(),
            folders.len()
        ));
    }
    report::conclude(&log, "which harness asks, and what came of it", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_must_ask_in_a_repository_nobody_answered_and_no_harness_ever_elsewhere() {
        let asking: Vec<&str> = Case::ALL
            .into_iter()
            .filter(|case| must_ask(Harness::Claude, *case))
            .map(Case::name)
            .collect();
        assert_eq!(
            asking,
            [
                "repository",
                "parent",
                "repository below",
                "untrusted's folder",
                "linked worktree",
                "submodule"
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
        assert_eq!(Case::Below.folder().1, None);
    }
}
