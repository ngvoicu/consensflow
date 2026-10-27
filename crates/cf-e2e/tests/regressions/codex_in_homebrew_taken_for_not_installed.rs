//! A Codex in Homebrew's folder was taken for one that is not installed.
//!
//! Seen by the owner on 2026-10-15, on 3.0.4 and Codex 0.162.1 (Homebrew's cask,
//! in `/opt/homebrew/bin`, which a terminal runs): "m-2065, a task from @chief on
//! T-439, did not reach @theia-keen-anchor: the launch failed: codex is not
//! installed on this machine." The Mac was at load 17–18 from builds when the app
//! started, and the login shell it asks for the daemon's PATH (`zsh -lc`, whose
//! `.zprofile` puts Homebrew's folders on it) had not answered within the five
//! seconds it was given: the daemon ran on the PATH the Finder gives an app,
//! `/usr/bin:/bin:/usr/sbin:/sbin`. Detection looked on that PATH and in the
//! folders under the home each harness installs itself in, which held the owner's
//! Claude (`~/.local/bin`), so only Codex broke; every program of Homebrew's a
//! window runs (`git`, `node`, `npm`, `cargo`, `brew`) was off its PATH too. The
//! report is the issue `issues/a-slow-login-shell-costs-the-path.md` in
//! consensflow-sme.
//!
//! What the owner should have seen: the task reaches the Codex worker, whose window
//! opens on the Codex in the system's folder, and its result reaches the chief; the
//! pickers offer Codex's agents; and a window runs Homebrew's programs. And, as the
//! reviews of the fix found (2026-10-16), a Codex the human keeps where it installs
//! itself is still the one a window opens on, not an older one of Homebrew's.
//!
//! The app hands its daemon the login shell's PATH, given longer now, else its own
//! (`app/src-tauri/src/daemon_command.rs`, whose tests hold that part); the daemon
//! puts the system's folders after it for everything it runs, and looks for a
//! harness on the PATH it was handed, then in the home, then in them. The rig starts
//! its daemon as the app started the owner's that night, on the Finder's PATH, with
//! the rig's folder for the system's.

use std::path::PathBuf;

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::{config, config_with_codex, secs, Outcome};

/// What the chief is told of a task its worker's window could not be opened for.
const REFUSED: &str = "the launch failed";

/// The roster of the case: the chief on the stand-in Claude, the worker on Codex.
fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": "codex", "model": "fake", "workTier": "standard" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// The bodies of the chief's messages that hold `words`.
fn saying(project: &Project, words: &str) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("chief")?
        .iter()
        .filter_map(|message| message["body"].as_str())
        .filter(|body| body.contains(words))
        .map(str::to_owned)
        .collect())
}

/// The results the chief has been given, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<Value>> {
    let mut results: Vec<Value> = project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .collect();
    results.sort_by_key(|message| message["id"].as_i64().unwrap_or_default());
    Ok(results)
}

/// The folder `name` of the rig's environment names.
fn named(rig: &Rig, name: &str) -> PathBuf {
    PathBuf::from(rig.var(name).unwrap_or_default())
}

/// A project whose Codex worker is given a task through the chief, which reaches it:
/// its result reaches the chief.
fn a_task_reaches_the_codex_worker(rig: &Rig) -> cf_e2e::Result<Project<'_>> {
    write_roster(rig)?;
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or("standard");
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: CODEX_OK"),
    )?;
    rig.wait_for(
        "T-1 to be done, or the chief to be told why not",
        secs(60),
        || Ok(project.task(1)?["state"] == "done" || !saying(&project, REFUSED)?.is_empty()),
    )?;
    let refused = saying(&project, REFUSED)?;
    assert!(
        refused.is_empty(),
        "the task never reached the worker: {refused:?}"
    );
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "CODEX_OK");
    Ok(project)
}

/// Whether a window of the rig ran `program`.
fn a_window_ran(rig: &Rig, program: &std::path::Path) -> bool {
    let program = program.to_string_lossy();
    rig.open_frames().iter().any(|frame| {
        frame["argv"]
            .as_array()
            .is_some_and(|argv| argv.iter().any(|word| word == program.as_ref()))
    })
}

#[test]
fn a_task_for_a_codex_worker_whose_codex_is_only_in_the_systems_folder_reaches_it_and_its_result_the_chief(
) -> Outcome {
    let rig = Rig::start(config_with_codex().as_the_finder_starts_it())?;
    a_task_reaches_the_codex_worker(&rig)?;
    // The worker's window ran the Codex of the system's folder, the only one there is.
    let codex = named(&rig, "CONSENSFLOW_TOOL_FOLDERS").join("codex");
    assert!(
        a_window_ran(&rig, &codex),
        "no window ran {}: {:?}",
        codex.display(),
        rig.open_frames()
    );
    // And the pickers offer Codex's agents: none of its harness is missing.
    let agents = rig.page("agents.list", json!({}))?;
    assert!(
        !agents["missing"]
            .as_array()
            .is_some_and(|missing| missing.iter().any(|harness| harness == "codex")),
        "{agents}"
    );
    rig.close()?;
    Ok(())
}

/// The reviews' case: the human's working Codex is where it installs itself, which
/// the Finder's PATH lacks, and Homebrew's folder holds an older one, which writes
/// down that it ran and fails. The window opens on the human's.
#[test]
fn a_task_for_a_codex_worker_runs_the_codex_in_its_home_and_not_an_older_one_of_the_systems_folder(
) -> Outcome {
    let rig = Rig::start(config_with_codex().as_the_finder_starts_it())?;
    let system = named(&rig, "CONSENSFLOW_TOOL_FOLDERS").join("codex");
    let own = named(&rig, "HOME").join(".codex").join("bin").join("codex");
    files::write_executable(&own, files::read_string(&system)?)?;
    let older_ran = named(&rig, "HOME").join("older-codex-ran");
    files::write_executable(
        &system,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 1\n",
            older_ran.display()
        ),
    )?;
    a_task_reaches_the_codex_worker(&rig)?;
    assert!(
        a_window_ran(&rig, &own),
        "no window ran {}: {:?}",
        own.display(),
        rig.open_frames()
    );
    assert!(
        !older_ran.exists(),
        "the older Codex ran: {}",
        files::read_string(&older_ran).unwrap_or_default()
    );
    rig.close()?;
    Ok(())
}

/// What a window runs of Homebrew's, here a `node` only in the system's folder that
/// writes down where it was found: a command the chief's turn runs by a shell, as an
/// agent runs one, finds it on the window's PATH. The file is there before a word is
/// in it, as the shell's `>` makes it before the `printf` writes (and the stand-in
/// is slow to write, so that the case is always read at that moment): the case waits
/// for what is written, and not for the file.
#[test]
fn a_window_of_a_daemon_on_the_finders_path_finds_a_program_in_the_systems_folder() -> Outcome {
    let rig = Rig::start(config().as_the_finder_starts_it())?;
    let node = named(&rig, "CONSENSFLOW_TOOL_FOLDERS").join("node");
    let found = named(&rig, "HOME").join("node-found");
    // The shell makes the file when it opens it for the redirect and writes it after: a
    // moment is made to last between the two, as a loaded machine makes one.
    files::write_executable(
        &node,
        format!(
            "#!/bin/sh\n: > '{found}'\n/bin/sleep 1\nprintf '%s\\n' \"$0\" >> '{found}'\n",
            found = found.display()
        ),
    )?;
    let project = Project::open(&rig, "chief", json!({}))?;
    rig.tell(project.id(), "RUN node --version")?;
    // The line the `node` writes, whole: the file is there from the moment the shell opens it.
    rig.wait_for("the chief's window to find node", secs(30), || {
        Ok(!files::read_finished_lines(&found)
            .unwrap_or_default()
            .is_empty())
    })?;
    assert_eq!(
        files::read_string(&found)?.trim_end(),
        node.to_string_lossy()
    );
    rig.close()?;
    Ok(())
}
