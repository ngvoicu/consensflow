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

/// The chief gives the worker two tasks at once, each with `work` before its words (a
/// `SLEEP` that makes it take a while, or nothing).
fn two_tasks(rig: &Rig, project: &Project, work: &str) -> Outcome {
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    for word in ["FIRST_DONE", "SECOND_DONE"] {
        rig.tell(
            project.id(),
            &format!("DISPATCH --tier {tier} {work}Reply with exactly: {word}"),
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
    two_tasks(&rig, &project, "")?;
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
    // Each task works 3 s, as a real one takes a while: one done at once has its window
    // closed before a look under load reads its prompt, and the yes is not taken as the
    // human's (round 5's plants, T-236).
    two_tasks(&rig, &project, "SLEEP 3 ")?;
    rig.wait_for(
        "the human to be told of the folder outside",
        secs(60),
        || Ok(!notes_of(&project, &root)?.is_empty()),
    )?;
    rig.wait_for("both of the worker's windows to be open", secs(30), || {
        Ok(workers(&rig, &project).len() == 2)
    })?;
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
    rig.wait_for("the human's window to take their yes", secs(30), || {
        Ok(!trusted(&rig).is_empty())
    })?;
    rig.wait_for(
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
    rig.wait_for(
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
    two_tasks(&rig, &project, "")?;
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
    chief_opened_again_at_rest_and_asking(Shown::AsDrawn)
}

#[test]
fn a_chief_opened_again_at_rest_whose_prompt_is_drawn_otherwise_is_handed_its_result_after_the_yes(
) -> Outcome {
    // Nothing goes into it before it is past the question, so its record cannot grow:
    // its prompt, drawn otherwise than the reader knows, is all there is to go by.
    chief_opened_again_at_rest_and_asking(Shown::PlainPrompt)
}

#[test]
fn a_chief_opened_again_at_rest_whose_screen_goes_unread_while_it_asks_is_handed_nothing_till_it_is_read(
) -> Outcome {
    // Calliope's re-check of round 5 (T-299): the host shows none of the chief's
    // screens for over 30 s while it asks, and a worker's result waits for it. Once the
    // host showed them again, the paste went in, and its Enter chose "No, exit".
    chief_opened_again_at_rest_and_asking(Shown::UnreadTillTold)
}

/// How the chief's window is shown while it asks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shown {
    /// As the stand-in draws it.
    AsDrawn,
    /// Its prompt drawn otherwise than the reader knows.
    PlainPrompt,
    /// None of its screens once it asks, until the human is told so, which is after
    /// ConsensFlow waited 30 s.
    UnreadTillTold,
}

/// The chief opened again at rest after the app's restart asks of a folder outside the
/// project, shown as `shown` says; a worker's result comes for it meanwhile, and must
/// reach it once the human has said yes, and not before.
fn chief_opened_again_at_rest_and_asking(shown: Shown) -> Outcome {
    // A chief opened again on its conversation after the app's restart has nothing to
    // begin a turn with: what comes for it waits for its window to be ready, and a
    // window that asks may read at rest, so the gate is ConsensFlow's.
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
    // It says it is idle while it asks, so that only ConsensFlow's gate holds what
    // waits for it.
    let restarted = config()
        .var("CF_TEST_TRUST", "chief")
        .var("CF_TEST_TRUST_IDLE_WHILE_ASKING", "1")
        .existing_root(kept);
    let rig = Rig::start(if shown == Shown::PlainPrompt {
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
    let unread = shown == Shown::UnreadTillTold;
    rig.wait_for("the chief's window to be held as asking", secs(30), || {
        Ok(project.lane("chief")?["activity"]["reason"]
            == "the window asks whether to trust its folder")
    })?;
    rig.refuse_snapshots(unread);
    // The worker's result comes while the chief's window asks, and waits; where its
    // screen goes unread, until the human is told so, past the 30 s ConsensFlow waits.
    let waits = rig
        .wait_for("T-1's result for the chief", secs(90), || {
            Ok(result_for(&project, 1)?.is_some())
        })
        .and_then(|()| {
            if !unread {
                return Ok(());
            }
            rig.wait_for(
                "the human to be told the chief's screen cannot be read",
                secs(60),
                || {
                    Ok(notes_of(&project, &root)?
                        .iter()
                        .any(|note| note.contains(UNREAD)))
                },
            )
        });
    rig.refuse_snapshots(false);
    waits?;
    // Long enough for a window whose screen can be read again to be drawn and ready.
    sleep(Duration::from_secs(if unread { 20 } else { 6 }));
    let gone = rig
        .exits()
        .iter()
        .any(|exit| exit["id"] == window["id"] && exit["generation"] == window["generation"]);
    assert!(
        !gone,
        "the chief's window was handed the result while it asked, and ended"
    );
    assert_eq!(
        keys_given(&rig),
        Vec::<String>::new(),
        "a key went into the chief's question"
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
fn a_chief_whose_screen_quotes_the_question_takes_what_it_is_told() -> Outcome {
    // Calliope's check of round 4 (T-231): a chief at rest whose last lines list the two
    // choices, each a line of its own, above its empty input box read as the question
    // with its cursor unplaced, and was held for good. A window that stays: a worker's
    // goes once its task is done, which its command line gave it, whatever the engine
    // read of its screen. What the human tells the chief is pasted into it.
    let rig = Rig::start(config().var("CF_TEST_TRUST", "chief").var(
        "CF_TEST_SAYS_FIRST",
        " ⏺ The dialog offers:\r\n   - No, exit\r\n   - Yes, I trust this folder\r\n ❯ ",
    ))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    rig.tell(project.id(), "Reply with exactly: CHIEF_HEARD")?;
    let chief = project.lane("chief")?;
    assert_eq!(chief["activity"]["state"], "idle", "{chief}");
    let notes = notes_of(&project, rig.workspace())?;
    assert!(
        notes.is_empty(),
        "a quoted question was answered: {notes:?}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_chief_whose_screen_quotes_the_question_in_words_and_draws_its_prompt_otherwise_is_not_asked(
) -> Outcome {
    // Both choices among other words, and no prompt the reader knows to end the watch:
    // the window never asked, and after its time it takes what it is told.
    let rig = Rig::start(
        config()
            .var("CF_TEST_TRUST", "chief")
            .var("CF_TEST_PLAIN_PROMPT", "1")
            .var(
                "CF_TEST_SAYS_FIRST",
                "Claude offers \"No, exit\" and \"Yes, I trust this folder\" there.",
            ),
    )?;
    let project = Project::open(&rig, "chief", json!({}))?;
    rig.tell(project.id(), "Reply with exactly: CHIEF_HEARD")?;
    let notes = notes_of(&project, rig.workspace())?;
    assert!(
        notes.is_empty(),
        "a quoted question was answered: {notes:?}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_chief_whose_conversation_quotes_the_question_before_its_prompt_says_no_yes_for_the_others(
) -> Outcome {
    // Calliope's check of round 4 (T-231): a window opened again draws its conversation
    // before its prompt, and one that quoted the two choices read as the question, then
    // its prompt as the human's yes, which answered the project's other windows.
    chief_quoting_before_its_prompt(&[])
}

#[test]
fn a_chief_whose_conversation_quotes_the_dialog_cursor_and_all_says_no_yes_for_the_others(
) -> Outcome {
    // Calliope's re-check of round 5 (T-299): a quote as the dialog draws it, the `❯` on
    // its first choice, read as the question with its choice placed: the human was told
    // of the chief's question, and its prompt banked a yes nobody gave.
    chief_quoting_before_its_prompt(&[("CF_TEST_QUOTES_THE_CURSOR", "1")])
}

/// The chief's conversation quotes the question's choices for 1.5 s before its prompt,
/// its stand-in set by `vars` besides; the worker's window asks of the repository's
/// root, outside the project's folder, which is the human's.
fn chief_quoting_before_its_prompt(vars: &[(&str, &str)]) -> Outcome {
    let rig = asking_with(&[&[("CF_TEST_QUOTES_BEFORE_PROMPT", "chief")], vars].concat())?;
    let root = rig.workspace().to_path_buf();
    git_init(&root)?;
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: QUOTED_TAIL_DONE"),
    )?;
    rig.wait_for(
        "the human to be told of the worker's question",
        secs(60),
        || {
            Ok(notes_of(&project, &root)?
                .iter()
                .any(|note| note.contains("@worker")))
        },
    )?;
    let notes = notes_of(&project, &root)?;
    assert!(
        notes.iter().all(|note| !note.contains("@chief")),
        "the chief's quote was told as a question: {notes:?}"
    );
    sleep(Duration::from_secs(3));
    assert_eq!(
        keys_given(&rig),
        Vec::<String>::new(),
        "the worker's question was answered for the human"
    );
    let window = workers(&rig, &project)[0].clone();
    human_presses(&rig, &window, &[DOWN, ENTER])?;
    rig.wait_for("T-1 to be done", secs(60), || {
        let state = project.task(1)?["state"].clone();
        assert_ne!(state, "failed", "T-1 failed after the yes");
        Ok(state == "done")
    })?;
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

/// The human presses `keys` into `window`, a moment apart, as a person does.
fn human_presses(rig: &Rig, window: &Value, keys: &[&[u8]]) -> Outcome {
    for key in keys {
        let pressed = rig.host(
            "pane.input",
            json!({ "id": window["id"], "generation": window["generation"], "bytes": key }),
        )?;
        assert_eq!(pressed["ok"], true, "{pressed}");
        sleep(Duration::from_millis(600));
    }
    Ok(())
}

#[test]
fn a_window_whose_screen_the_host_cannot_show_while_it_asks_is_still_held() -> Outcome {
    // Calliope's review of round 3 (T-120): looks the host could not answer while the
    // question was up read as the question gone, and once they had for 12 s the first
    // message's Enter chose "No, exit". The worker's window asks of a folder outside the
    // project, and the host shows none of its screens for longer than that.
    let rig = Rig::start(asking())?;
    let root = rig.workspace().to_path_buf();
    git_init(&root)?;
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: UNREAD_DONE"),
    )?;
    rig.wait_for("the worker's window to ask", secs(60), || {
        Ok(!notes_of(&project, &root)?.is_empty() && workers(&rig, &project).len() == 1)
    })?;
    rig.refuse_snapshots(true);
    sleep(Duration::from_secs(16));
    rig.refuse_snapshots(false);
    sleep(Duration::from_secs(1));
    let lane = project.lane("worker")?;
    assert_eq!(
        lane["activity"]["reason"], "the window asks whether to trust its folder",
        "{lane}"
    );
    assert_eq!(
        keys_given(&rig),
        Vec::<String>::new(),
        "a key went into the question"
    );
    // The human says yes in it, and its task goes on.
    let window = workers(&rig, &project)[0].clone();
    human_presses(&rig, &window, &[DOWN, ENTER])?;
    rig.wait_for("T-1 to be done", secs(60), || {
        let state = project.task(1)?["state"].clone();
        assert_ne!(state, "failed", "T-1 failed after the yes");
        Ok(state == "done")
    })?;
    assert_eq!(keys_given(&rig), ["down", "enter"]);
    rig.close()?;
    Ok(())
}

#[test]
fn a_question_whose_cursor_is_drawn_otherwise_is_told_of_again_once_the_human_answered_another(
) -> Outcome {
    // Calliope's re-check of round 5 (T-299): two of the worker's windows ask of one
    // folder outside the project, each with its cursor drawn otherwise, and the human
    // is told once. Their yes in the first cannot be carried to the second, which no
    // key of ConsensFlow's can answer: it was held, and nobody was told of it again.
    let rig = asking_with(&[("CF_TEST_TRUST_CURSOR", ">")])?;
    let root = rig.workspace().to_path_buf();
    git_init(&root)?;
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    two_tasks(&rig, &project, "SLEEP 3 ")?;
    rig.wait_for("both windows to ask, and the human told", secs(60), || {
        Ok(workers(&rig, &project).len() == 2 && notes_of(&project, &root)?.len() == 1)
    })?;
    let windows = workers(&rig, &project);
    human_presses(&rig, &windows[0], &[DOWN, ENTER])?;
    let other = windows[1]["id"]
        .as_str()
        .and_then(|id| id.split_once('-'))
        .map(|(_, handle)| format!("@{handle}'s window"))
        .ok_or("the other window has no id")?;
    rig.wait_for("the human to be told of the other window", secs(60), || {
        let notes = notes_of(&project, &root)?;
        Ok(notes.len() == 2 && notes.iter().any(|note| note.contains(&other)))
    })?;
    assert_eq!(
        keys_given(&rig),
        ["down", "enter"],
        "a key went into the other window"
    );
    human_presses(&rig, &windows[1], &[DOWN, ENTER])?;
    rig.wait_for("T-1 and T-2 to be done", secs(60), || both_done(&project))?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_question_whose_cursor_is_drawn_otherwise_is_left_to_the_human_who_is_told() -> Outcome {
    // Calliope's review of round 3 (T-120): a question whose cursor the reader could not
    // place read as no question, and after 30 s the first message's Enter chose "No,
    // exit". Its cursor here is drawn as a Claude Code that changed its glyph would draw it.
    let rig = asking_with(&[("CF_TEST_TRUST_CURSOR", ">")])?;
    git_init(rig.workspace())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: OTHERWISE_DONE"),
    )?;
    rig.wait_for(
        "the human to be told ConsensFlow could not answer",
        secs(60),
        || {
            let notes = notes_of(&project, rig.workspace())?;
            Ok(notes.iter().any(|note| note.contains("could not answer")))
        },
    )?;
    assert_eq!(
        keys_given(&rig),
        Vec::<String>::new(),
        "a key went into the question"
    );
    let lane = project.lane("worker")?;
    assert_eq!(
        lane["activity"]["reason"], "the window asks whether to trust its folder",
        "{lane}"
    );
    let window = workers(&rig, &project)[0].clone();
    human_presses(&rig, &window, &[DOWN, ENTER])?;
    rig.wait_for("T-1 to be done", secs(60), || {
        let state = project.task(1)?["state"].clone();
        assert_ne!(state, "failed", "T-1 failed after the yes");
        Ok(state == "done")
    })?;
    assert_eq!(keys_given(&rig), ["down", "enter"]);
    let notes = notes_of(&project, rig.workspace())?;
    assert_eq!(notes.len(), 1, "the human is told once: {notes:?}");
    rig.close()?;
    Ok(())
}

/// The note that the window's screen could not be read.
const UNREAD: &str = "could not be read";

/// When the note that a window's screen could not be read was written, in
/// milliseconds since the epoch.
fn unread_note_written(project: &Project) -> cf_e2e::Result<Option<i64>> {
    Ok(project
        .inbox("human")?
        .iter()
        .find(|message| {
            message["body"]
                .as_str()
                .is_some_and(|body| body.contains(UNREAD))
        })
        .and_then(|message| {
            message["createdAt"]
                .as_str()?
                .parse::<jiff::Timestamp>()
                .ok()
        })
        .map(jiff::Timestamp::as_millisecond))
}

#[test]
fn a_window_whose_screen_the_host_never_shows_while_it_asks_is_told_of_and_held_till_it_is_read(
) -> Outcome {
    // Calliope's check of round 4 (T-231): a window that asked, whose host then showed
    // none of its screens, was held for good, and nobody was told. The human is told
    // now, and it is held still: nothing may go into a question no look has read. The
    // page is told the board moved as the note is written (Calliope's last re-check,
    // T-325): on a board where nothing else moves, it showed the note late.
    let rig = Rig::start(asking())?;
    let root = rig.workspace().to_path_buf();
    git_init(&root)?;
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: NEVER_SHOWN_DONE"),
    )?;
    rig.wait_for("the worker's window to ask", secs(60), || {
        Ok(!notes_of(&project, &root)?.is_empty())
    })?;
    rig.refuse_snapshots(true);
    let told = rig.wait_for(
        "the human to be told the screen cannot be read",
        secs(60),
        || {
            Ok(notes_of(&project, &root)?
                .iter()
                .any(|note| note.contains(UNREAD)))
        },
    );
    sleep(Duration::from_secs(2));
    let lane = project.lane("worker");
    let written = unread_note_written(&project);
    let moves = rig.board_moves();
    rig.refuse_snapshots(false);
    told?;
    let lane = lane?;
    let written = written?.ok_or("the note has no time it was written")?;
    assert!(
        moves
            .iter()
            .any(|&moved| (written..written + 1_000).contains(&moved)),
        "the page was not told the board moved within 1 s of the note written at {written}: {moves:?}"
    );
    assert_eq!(
        lane["activity"]["reason"], "the window asks whether to trust its folder",
        "{lane}"
    );
    assert_eq!(
        keys_given(&rig),
        Vec::<String>::new(),
        "a key went into the question"
    );
    let window = workers(&rig, &project)[0].clone();
    human_presses(&rig, &window, &[DOWN, ENTER])?;
    rig.wait_for("T-1 to be done", secs(60), || {
        let state = project.task(1)?["state"].clone();
        assert_ne!(state, "failed", "T-1 failed after the yes");
        Ok(state == "done")
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_window_whose_screen_the_host_cannot_show_is_past_the_question_once_its_record_grows() -> Outcome
{
    // The human says No in a window opened again on its conversation while the host
    // shows none of its screens, and a process before it writes to its record as it
    // ends, 45 s later: its record grew, so it is past the question, with nothing to
    // tell of its screen.
    let rig = asking_with(&[
        ("CF_TEST_TRUST_EXIT_MS", "45000"),
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
    rig.tell(
        project.id(),
        "CF task add --after T-1 :: Reply with exactly: SECOND_DONE",
    )?;
    rig.wait_for("the worker's window opened again to ask", secs(60), || {
        Ok(!notes_of(&project, &root)?.is_empty())
    })?;
    let window = workers(&rig, &project)
        .last()
        .cloned()
        .ok_or("the worker has no window")?;
    rig.refuse_snapshots(true);
    let pressed = human_presses(&rig, &window, &[ENTER]);
    sleep(Duration::from_secs(36));
    let notes = notes_of(&project, &root);
    rig.refuse_snapshots(false);
    pressed?;
    let notes = notes?;
    assert!(
        notes.iter().all(|note| !note.contains(UNREAD)),
        "a window past the question was held for its screen: {notes:?}"
    );
    rig.close()?;
    Ok(())
}

// The same dialog, asked of a folder trusted by a folder above it whose own settings
// grant more than its trust (T-354's probe, Claude Code 2.1.309: a plain folder below a
// trusted one, its `.claude/settings.json` pre-approving a tool). Its no reads "No,
// continue without these permissions" and goes on without them; its yes lets them apply
// without asking, and records as trusted the root of the folder's repository, or the
// folder out of one (T-381). ConsensFlow read it as no question: a worker's launch failed
// as never showing its first message, and what went into the window chose the no. The
// owner's decision of 2026-10-23: it is answered as the first question is, yes where the
// folder its yes records is the project's folder or below it; elsewhere it is held, the
// human told, and nothing pressed.

/// The rig, the windows of the participant `who` asking of their folder's grants.
fn asking_of_grants(who: &str) -> Config {
    config()
        .var("CF_TEST_TRUST", who)
        .var("CF_TEST_TRUST_GRANTS", "1")
        .var("CONSENSFLOW_LAUNCH_TIMEOUT_MS", "8000")
}

/// The notes to the human that a window asks of its folder's grants: not those that
/// quote a window's last lines, which may show the question's words.
fn grants_notes(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("human")?
        .iter()
        .filter_map(|message| message["body"].as_str())
        .filter(|body| body.contains("'s window asks whether the settings of its folder"))
        .map(str::to_owned)
        .collect())
}

/// The notes to the human that ConsensFlow answered a window's question.
fn answered_notes(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("human")?
        .iter()
        .filter_map(|message| message["body"].as_str())
        .filter(|body| body.contains("answered yes"))
        .map(str::to_owned)
        .collect())
}

/// The handle of the participant whose window `frame` is, as a note names it.
fn handle_of(frame: &Value) -> String {
    frame["id"]
        .as_str()
        .and_then(|id| id.split_once('-'))
        .map(|(_, handle)| format!("@{handle}'s window"))
        .unwrap_or_default()
}

/// A project's folder below the root of a repository, `app` in the rig's workspace: a
/// yes to the grants' question would trust the repository, outside the project's folder.
fn below_a_repository(rig: &Rig) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let root = rig.workspace().to_path_buf();
    git_init(&root)?;
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    Ok(folder)
}

#[test]
fn a_worker_asked_of_the_grants_of_the_projects_own_folder_is_answered_yes_and_does_its_task(
) -> Outcome {
    // A plain folder: a yes records the folder itself, the project's.
    let rig = Rig::start(asking_of_grants("worker"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: GRANTED_DONE"),
    )?;
    rig.wait_for("T-1 to be done", secs(60), || {
        let state = project.task(1)?["state"].clone();
        assert_ne!(state, "failed", "T-1 failed at the question");
        Ok(state == "done")
    })?;
    assert_eq!(keys_given(&rig), ["down", "enter"], "not one yes");
    let folder = real(rig.workspace());
    assert!(
        trusted(&rig)
            .lines()
            .any(|line| real(Path::new(line)) == folder),
        "the yes trusted no folder: {}",
        trusted(&rig)
    );
    let answered = answered_notes(&project)?;
    assert_eq!(answered.len(), 1, "the human is told once: {answered:?}");
    assert!(answered[0].contains("apply without asking"), "{answered:?}");
    let held: Vec<String> = grants_notes(&project)?
        .into_iter()
        .filter(|note| !note.contains("answered yes"))
        .collect();
    assert_eq!(held, Vec::<String>::new(), "told as held");
    rig.close()?;
    Ok(())
}

#[test]
fn a_worker_asked_of_grants_whose_yes_would_trust_a_repository_outside_the_project_is_held_and_the_human_told(
) -> Outcome {
    // Calliope's review (T-381): a yes records the repository's root, which here lies
    // outside the project's folder.
    let rig = Rig::start(asking_of_grants("worker"))?;
    let folder = below_a_repository(&rig)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: GRANTS_DONE"),
    )?;
    rig.wait_for("the human to be told of the grants", secs(60), || {
        Ok(grants_notes(&project)?.len() == 1)
    })?;
    // Held past the 8 s its launch has, and nothing pressed.
    sleep(Duration::from_secs(10));
    assert_eq!(
        keys_given(&rig),
        Vec::<String>::new(),
        "a key went into the question"
    );
    let lane = project.lane("worker")?;
    assert_eq!(
        lane["activity"]["reason"], "the window asks whether to trust its folder",
        "{lane}"
    );
    assert_ne!(
        project.task(1)?["state"],
        "failed",
        "T-1 failed while it asked"
    );
    let window = workers(&rig, &project)[0].clone();
    let notes = grants_notes(&project)?;
    assert!(notes[0].contains(&handle_of(&window)), "{notes:?}");
    assert!(
        notes[0].contains(&real(rig.workspace())) && notes[0].contains("outside"),
        "the note does not name the repository's root outside the project: {notes:?}"
    );
    assert_eq!(
        answered_notes(&project)?,
        Vec::<String>::new(),
        "ConsensFlow said it answered"
    );
    // The human's no goes on without the grants, and the task with it.
    human_presses(&rig, &window, &[ENTER])?;
    rig.wait_for("T-1 to be done", secs(60), || {
        let state = project.task(1)?["state"].clone();
        assert_ne!(state, "failed", "T-1 failed after the no");
        Ok(state == "done")
    })?;
    assert_eq!(keys_given(&rig), ["enter"]);
    assert_eq!(trusted(&rig), "", "the no wrote a trust down");
    rig.close()?;
    Ok(())
}

#[test]
fn a_no_to_a_trusted_folders_grants_in_one_window_and_the_other_window_asking_them_is_told_of_again(
) -> Outcome {
    // Both of the worker's windows ask at once, and the human is told once, however
    // long they ask. Their no in the first is that window's alone: the second still
    // asks, and is told of again.
    let rig = Rig::start(asking_of_grants("worker"))?;
    let folder = below_a_repository(&rig)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    two_tasks(&rig, &project, "SLEEP 3 ")?;
    rig.wait_for("both windows to ask, and the human told", secs(60), || {
        Ok(workers(&rig, &project).len() == 2 && grants_notes(&project)?.len() == 1)
    })?;
    // Calliope's review (T-381): told once, held for a while.
    sleep(Duration::from_secs(4));
    let notes = grants_notes(&project)?;
    assert_eq!(notes.len(), 1, "the human is told once: {notes:?}");
    let windows = workers(&rig, &project);
    let (first, second) = if notes[0].contains(&handle_of(&windows[0])) {
        (windows[0].clone(), windows[1].clone())
    } else {
        (windows[1].clone(), windows[0].clone())
    };
    human_presses(&rig, &first, &[ENTER])?;
    rig.wait_for("the human to be told of the other window", secs(60), || {
        let notes = grants_notes(&project)?;
        Ok(notes.len() == 2 && notes.iter().any(|note| note.contains(&handle_of(&second))))
    })?;
    assert_eq!(
        keys_given(&rig),
        ["enter"],
        "a key went into the other window"
    );
    human_presses(&rig, &second, &[ENTER])?;
    rig.wait_for("T-1 and T-2 to be done", secs(60), || both_done(&project))?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_chief_asked_of_its_trusted_folders_grants_is_handed_nothing_till_the_human_answers() -> Outcome
{
    // A chief at the question read as no question was at rest: what came for it was
    // pasted into the question, and its Enter chose the no. It says it is idle while it
    // asks, so that only ConsensFlow's gate holds what waits for it (Calliope, T-381).
    let rig = Rig::start(asking_of_grants("chief").var("CF_TEST_TRUST_IDLE_WHILE_ASKING", "1"))?;
    let folder = below_a_repository(&rig)?;
    let project = Project::open(
        &rig,
        "chief",
        json!({ "directory": folder, "staff": [{ "agent": "worker", "roles": ["worker"] }] }),
    )?;
    rig.wait_for("the human to be told of the grants", secs(60), || {
        Ok(grants_notes(&project)?.len() == 1)
    })?;
    assert!(grants_notes(&project)?[0].contains("@chief"));
    let chief = project.lane("chief")?;
    assert_eq!(
        chief["activity"]["reason"], "the window asks whether to trust its folder",
        "{chief}"
    );
    // A worker's result comes for the chief while it asks, and waits.
    let tier = project.tiers()?.get("worker").cloned().unwrap_or_default();
    let window = chiefs(&rig, &project)[0].clone();
    let added = rig.cf_in_window(
        &window,
        [
            "task",
            "add",
            "--tier",
            &tier,
            "Reply with exactly: HELD_OK",
        ],
    )?;
    assert_eq!(added.code, Some(0), "{added}");
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
    assert_eq!(keys_given(&rig), Vec::<String>::new(), "a key went into it");
    human_presses(&rig, &window, &[DOWN, ENTER])?;
    rig.wait_for("T-1's result to reach the chief", secs(60), || {
        Ok(result_for(&project, 1)?.is_some_and(|result| result["state"] == "delivered"))
    })?;
    assert_eq!(keys_given(&rig), ["down", "enter"]);
    let root = real(rig.workspace());
    assert!(
        trusted(&rig)
            .lines()
            .any(|line| real(Path::new(line)) == root),
        "the yes trusted no repository: {}",
        trusted(&rig)
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_window_that_goes_while_it_asks_of_its_folders_grants_leaves_the_next_that_asks_them_told_of(
) -> Outcome {
    // The worker's window that asks is ended before anyone answers it; the window
    // opened for the worker's next task asks again, and the human is told again.
    let rig = Rig::start(asking_of_grants("worker"))?;
    let folder = below_a_repository(&rig)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: FIRST_DONE"),
    )?;
    rig.wait_for("the human to be told of the grants", secs(60), || {
        Ok(grants_notes(&project)?.len() == 1)
    })?;
    let first = workers(&rig, &project)[0].clone();
    let killed = rig.host(
        "pane.kill",
        json!({ "id": first["id"], "generation": first["generation"] }),
    )?;
    assert_eq!(killed["ok"], true, "{killed}");
    rig.wait_for("the window to go", secs(30), || {
        Ok(rig
            .exits()
            .iter()
            .any(|exit| exit["id"] == first["id"] && exit["generation"] == first["generation"]))
    })?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: SECOND_DONE"),
    )?;
    rig.wait_for("the human to be told of the next window", secs(90), || {
        Ok(grants_notes(&project)?.len() == 2)
    })?;
    assert_eq!(
        keys_given(&rig),
        Vec::<String>::new(),
        "a key went into a question"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_chief_whose_conversation_quotes_the_grants_dialog_cursor_and_all_is_told_of_no_question(
) -> Outcome {
    // Calliope's review (T-381): the question of the grants is told of only once it has
    // stayed 5 s, as the first question is, so a quote of it before the prompt is not.
    chief_quoting_before_its_prompt(&[("CF_TEST_QUOTES_THE_GRANTS", "1")])
}

// Claude Code's dialog of a server of the project's `.mcp.json` it was never told to use
// (T-354's probe; T-383's, Claude Code 2.1.309): "New MCP server found in this project",
// asked once the folder is trusted, its `❯` on "Continue without using this MCP server".
// ConsensFlow read it as no question: a worker sat on it till its launch failed, and what
// went into a chief's window answered it. The owner's decision of 2026-10-23: in the
// project's own folder ConsensFlow uses the server; elsewhere the window is held, the
// human told, and nothing pressed.

/// The key toward using the server, from the choice the dialog opens on.
const UP: &[u8] = b"\x1b[A";

/// The rig, the windows of the participant `who` asking of the server `probe` of their
/// folder's `.mcp.json`, once past any question about the folder.
fn asking_of_a_server(who: &str) -> Config {
    config()
        .var("CF_TEST_TRUST", who)
        .var("CF_TEST_TRUST_SERVER", "probe")
        .var("CONSENSFLOW_LAUNCH_TIMEOUT_MS", "8000")
}

/// The notes to the human that a window asks of a new server.
fn servers_notes(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("human")?
        .iter()
        .filter_map(|message| message["body"].as_str())
        .filter(|body| body.contains("'s window asks whether to use a new MCP server"))
        .map(str::to_owned)
        .collect())
}

/// The notes to the human that ConsensFlow used a window's new server.
fn used_notes(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("human")?
        .iter()
        .filter_map(|message| message["body"].as_str())
        .filter(|body| body.contains("ConsensFlow said to use it"))
        .map(str::to_owned)
        .collect())
}

/// What the local settings of `folder` say of the project's servers.
fn local_settings(folder: &Path) -> Value {
    files::read_string(&folder.join(".claude").join("settings.local.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// The stand-in told to trust `folder`, as a yes the human gave before.
fn trusted_before(rig: &Rig, folder: &Path) -> Outcome {
    let config = rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default();
    std::fs::write(
        Path::new(config).join("trusted-folders"),
        format!("{}\n", real(folder)),
    )?;
    Ok(())
}

/// A worker given one task that asks of the server, in the project's folder `folder`.
fn a_worker_asking_of_a_server<'a>(
    rig: &'a Rig,
    folder: &Path,
) -> Result<(Project<'a>, String), Box<dyn std::error::Error>> {
    let project = Project::open(rig, "chief", json!({ "directory": folder }))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: SERVER_DONE"),
    )?;
    Ok((project, tier))
}

#[test]
fn a_worker_asked_of_a_new_server_of_the_projects_mcp_json_has_it_used_once_and_does_its_tasks(
) -> Outcome {
    // A plain folder, trusted: the dialog comes alone, and the folder is the project's.
    let rig = Rig::start(asking_of_a_server("worker"))?;
    let (project, tier) = a_worker_asking_of_a_server(&rig, rig.workspace())?;
    rig.wait_for("T-1 to be done", secs(60), || {
        let state = project.task(1)?["state"].clone();
        assert_ne!(state, "failed", "T-1 failed at the dialog");
        Ok(state == "done")
    })?;
    // Up twice to "Use this MCP server", and Enter: the server alone, not every future one.
    assert_eq!(keys_given(&rig), ["up", "up", "enter"], "not one use");
    let settings = local_settings(rig.workspace());
    assert_eq!(
        settings["enabledMcpjsonServers"],
        json!(["probe"]),
        "{settings}"
    );
    assert_eq!(
        settings["enableAllProjectMcpServers"],
        Value::Null,
        "{settings}"
    );
    let used = used_notes(&project)?;
    assert_eq!(used.len(), 1, "the human is told once: {used:?}");
    assert!(
        used[0].contains(&real(rig.workspace())) && used[0].contains("settings.local.json"),
        "{used:?}"
    );
    assert_eq!(
        servers_notes(&project)?,
        Vec::<String>::new(),
        "told as held"
    );
    // The next window does not ask again, and nobody is told again.
    a_task(&rig, &project, &tier, 2, "AGAIN_DONE")?;
    assert_eq!(keys_given(&rig), ["up", "up", "enter"]);
    assert_eq!(used_notes(&project)?.len(), 1);
    rig.close()?;
    Ok(())
}

#[test]
fn a_worker_asked_to_trust_the_projects_new_repository_then_of_its_server_has_both_answered_each_with_its_own_keys(
) -> Outcome {
    // T-383's probe: in a new repository Claude Code asks of the folder first, then of
    // the server. Each window drops the first key of each, so the two take seven keys
    // between them, more than one question is given.
    let rig = Rig::start(
        asking_of_a_server("worker")
            .var("CF_TEST_TRUST_DROPS_FIRST_KEY", "1")
            .var("CF_TEST_TRUST_SERVER_DROPS_FIRST_KEY", "1"),
    )?;
    git_init(rig.workspace())?;
    let (project, _) = a_worker_asking_of_a_server(&rig, rig.workspace())?;
    rig.wait_for("T-1 to be done", secs(90), || {
        let state = project.task(1)?["state"].clone();
        assert_ne!(state, "failed", "T-1 failed at a question");
        Ok(state == "done")
    })?;
    assert_eq!(
        keys_given(&rig),
        ["dropped", "down", "enter", "dropped", "up", "up", "enter"]
    );
    assert_eq!(
        local_settings(rig.workspace())["enabledMcpjsonServers"],
        json!(["probe"])
    );
    assert_eq!(
        answered_notes(&project)?.len(),
        1,
        "the folder's yes told once"
    );
    assert_eq!(used_notes(&project)?.len(), 1, "the server's use told once");
    assert_eq!(
        servers_notes(&project)?,
        Vec::<String>::new(),
        "told as held"
    );
    rig.close()?;
    Ok(())
}

/// A worker asking of the server in the project's folder `folder`, which ConsensFlow
/// must not answer: held past its launch's time, nothing pressed, the human told once in
/// words that hold `why`; then the human's yes, and the task done.
fn a_worker_held_at_the_server(rig: &Rig, folder: &Path, why: &str) -> Outcome {
    let (project, _) = a_worker_asking_of_a_server(rig, folder)?;
    rig.wait_for("the human to be told of the server", secs(60), || {
        Ok(servers_notes(&project)?.len() == 1)
    })?;
    // Held past the 8 s its launch has, and nothing pressed.
    sleep(Duration::from_secs(10));
    assert_eq!(
        keys_given(rig),
        Vec::<String>::new(),
        "a key went into the dialog"
    );
    let lane = project.lane("worker")?;
    assert_eq!(
        lane["activity"]["reason"], "the window asks whether to trust its folder",
        "{lane}"
    );
    assert_ne!(
        project.task(1)?["state"],
        "failed",
        "T-1 failed while it asked"
    );
    let notes = servers_notes(&project)?;
    assert_eq!(notes.len(), 1, "the human is told once: {notes:?}");
    let window = workers(rig, &project)[0].clone();
    assert!(notes[0].contains(&handle_of(&window)), "{notes:?}");
    assert!(
        notes[0].contains(why),
        "the note does not say why: {notes:?}"
    );
    assert_eq!(
        used_notes(&project)?,
        Vec::<String>::new(),
        "ConsensFlow said it used it"
    );
    // The human's yes, by hand, and the task goes on.
    human_presses(rig, &window, &[UP, UP, ENTER])?;
    rig.wait_for("T-1 to be done", secs(60), || {
        let state = project.task(1)?["state"].clone();
        assert_ne!(state, "failed", "T-1 failed after the human's yes");
        Ok(state == "done")
    })?;
    assert_eq!(keys_given(rig), ["up", "up", "enter"]);
    assert_eq!(
        local_settings(folder)["enabledMcpjsonServers"],
        json!(["probe"])
    );
    Ok(())
}

#[test]
fn a_worker_asked_of_a_new_server_in_a_project_inside_a_repository_outside_it_is_held_and_the_human_told(
) -> Outcome {
    // The repository's root, outside the project's folder, was trusted before: the dialog
    // comes alone, and the folder a yes to the first question records is not the project's.
    let rig = Rig::start(asking_of_a_server("worker"))?;
    let folder = below_a_repository(&rig)?;
    trusted_before(&rig, rig.workspace())?;
    a_worker_held_at_the_server(
        &rig,
        &folder,
        &format!("{}, which is outside it", real(rig.workspace())),
    )?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_worker_asked_of_a_new_server_where_a_folder_above_the_project_has_servers_is_held_and_the_human_told(
) -> Outcome {
    // A plain folder whose parent has a `.mcp.json` of its own, whose servers Claude Code
    // asks of in the same dialog.
    let rig = Rig::start(asking_of_a_server("worker"))?;
    let folder = rig.workspace().join("app");
    std::fs::create_dir(&folder)?;
    std::fs::write(rig.workspace().join(".mcp.json"), r#"{"mcpServers":{}}"#)?;
    a_worker_held_at_the_server(
        &rig,
        &folder,
        &format!("{}, above the project's folder", real(rig.workspace())),
    )?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_chief_asked_of_a_new_server_it_may_not_use_is_handed_nothing_till_the_human_answers() -> Outcome
{
    // A chief at the dialog read as no question was at rest: what came for it was pasted
    // into the dialog, and its Enter chose the choice its cursor was on.
    let rig = Rig::start(asking_of_a_server("chief").var("CF_TEST_TRUST_IDLE_WHILE_ASKING", "1"))?;
    let folder = below_a_repository(&rig)?;
    trusted_before(&rig, rig.workspace())?;
    let project = Project::open(
        &rig,
        "chief",
        json!({ "directory": folder, "staff": [{ "agent": "worker", "roles": ["worker"] }] }),
    )?;
    rig.wait_for("the human to be told of the server", secs(60), || {
        Ok(servers_notes(&project)?.len() == 1)
    })?;
    assert!(servers_notes(&project)?[0].contains("@chief"));
    let tier = project.tiers()?.get("worker").cloned().unwrap_or_default();
    let window = chiefs(&rig, &project)[0].clone();
    let added = rig.cf_in_window(
        &window,
        [
            "task",
            "add",
            "--tier",
            &tier,
            "Reply with exactly: HELD_OK",
        ],
    )?;
    assert_eq!(added.code, Some(0), "{added}");
    rig.wait_for("T-1's result for the chief", secs(90), || {
        Ok(result_for(&project, 1)?.is_some())
    })?;
    sleep(Duration::from_secs(6));
    assert_ne!(
        result_for(&project, 1)?.map(|result| result["state"].clone()),
        Some(json!("delivered")),
        "the result was handed to the chief while it asked"
    );
    assert_eq!(keys_given(&rig), Vec::<String>::new(), "a key went into it");
    human_presses(&rig, &window, &[UP, UP, ENTER])?;
    rig.wait_for("T-1's result to reach the chief", secs(60), || {
        Ok(result_for(&project, 1)?.is_some_and(|result| result["state"] == "delivered"))
    })?;
    assert_eq!(keys_given(&rig), ["up", "up", "enter"]);
    assert_eq!(
        local_settings(&folder)["enabledMcpjsonServers"],
        json!(["probe"])
    );
    rig.close()?;
    Ok(())
}
