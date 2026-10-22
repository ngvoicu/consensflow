//! Where a rig's home is, and the homes a killed run left. A rig's home is in the
//! machine's temp folder, but for a rig built for the real harnesses on Unix
//! (`live::rig`), whose windows write what a real harness writes with the person's login
//! there: that one is under the checkout's build folder, where `cargo clean` takes it
//! and the machine's temp folder does not hold it for good. On Windows it stays in the
//! temp folder: a checkout's deep path and a harness's own under it come near MAX_PATH.
//! (A Unix socket is no reason: the Codex session's falls back to the temp folder's when
//! its home's is too long.)
//!
//! A run that was killed leaves its rigs' homes, and so does `CONSENSFLOW_LIVE_KEEP`. A
//! rig that starts sweeps the ones in the folder it makes its own in that are older than
//! a day, and that no process works in, without reading what is in them.

use std::fs;

use cf_e2e::rig::Rig;
use cf_e2e::{checkout, live};

use crate::{config, Outcome};

/// Where the homes of rigs built for the real harnesses are, under the checkout.
const LIVE_HOMES: &str = "app/src-tauri/target/live-homes";

#[test]
fn a_live_rigs_home_is_under_the_checkouts_build_folder_on_unix_and_in_the_temp_folder_on_windows(
) -> Outcome {
    let rig = Rig::start(live::rig(&[], crate::FAKE_AGENT))?;
    let workspace = rig.workspace().to_path_buf();
    rig.close()?;

    let folder = if cfg!(windows) {
        std::env::temp_dir()
    } else {
        checkout::path(LIVE_HOMES)
    };
    assert!(
        workspace.starts_with(&folder),
        "{} is not under {}",
        workspace.display(),
        folder.display()
    );
    // And a rig that is not built for the real harnesses has its home in the temp folder.
    let plain = Rig::start(config())?;
    let workspace = plain.workspace().to_path_buf();
    plain.close()?;
    assert!(
        workspace.starts_with(std::env::temp_dir()),
        "{}",
        workspace.display()
    );
    Ok(())
}

#[test]
fn a_rig_is_given_its_home_in_the_folder_its_case_names() -> Outcome {
    let folder = tempfile::tempdir()?;

    let rig = Rig::start(config().homes_in(folder.path()))?;

    let workspace = rig.workspace().to_path_buf();
    assert!(
        workspace.starts_with(folder.path()),
        "{} is not under {}",
        workspace.display(),
        folder.path().display()
    );
    rig.close()?;
    assert!(
        fs::read_dir(folder.path())?.next().is_none(),
        "the home was not taken away when the rig closed"
    );
    Ok(())
}

#[cfg(unix)]
mod sweeping {
    use std::fs::File;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use cf_e2e::process::Run;

    use super::*;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    /// `path`, last changed `ago` ago, by the clock the sweep reads its age against.
    #[allow(clippy::disallowed_methods)]
    fn aged(path: &Path, ago: Duration) -> std::io::Result<()> {
        File::open(path)?.set_modified(SystemTime::now() - ago)
    }

    /// A home called `name` in `folder` with a workspace and a note in it, last changed
    /// `ago` ago.
    fn home(folder: &Path, name: &str, ago: Duration) -> std::io::Result<std::path::PathBuf> {
        let home = folder.join(name);
        fs::create_dir_all(home.join("workspace"))?;
        fs::write(home.join("workspace").join("note"), "kept or not")?;
        aged(&home, ago)?;
        Ok(home)
    }

    #[test]
    fn a_rig_that_starts_sweeps_the_homes_older_than_a_day_that_no_process_works_in_and_nothing_else(
    ) -> Outcome {
        let folder = tempfile::tempdir()?;
        let stale = home(folder.path(), "consensflow-integration-stale", DAY * 3)?;
        let worked_in = home(folder.path(), "consensflow-integration-worked", DAY * 3)?;
        let named = home(folder.path(), "consensflow-integration-named", DAY * 3)?;
        let young = home(folder.path(), "consensflow-integration-young", DAY / 2)?;
        let other = home(folder.path(), "kept-other-folder", DAY * 3)?;
        // A report is a file, not a home; a link is not followed.
        let report = folder.path().join("consensflow-integration-stale-left.txt");
        fs::write(&report, "a report")?;
        aged(&report, DAY * 3)?;
        let elsewhere = home(folder.path(), "elsewhere", DAY * 3)?;
        let link = folder.path().join("consensflow-integration-link");
        std::os::unix::fs::symlink(&elsewhere, &link)?;

        // A process working in one, and one that has the other's name in its command line.
        let mut sleeper = Run::new("sleep")
            .arg("120")
            .cwd(worked_in.join("workspace"))
            .spawn()?;
        let mut namer = Run::new("sh")
            .args(["-c", "read line", "named-home"])
            .arg(&named)
            .spawn()?;

        let rig = Rig::start(config().homes_in(folder.path()));
        sleeper.kill();
        namer.kill();
        let rig = rig?;
        let own = rig.workspace().to_path_buf();
        rig.close()?;

        assert!(
            !stale.exists(),
            "a home older than a day, worked in by none, is still there"
        );
        assert!(
            worked_in.join("workspace").join("note").is_file(),
            "a home a process works in"
        );
        assert!(
            named.join("workspace").join("note").is_file(),
            "a home a process names"
        );
        assert!(
            young.join("workspace").join("note").is_file(),
            "a home under a day old"
        );
        assert!(
            other.join("workspace").join("note").is_file(),
            "a folder that is no home"
        );
        assert!(report.is_file(), "a report");
        assert!(
            link.is_symlink() && elsewhere.join("workspace").join("note").is_file(),
            "a link"
        );
        assert!(own.starts_with(folder.path()));
        Ok(())
    }
}
