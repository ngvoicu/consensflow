//! Claude Code's question about trusting a folder stopped every launch in a new repository.
//!
//! Seen from 2026-10-17 to 10-19 by dip-agentos's chief, on ConsensFlow 3.0.4: after `git
//! init` in the project's folder, every worker's window ConsensFlow opened stopped at
//! Claude Code's "Do you trust this folder?", and T-11, T-13, T-14 and T-15 each failed and
//! needed a manual reopen. The owner hit it again on 2026-10-20 in other folders. The
//! project isn't on this Mac, so its transcripts weren't read. The report is item 2 of
//! `issues/field-reports-2026-10-19-dip-agentos.md` in consensflow-sme, and the decision
//! `decisions/2026-10-20-consensflow-answers-trust-in-the-projects-folder.md`.
//!
//! What the owner should have seen: choosing a folder for a project counts as trusting it.
//! A window that asks of the project's folder is answered yes by ConsensFlow on its screen,
//! as a person would, and the human's notes say so once, however many windows ask; its
//! task is done. A window whose yes would trust a folder outside the project's (the root
//! of the repository the project's folder is in) is never answered by ConsensFlow: the
//! human is told once, nothing fails while the question waits, and once they said yes in
//! a window every task that waited goes on by itself.
//!
//! The stand-in asks the question as Claude Code 2.1.305 does (`fake_agent`'s `trust`):
//! in a git repository whose root it was never told to trust, before anything else, its
//! `❯` on "No, exit", and the yes written down for the repository's root. Each case gives
//! the worker two tasks at once, two windows that ask together, as dip-agentos's did; a
//! chief opened in a new repository asks too, and must take what the human tells it. A
//! launch here has 8 s to show its first message (`CONSENSFLOW_LAUNCH_TIMEOUT_MS`), not
//! the three minutes it has in the app: on main each task fails at that. A window the
//! human said yes in may write its first message only seconds later: that message has
//! its whole time from the yes.

use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::files;
use cf_e2e::process::Run;
use cf_e2e::rig::{Config, Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The keys the human answers the question with: Down to the yes, then Enter.
const DOWN: &[u8] = b"\x1b[B";
const ENTER: &[u8] = b"\r";

/// The rig, the worker's windows asking, its launches given 8 s.
fn asking() -> Config {
    config()
        .var("CF_TEST_TRUST", "worker")
        .var("CONSENSFLOW_LAUNCH_TIMEOUT_MS", "8000")
}

/// `git init` in `folder`.
fn git_init(folder: &Path) -> Outcome {
    let ran = Run::new(Path::new("git"))
        .args(["init", "-q"])
        .arg(folder)
        .inheriting_env()
        .run()?;
    assert_eq!(ran.code, Some(0), "git init {}: {ran}", folder.display());
    Ok(())
}

/// The chief gives the worker two tasks at once.
fn two_tasks(rig: &Rig, project: &Project) -> Outcome {
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    for word in ["FIRST_DONE", "SECOND_DONE"] {
        rig.tell(
            project.id(),
            &format!("DISPATCH --tier {tier} Reply with exactly: {word}"),
        )?;
    }
    Ok(())
}

/// `folder` as the system names it, as the notes name a folder: its links followed,
/// and on Windows with no `\\?\` before its drive.
fn real(folder: &Path) -> String {
    let real = std::fs::canonicalize(folder).map_or_else(
        |_| folder.to_string_lossy().into_owned(),
        |real| real.to_string_lossy().into_owned(),
    );
    real.strip_prefix(r"\\?\")
        .map_or(real.clone(), str::to_owned)
}

/// The notes to the human that speak of trusting `folder`.
fn notes_of(project: &Project, folder: &Path) -> cf_e2e::Result<Vec<String>> {
    let folder = real(folder);
    Ok(project
        .inbox("human")?
        .iter()
        .filter_map(|message| message["body"].as_str())
        .filter(|body| body.contains("trust") && body.contains(&folder))
        .map(str::to_owned)
        .collect())
}

/// The folders the stand-in was told to trust, a yes each.
fn trusted(rig: &Rig) -> String {
    let config = rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default();
    files::read_string(&Path::new(config).join("trusted-folders")).unwrap_or_default()
}

/// The worker's windows open now.
fn workers(rig: &Rig, project: &Project) -> Vec<Value> {
    let prefix = format!("p{}-worker", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| {
            frame["id"]
                .as_str()
                .is_some_and(|id| id.starts_with(&prefix))
        })
        .collect()
}

/// Both tasks done.
fn both_done(project: &Project) -> cf_e2e::Result<bool> {
    Ok(project.task(1)?["state"] == "done" && project.task(2)?["state"] == "done")
}

#[test]
fn a_worker_asked_to_trust_the_projects_new_repository_is_answered_yes_and_does_its_tasks(
) -> Outcome {
    let rig = Rig::start(asking())?;
    git_init(rig.workspace())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    two_tasks(&rig, &project)?;
    rig.wait_for("T-1 and T-2 to be done", secs(90), || both_done(&project))?;
    // ConsensFlow said yes on the screen, once for the folder: the stand-in wrote it down.
    let folder = rig.workspace();
    let same = |line: &str| std::fs::canonicalize(line).ok() == std::fs::canonicalize(folder).ok();
    assert!(
        trusted(&rig).lines().any(same),
        "the stand-in was never told yes: {}",
        trusted(&rig)
    );
    let notes = notes_of(&project, folder)?;
    assert_eq!(notes.len(), 1, "the human is told once: {notes:?}");
    assert!(notes[0].contains("answered yes"), "{notes:?}");
    rig.close()?;
    Ok(())
}

#[test]
fn a_chief_asked_to_trust_the_projects_new_repository_is_answered_yes_and_takes_what_it_is_told(
) -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_TRUST", "chief"))?;
    git_init(rig.workspace())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    // What the human tells the chief goes into its window once it is past the question.
    rig.tell(project.id(), "Reply with exactly: CHIEF_HEARD")?;
    let chief = project.lane("chief")?;
    assert_eq!(chief["activity"]["state"], "idle", "{chief}");
    let notes = notes_of(&project, rig.workspace())?;
    assert_eq!(notes.len(), 1, "the human is told once: {notes:?}");
    assert!(
        notes[0].contains("@chief") && notes[0].contains("answered yes"),
        "{notes:?}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_worker_whose_yes_would_trust_a_folder_outside_the_project_waits_for_the_human_and_goes_on_after(
) -> Outcome {
    outside_answered_by_the_human(&[])
}

#[test]
fn a_worker_that_reads_the_humans_keys_late_takes_their_yes_and_goes_on_after() -> Outcome {
    // main's Windows CI, 2026-10-21 (run 39616304894): on a slow runner the case above
    // timed out after the human's yes. A window that reads its terminal late finds the
    // human's Down and Enter in one read.
    outside_answered_by_the_human(&[("CF_TEST_TRUST_READS_LATE_MS", "1500")])
}

/// Two of the worker's windows ask of a folder outside the project; the human says
/// yes in one, with the stand-in set by `vars` besides, and both tasks go on.
fn outside_answered_by_the_human(vars: &[(&str, &str)]) -> Outcome {
    let rig = asking_with(vars)?;
    // The repository is the workspace; the project's folder is below it.
    let root = rig.workspace().to_path_buf();
    git_init(&root)?;
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    two_tasks(&rig, &project)?;
    waiting(
        &rig,
        &project,
        "the human to be told of the folder outside",
        secs(60),
        || Ok(!notes_of(&project, &root)?.is_empty()),
    )?;
    waiting(
        &rig,
        &project,
        "both of the worker's windows to be open",
        secs(30),
        || Ok(workers(&rig, &project).len() == 2),
    )?;
    // Past the time a launch has, nothing failed, and nothing said yes for the human.
    // The chief tells T-1's window something meanwhile: it waits for the window to
    // be past the question, whose Enter would choose "No, exit".
    rig.tell(project.id(), "CF tell T-1 :: KEEP_THIS_IN_MIND")?;
    sleep(Duration::from_secs(12));
    for number in [1, 2] {
        let state = project.task(number)?["state"].clone();
        assert!(
            state != "failed" && state != "done",
            "T-{number} is {state} while its window asks"
        );
    }
    assert_eq!(
        workers(&rig, &project).len(),
        2,
        "a window that asks was given what was told to it"
    );
    assert_eq!(trusted(&rig), "", "ConsensFlow answered for the human");
    // The board says why the worker's windows are still starting.
    let lane = project.lane("worker")?;
    assert_eq!(lane["activity"]["state"], "starting", "{lane}");
    assert_eq!(
        lane["activity"]["reason"], "the window asks whether to trust its folder",
        "{lane}"
    );
    let notes = notes_of(&project, &root)?;
    assert_eq!(notes.len(), 1, "the human is told once: {notes:?}");
    assert!(
        notes[0].contains("outside the project's folder"),
        "{notes:?}"
    );
    // The human says yes in one of the windows; both tasks go on by themselves.
    let window = workers(&rig, &project)
        .first()
        .cloned()
        .ok_or("no window of the worker")?;
    for keys in [DOWN, ENTER] {
        let pressed = rig.host(
            "pane.input",
            json!({ "id": window["id"], "generation": window["generation"], "bytes": keys }),
        )?;
        assert_eq!(pressed["ok"], true, "{pressed}");
        sleep(Duration::from_millis(600));
    }
    // Each step is bound from the one before it, so a stall says where it was: the
    // human's window takes their yes, the yes is carried to the other window, which
    // takes it too, and then both tasks are done.
    waiting(
        &rig,
        &project,
        "the human's window to take their yes",
        secs(30),
        || Ok(!trusted(&rig).is_empty()),
    )?;
    waiting(
        &rig,
        &project,
        "the yes to be carried to the other window",
        secs(30),
        || {
            Ok(keys_given(&rig)
                .iter()
                .filter(|key| *key == "enter")
                .count()
                == 2)
        },
    )?;
    waiting(
        &rig,
        &project,
        "T-1 and T-2 to be done once both took the yes",
        secs(60),
        || both_done(&project),
    )?;
    assert_eq!(notes_of(&project, &root)?.len(), 1, "told once");
    rig.close()?;
    Ok(())
}

#[test]
fn a_worker_the_human_said_yes_in_that_starts_slowly_has_its_first_messages_whole_time_from_then(
) -> Outcome {
    // Its prompt drawn, it writes its first message 4 s after the yes, past the time
    // its launch had counted from the launch.
    let rig = Rig::start(asking().var("CF_TEST_TRUST_STARTS_MS", "4000"))?;
    let root = rig.workspace().to_path_buf();
    git_init(&root)?;
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: SLOW_DONE"),
    )?;
    rig.wait_for("the worker's window to ask", secs(60), || {
        Ok(!notes_of(&project, &root)?.is_empty() && workers(&rig, &project).len() == 1)
    })?;
    sleep(Duration::from_secs(10));
    let window = workers(&rig, &project)
        .first()
        .cloned()
        .ok_or("no window of the worker")?;
    for keys in [DOWN, ENTER] {
        let pressed = rig.host(
            "pane.input",
            json!({ "id": window["id"], "generation": window["generation"], "bytes": keys }),
        )?;
        assert_eq!(pressed["ok"], true, "{pressed}");
        sleep(Duration::from_millis(600));
    }
    rig.wait_for("T-1 to be done", secs(60), || {
        let state = project.task(1)?["state"].clone();
        assert_ne!(state, "failed", "T-1 failed after the yes");
        Ok(state == "done")
    })?;
    rig.close()?;
    Ok(())
}

/// The chief gives the worker a task, which is done: its number.
fn a_task(rig: &Rig, project: &Project, tier: &str, number: i64, word: &str) -> Outcome {
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: {word}"),
    )?;
    rig.wait_for(&format!("T-{number} to be done"), secs(90), || {
        let state = project.task(number)?["state"].clone();
        assert_ne!(state, "failed", "T-{number} failed");
        Ok(state == "done")
    })?;
    Ok(())
}

/// Waits as `Rig::wait_for` does, and where it times out says where the case stood: the
/// keys the question was given, the folders trusted, how many of the worker's windows are
/// open, each task's state and what the board says of the worker. Which of the case's
/// bounded waits ran out, and with the windows at what, is what a stall on a slow
/// machine is read by.
fn waiting(
    rig: &Rig,
    project: &Project,
    what: &str,
    within: Duration,
    predicate: impl FnMut() -> cf_e2e::Result<bool>,
) -> Outcome {
    rig.wait_for(what, within, predicate).map_err(|failed| {
        let state = |number| {
            project
                .task(number)
                .map_or_else(|cause| cause.to_string(), |task| task["state"].to_string())
        };
        let lane = project
            .lane("worker")
            .map_or_else(|cause| cause.to_string(), |lane| lane["activity"].to_string());
        format!(
            "{failed}; keys given {:?}, trusted {:?}, {} worker windows open, T-1 {}, T-2 {}, worker {lane}",
            keys_given(rig),
            trusted(rig),
            workers(rig, project).len(),
            state(1),
            state(2),
        )
    })?;
    Ok(())
}

/// The keys the stand-in's question was given, a word each.
fn keys_given(rig: &Rig) -> Vec<String> {
    let config = rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default();
    files::read_string(&Path::new(config).join("trust-keys"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_worker_opened_again_on_its_conversation_after_git_init_is_answered_yes_and_does_its_task(
) -> Outcome {
    // dip-agentos's shape: the worker's first task was done in a plain folder; then the
    // folder was made a repository, and a follow-up opened its window again on its
    // conversation, whose record holds the first task from its first look.
    let rig = Rig::start(asking())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    a_task(&rig, &project, &tier, 1, "FIRST_DONE")?;
    git_init(rig.workspace())?;
    let before = rig.open_frames().len();
    // A follow-up for the window that did T-1, which keeps its conversation.
    rig.tell(
        project.id(),
        "CF task add --after T-1 :: Reply with exactly: SECOND_DONE",
    )?;
    rig.wait_for("T-2 to be done", secs(90), || {
        let state = project.task(2)?["state"].clone();
        assert_ne!(state, "failed", "T-2 failed");
        Ok(state == "done")
    })?;
    let resumed = rig.open_frames()[before..].iter().any(|frame| {
        frame["id"]
            .as_str()
            .is_some_and(|id| id.contains("-worker"))
            && frame["argv"]
                .as_array()
                .is_some_and(|argv| argv.iter().any(|word| word == "--resume"))
    });
    assert!(
        resumed,
        "the second task's window was no resume: {:?}",
        rig.open_frames()
    );
    let notes = notes_of(&project, rig.workspace())?;
    assert_eq!(notes.len(), 1, "the human is told once: {notes:?}");
    assert!(notes[0].contains("answered yes"), "{notes:?}");
    rig.close()?;
    Ok(())
}

#[test]
fn a_worker_whose_first_frame_holds_still_before_the_question_is_still_answered() -> Outcome {
    // A harness under load may hold its first frame still for longer than a screen
    // takes to read as drawn before its question comes.
    let rig = Rig::start(asking().var("CF_TEST_TRUST_FIRST_FRAME_MS", "3000"))?;
    git_init(rig.workspace())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    a_task(&rig, &project, &tier, 1, "STILL_DONE")?;
    assert_eq!(notes_of(&project, rig.workspace())?.len(), 1);
    rig.close()?;
    Ok(())
}

#[test]
fn a_question_slow_to_redraw_is_given_one_key_at_a_time() -> Outcome {
    // It takes longer to move its cursor than a screen takes to read as drawn.
    let rig = Rig::start(asking().var("CF_TEST_TRUST_REDRAW_MS", "2500"))?;
    git_init(rig.workspace())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    a_task(&rig, &project, &tier, 1, "SLOW_REDRAW_DONE")?;
    assert_eq!(keys_given(&rig), ["down", "enter"]);
    rig.close()?;
    Ok(())
}

#[test]
fn a_window_that_asks_nothing_never_reads_as_asking() -> Outcome {
    // Its first frame holds still for three seconds before its prompt.
    let rig = Rig::start(asking().var("CF_TEST_TRUST_FIRST_FRAME_MS", "3000"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} SLEEP 3 Reply with exactly: PLAIN_DONE"),
    )?;
    // Every look at the board while the worker's window opens and works.
    rig.wait_for("T-1 to be done", secs(60), || {
        let lane = project.lane("worker")?;
        assert_ne!(
            lane["activity"]["reason"], "the window asks whether to trust its folder",
            "{lane}"
        );
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_humans_no_in_one_window_is_no_yes_for_the_others() -> Outcome {
    // A No is said on the window's screen, and the window takes three seconds to end.
    let rig = Rig::start(asking().var("CF_TEST_TRUST_EXIT_MS", "3000"))?;
    let root = rig.workspace().to_path_buf();
    git_init(&root)?;
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    two_tasks(&rig, &project)?;
    rig.wait_for("both of the worker's windows to ask", secs(60), || {
        Ok(!notes_of(&project, &root)?.is_empty() && workers(&rig, &project).len() == 2)
    })?;
    // The human says No in one of them: Enter on the choice it opens on.
    let window = workers(&rig, &project)[0].clone();
    let pressed = rig.host(
        "pane.input",
        json!({ "id": window["id"], "generation": window["generation"], "bytes": ENTER }),
    )?;
    assert_eq!(pressed["ok"], true, "{pressed}");
    sleep(Duration::from_secs(8));
    assert_eq!(trusted(&rig), "", "the other window was answered yes");
    assert!(
        !keys_given(&rig).iter().any(|key| key == "down"),
        "a key toward yes went into the other window: {:?}",
        keys_given(&rig)
    );
    rig.close()?;
    Ok(())
}

/// `git` in `folder` with `words`, which must do what it is asked.
fn git(folder: &Path, words: &[&str]) -> Outcome {
    let ran = Run::new(Path::new("git"))
        .arg("-C")
        .arg(folder)
        .args([
            "-c",
            "user.name=rig",
            "-c",
            "user.email=rig@example.invalid",
        ])
        .args(words)
        .inheriting_env()
        .run()?;
    assert_eq!(ran.code, Some(0), "git {words:?}: {ran}");
    Ok(())
}

#[test]
fn a_project_on_a_linked_worktree_leaves_the_main_checkouts_trust_to_the_human() -> Outcome {
    // Claude Code keeps a yes in a linked worktree for the checkout it belongs to.
    let rig = Rig::start(asking())?;
    let main = rig.workspace().join("main");
    std::fs::create_dir(&main)?;
    git_init(&main)?;
    git(&main, &["commit", "-q", "--allow-empty", "-m", "first"])?;
    let tree = rig.workspace().join("tree");
    git(&main, &["worktree", "add", "-q", &tree.to_string_lossy()])?;
    let project = Project::open(&rig, "chief", json!({ "directory": tree }))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: TREE_DONE"),
    )?;
    rig.wait_for(
        "the human to be told of the main checkout",
        secs(60),
        || Ok(!notes_of(&project, &main)?.is_empty()),
    )?;
    sleep(Duration::from_secs(4));
    assert!(keys_given(&rig).is_empty(), "{:?}", keys_given(&rig));
    let notes = notes_of(&project, &main)?;
    assert!(
        notes[0].contains("outside the project's folder"),
        "{notes:?}"
    );
    rig.close()?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_link_in_the_projects_path_leaves_the_repository_it_leads_into_to_the_human() -> Outcome {
    // The project's folder is reached through a link into a folder of a repository no
    // folder of its own path is in.
    let rig = Rig::start(asking())?;
    let repository = rig.workspace().join("B").join("repo");
    std::fs::create_dir_all(repository.join("x").join("sub"))?;
    git_init(&repository)?;
    std::fs::create_dir(rig.workspace().join("A"))?;
    std::os::unix::fs::symlink(repository.join("x"), rig.workspace().join("A").join("link"))?;
    let folder = rig.workspace().join("A").join("link").join("sub");
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: LINK_DONE"),
    )?;
    rig.wait_for("the human to be told of the repository", secs(60), || {
        Ok(!notes_of(&project, &repository)?.is_empty())
    })?;
    sleep(Duration::from_secs(4));
    assert!(keys_given(&rig).is_empty(), "{:?}", keys_given(&rig));
    assert_eq!(trusted(&rig), "");
    rig.close()?;
    Ok(())
}

#[test]
fn a_window_opened_again_that_asks_is_given_nothing_until_the_human_has_answered() -> Outcome {
    // A window opened again on its conversation reads at rest from its record while
    // it asks: what is told to it must wait, or its Enter would choose "No, exit".
    let rig = Rig::start(asking())?;
    let root = rig.workspace().to_path_buf();
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    a_task(&rig, &project, &tier, 1, "FIRST_DONE")?;
    // The folder above the project's is made a repository: a yes is the human's.
    git_init(&root)?;
    rig.tell(
        project.id(),
        "CF task add --after T-1 :: Reply with exactly: SECOND_DONE",
    )?;
    let opened = workers(&rig, &project).len();
    rig.wait_for("the window opened again to ask", secs(60), || {
        Ok(!notes_of(&project, &root)?.is_empty() && workers(&rig, &project).len() > opened)
    })?;
    rig.tell(project.id(), "CF tell T-2 :: KEEP_THIS_IN_MIND")?;
    sleep(Duration::from_secs(6));
    let window = workers(&rig, &project)
        .last()
        .cloned()
        .ok_or("no window of the worker")?;
    let gone = rig
        .exits()
        .iter()
        .any(|exit| exit["id"] == window["id"] && exit["generation"] == window["generation"]);
    assert!(
        !gone,
        "the window that asks was given what was told to it, and ended"
    );
    for keys in [DOWN, ENTER] {
        let pressed = rig.host(
            "pane.input",
            json!({ "id": window["id"], "generation": window["generation"], "bytes": keys }),
        )?;
        assert_eq!(pressed["ok"], true, "{pressed}");
        sleep(Duration::from_millis(600));
    }
    rig.wait_for("T-2 to be done", secs(90), || {
        let state = project.task(2)?["state"].clone();
        assert_ne!(state, "failed", "T-2 failed");
        Ok(state == "done")
    })?;
    rig.close()?;
    Ok(())
}

/// The chief's windows opened so far, oldest first.
fn chiefs(rig: &Rig, project: &Project) -> Vec<Value> {
    let id = format!("p{}-chief", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] == id.as_str())
        .collect()
}

/// The result of T-`number` for the chief, once there is one.
fn result_for(project: &Project, number: i64) -> cf_e2e::Result<Option<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "result" && message["taskNumber"] == number))
}

#[test]
fn a_chief_opened_again_at_rest_that_asks_is_handed_nothing_until_the_human_has_answered() -> Outcome
{
    chief_opened_again_at_rest_and_asking(false)
}

#[test]
fn a_chief_opened_again_at_rest_whose_prompt_is_drawn_otherwise_is_handed_its_result_after_the_yes(
) -> Outcome {
    // Nothing goes into it before it is past the question, so its record cannot grow:
    // its prompt, drawn otherwise than the reader knows, is all there is to go by.
    chief_opened_again_at_rest_and_asking(true)
}

/// The chief opened again at rest after the app's restart asks of a folder outside the
/// project, its prompt drawn otherwise where `plain`; a worker's result comes for it
/// meanwhile, and must reach it once the human has said yes.
fn chief_opened_again_at_rest_and_asking(plain: bool) -> Outcome {
    // A chief opened again on its conversation after the app's restart has nothing to
    // begin a turn with: what comes for it waits for its window to be ready, and a
    // window that asks reads at rest from its record, so the gate is ConsensFlow's.
    let mut rig = Rig::start(config())?;
    let root = rig.workspace().to_path_buf();
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    // The worker is the staff from the start: a member added later changes the
    // chief's role, which a note would then say to it first.
    let project = Project::open(
        &rig,
        "chief",
        json!({ "directory": folder, "staff": [{ "agent": "worker", "roles": ["worker"] }] }),
    )?;
    let id = project.id();
    let tier = project.tiers()?.get("worker").cloned().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{id}-chief"), cf_e2e::rig::OPEN)?;
    let added = rig.cf_in_window(
        &chief,
        [
            "task",
            "add",
            "--tier",
            &tier,
            "SLEEP 40 Reply with exactly: LATE_OK",
        ],
    )?;
    assert_eq!(added.code, Some(0), "{added}");
    rig.wait_for("T-1 at work and the chief at rest", secs(60), || {
        Ok(project.task(1)?["state"] == "working"
            && project.lane("chief")?["activity"]["state"] == "idle")
    })?;
    // The project's folder is made a repository, and the app quits and starts again.
    git_init(&root)?;
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let kept = rig.close_keeping_the_root()?;
    let restarted = config().var("CF_TEST_TRUST", "chief").existing_root(kept);
    let rig = Rig::start(if plain {
        restarted.var("CF_TEST_PLAIN_PROMPT", "1")
    } else {
        restarted
    })?;
    let project = Project::new(&rig, id);
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    rig.wait_for("the chief's window to ask", secs(60), || {
        Ok(chiefs(&rig, &project).last().is_some_and(|frame| {
            rig.screen(frame["id"].as_str().unwrap_or_default())
                .contains("Yes, I trust this folder")
        }))
    })?;
    let window = chiefs(&rig, &project)
        .last()
        .cloned()
        .ok_or("no chief window")?;
    // The worker's result comes while the chief's window asks, and waits.
    rig.wait_for("T-1's result for the chief", secs(90), || {
        Ok(result_for(&project, 1)?.is_some())
    })?;
    sleep(Duration::from_secs(6));
    let gone = rig
        .exits()
        .iter()
        .any(|exit| exit["id"] == window["id"] && exit["generation"] == window["generation"]);
    assert!(
        !gone,
        "the chief's window was handed the result while it asked, and ended"
    );
    for keys in [DOWN, ENTER] {
        let pressed = rig.host(
            "pane.input",
            json!({ "id": window["id"], "generation": window["generation"], "bytes": keys }),
        )?;
        assert_eq!(pressed["ok"], true, "{pressed}");
        sleep(Duration::from_millis(600));
    }
    rig.wait_for("T-1's result to reach the chief", secs(60), || {
        Ok(result_for(&project, 1)?.is_some_and(|result| result["state"] == "delivered"))
    })?;
    rig.close()?;
    Ok(())
}

/// The rig whose worker's windows ask, its stand-in set by `vars` besides.
fn asking_with(vars: &[(&str, &str)]) -> cf_e2e::Result<Rig> {
    Rig::start(
        vars.iter()
            .fold(asking(), |config, &(name, value)| config.var(name, value)),
    )
}

#[test]
fn a_window_whose_prompt_is_drawn_otherwise_goes_on_after_the_yes() -> Outcome {
    // Its prompt never says "bypass permissions on" (a reworded status line, or a
    // managed setting that keeps the session out of that mode).
    let rig = asking_with(&[("CF_TEST_PLAIN_PROMPT", "1")])?;
    git_init(rig.workspace())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    a_task(&rig, &project, &tier, 1, "PLAIN_PROMPT_DONE")?;
    assert_eq!(keys_given(&rig), ["down", "enter"]);
    rig.close()?;
    Ok(())
}

#[test]
fn a_window_that_did_not_take_a_key_is_given_it_again() -> Outcome {
    let rig = asking_with(&[("CF_TEST_TRUST_DROPS_FIRST_KEY", "1")])?;
    git_init(rig.workspace())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    a_task(&rig, &project, &tier, 1, "AGAIN_DONE")?;
    assert_eq!(keys_given(&rig), ["dropped", "down", "enter"]);
    rig.close()?;
    Ok(())
}

#[test]
fn a_screen_that_quotes_the_question_asks_nothing() -> Outcome {
    // A plain folder: nothing asks. The window's screen names both choices, as a
    // conversation about this very dialog does.
    let rig = asking_with(&[(
        "CF_TEST_SAYS_FIRST",
        "Claude offers \"No, exit\" and \"Yes, I trust this folder\" there.",
    )])?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    a_task(&rig, &project, &tier, 1, "QUOTED_DONE")?;
    let notes = notes_of(&project, rig.workspace())?;
    assert!(
        notes.is_empty(),
        "a quoted question was answered: {notes:?}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_record_that_grows_as_a_no_ends_the_window_is_no_yes() -> Outcome {
    // A window opened again on its conversation asks of a folder outside the project,
    // and so does a new window of the worker. The human says No in the first, and a
    // process before it, which the host could not end, writes to its record as it
    // ends: the record grew, and nobody said yes.
    let rig = asking_with(&[
        ("CF_TEST_TRUST_EXIT_MS", "4000"),
        ("CF_TEST_RECORD_GROWS_ON_NO", "1"),
    ])?;
    let root = rig.workspace().to_path_buf();
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    a_task(&rig, &project, &tier, 1, "FIRST_DONE")?;
    git_init(&root)?;
    let opened = workers(&rig, &project).len();
    rig.tell(
        project.id(),
        "CF task add --after T-1 :: Reply with exactly: AFTER_DONE",
    )?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: NEW_DONE"),
    )?;
    rig.wait_for("both of the worker's windows to ask", secs(60), || {
        Ok(!notes_of(&project, &root)?.is_empty() && workers(&rig, &project).len() == opened + 2)
    })?;
    let resumed = workers(&rig, &project)[opened..]
        .iter()
        .find(|frame| {
            frame["argv"]
                .as_array()
                .is_some_and(|argv| argv.iter().any(|word| word == "--resume"))
        })
        .cloned()
        .ok_or("no window was opened again on its conversation")?;
    sleep(Duration::from_secs(2));
    let pressed = rig.host(
        "pane.input",
        json!({ "id": resumed["id"], "generation": resumed["generation"], "bytes": ENTER }),
    )?;
    assert_eq!(pressed["ok"], true, "{pressed}");
    sleep(Duration::from_secs(9));
    assert_eq!(trusted(&rig), "", "the other window was answered yes");
    assert!(
        !keys_given(&rig).iter().any(|key| key == "down"),
        "a key toward yes went into the other window: {:?}",
        keys_given(&rig)
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_window_opened_again_whose_record_cannot_be_read_at_first_is_still_answered() -> Outcome {
    // The follow-up's window cannot read its record for its first 1.5 s (it was moved
    // aside before the window opened, an empty one in its place), and asks after 3 s:
    // the record found then is the conversation it opened with, and no growth.
    let rig = asking_with(&[
        ("CF_TEST_RECORD_AWAY_MS", "1500"),
        ("CF_TEST_TRUST_FIRST_FRAME_MS", "3000"),
    ])?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    a_task(&rig, &project, &tier, 1, "FIRST_DONE")?;
    git_init(rig.workspace())?;
    let session = workers(&rig, &project)
        .last()
        .and_then(|frame| {
            let argv = frame["argv"].as_array()?;
            let at = argv.iter().position(|word| word == "--session-id")?;
            argv.get(at + 1)?.as_str().map(str::to_owned)
        })
        .ok_or("the worker's window has no session")?;
    let config = Path::new(rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default()).to_path_buf();
    let transcript = config
        .join("projects")
        .join("integration")
        .join(format!("{session}.jsonl"));
    std::fs::rename(&transcript, transcript.with_extension("away"))?;
    // An empty record in its place: the conversation is there to resume, and its
    // first looks read nothing of it.
    std::fs::write(&transcript, "")?;
    rig.tell(
        project.id(),
        "CF task add --after T-1 :: Reply with exactly: SECOND_DONE",
    )?;
    rig.wait_for("T-2 to be done", secs(90), || {
        let state = project.task(2)?["state"].clone();
        assert_ne!(state, "failed", "T-2 failed");
        Ok(state == "done")
    })?;
    assert_eq!(notes_of(&project, rig.workspace())?.len(), 1);
    rig.close()?;
    Ok(())
}
