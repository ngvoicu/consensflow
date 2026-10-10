//! What the live tests stand on, held to what it says: a rig for the machine's
//! Codex gives its windows that Codex's login and its command, after the
//! stand-in Claude and before the system's; a rig told to keep its home leaves
//! it, and one that is not takes it away; and the windows that ended are the
//! rig's to tell, with how they ended and what their screens showed.

use std::path::Path;

use cf_e2e::live::Codex;
use cf_e2e::rig::{Project, Rig};
use serde_json::json;

use crate::{rig, secs, Outcome, FAKE_AGENT};

/// What a rig for `codex` gives its windows: the login they read, and the
/// folders of their `PATH`, in order.
fn windows_of(codex: &Codex) -> Result<(String, Vec<String>), Box<dyn std::error::Error>> {
    let rig = Rig::start(codex.rig(FAKE_AGENT))?;
    let login = rig.var("CODEX_HOME").unwrap_or_default().to_owned();
    let path = rig.var("PATH").unwrap_or_default().to_owned();
    rig.close()?;
    let delimiter = if cfg!(windows) { ';' } else { ':' };
    Ok((login, path.split(delimiter).map(str::to_owned).collect()))
}

#[test]
fn a_rig_for_a_codex_gives_its_windows_its_login_and_the_folders_it_needs_after_the_stand_in_claude(
) -> Outcome {
    let (installed, node, login) = (
        tempfile::tempdir()?,
        tempfile::tempdir()?,
        tempfile::tempdir()?,
    );
    let command = installed
        .path()
        .join(if cfg!(windows) { "codex.cmd" } else { "codex" });
    let text = |folder: &tempfile::TempDir| folder.path().to_string_lossy().into_owned();

    // The command's folder follows the stand-in's, and the system's follow it.
    let codex = Codex::at(&command, login.path());
    let (home, plain) = windows_of(&codex)?;
    assert_eq!(home, text(&login));
    assert!(plain[0].ends_with("fake-bin"), "{plain:?}");
    assert_eq!(plain[1], text(&installed), "{plain:?}");
    assert!(plain.len() > 2, "the system's folders follow: {plain:?}");

    // Node's, where it is another folder, comes next. (Each rig has a stand-in
    // folder of its own, which is the first.)
    let (_, with_node) = windows_of(&codex.clone().with_node(node.path()))?;
    assert!(with_node[0].ends_with("fake-bin"), "{with_node:?}");
    assert_eq!(with_node[1..3], [text(&installed), text(&node)]);
    assert_eq!(with_node[3..], plain[2..]);

    // Where it is the command's own folder, it is not there twice.
    let (_, together) = windows_of(&codex.with_node(installed.path()))?;
    assert_eq!(together[1..], plain[1..]);
    Ok(())
}

#[test]
fn a_rig_keeps_its_home_when_told_to_and_takes_it_away_when_not() -> Outcome {
    let root_of = |rig: &Rig| rig.home().parent().map(Path::to_path_buf);

    let plain = rig()?;
    let gone = root_of(&plain).ok_or("a rig's home is in a folder")?;
    assert!(gone.join("workspace").is_dir());
    plain.close()?;
    assert!(!gone.exists(), "a rig takes its home away");

    let mut kept = rig()?;
    let saved = kept.keep_home();
    assert_eq!(Some(saved.clone()), root_of(&kept));
    kept.close()?;
    assert!(
        saved.join("workspace").is_dir() && saved.join("consensflow").is_dir(),
        "a rig told to keep its home leaves it whole"
    );
    std::fs::remove_dir_all(&saved)?;
    Ok(())
}

#[test]
fn a_window_that_ended_is_among_the_rigs_exits_with_how_it_ended_and_what_its_screen_showed(
) -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    assert!(rig.exits().is_empty(), "no window has ended yet");
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: ENDED",
            added["member"]["tier"].as_str().unwrap_or_default()
        ),
    )?;
    rig.wait_for("the worker's task to be done", secs(30), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let prefix = format!("p{}-worker-", project.id());
    let ended = || {
        rig.exits().into_iter().find(|exit| {
            exit["id"]
                .as_str()
                .is_some_and(|id| id.starts_with(&prefix))
        })
    };
    rig.wait_for("the worker's window to end", secs(30), || {
        Ok(ended().is_some())
    })?;
    let exit = ended().ok_or("the worker's window ended, and is not among the exits")?;
    assert!(exit["generation"].is_number(), "{exit}");
    assert!(
        exit["tail"]
            .as_array()
            .is_some_and(|tail| tail.iter().any(|line| line
                .as_str()
                .is_some_and(|line| line.contains("fake agent started on")))),
        "its screen, as it ended, is what the exit says: {exit}"
    );
    // The chief's window is still there: it has not ended.
    assert!(rig.exits().iter().all(|exit| exit["id"] != "p1-chief"));
    rig.close()?;
    Ok(())
}
