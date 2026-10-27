//! A `cf` that a build took away left a window none: "no such file" when a stand-in ran it
//! (T-514, a worker's crate run of 2026-10-25, once; reproduced by T-581, 2026-10-26).
//!
//! A window's `PATH` starts with the folder of the daemon's program, where the `cf` of the
//! install is. The rig's daemon ran the release build, and a `cargo build` of `cf` in the
//! checkout (the worker's other run, the plants', a test process that builds for itself)
//! takes `target/release/cf` away and puts it back, for a moment, though nothing changed:
//! a stand-in's agent that ran `cf` then found none, its task was never added, and the case
//! timed out. The release `cf` that the worker found afterwards was there, and as old as its
//! build, for the file linked back keeps its modification time.
//!
//! What should happen: the windows of a rig find the `cf` the rig runs, whatever is done to
//! the build it was made from. The case takes the build away for good once the rig is up,
//! as a build does for a moment, and the chief's `cf task add` has to find `cf` all the same.

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::{cf, files};
use serde_json::json;

use crate::a_workers_still_going_was_given_as_its_result::tier;
use crate::{config, secs, Outcome};

#[test]
fn a_chief_runs_cf_in_its_window_though_the_build_it_was_made_from_is_gone() -> Outcome {
    let build = tempfile::tempdir()?;
    let release = build.path().join("release");
    files::make_dir(&release)?;
    let built = release.join("cf");
    std::fs::copy(cf::binary()?, &built)?;
    let rig = Rig::start(config().cf_from(&built))?;
    // The build is taken from under the rig, as the link of a new one takes it for a moment.
    std::fs::remove_file(&built)?;
    assert!(!built.exists());

    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    // The chief runs `cf task add` as an agent runs it, by the `cf` first on its `PATH`.
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: WORKER_OK",
            tier(&added)
        ),
    )?;
    rig.wait_for("the chief's cf to add the task", secs(60), || {
        Ok(project.lane("worker")?["tasks"][0]["state"] == "done")
    })?;
    rig.close()?;
    Ok(())
}
