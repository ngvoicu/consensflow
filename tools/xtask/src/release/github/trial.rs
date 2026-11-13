//! The hand run of the Release workflow tried before any tag.

use std::path::Path;

use super::super::{short, Error, Release, REPOSITORY};
use super::runs::{HeadOf, Left, Watched};

impl Release<'_> {
    /// A hand run of the Release workflow on `commit`, started and watched to its
    /// end: the Mac job builds, signs and smoke-tests the app as the tag's run
    /// will, the Windows job builds, and nothing is published (only the push of a
    /// tag publishes). What would fail the tag's run fails this one first, with no
    /// tag made: a tag whose run failed is left behind, and a tag is never moved or
    /// pushed again, so its version would be spent.
    ///
    /// The run is asked for with `notes` as its `notes` input — the file the tag
    /// will carry, which `gh` reads and sends (`-F notes=@…`): a hand run has no
    /// tag's message to read, so the trial reads the same notes the tag's run
    /// will. A run that gets none writes the commits since the previous release,
    /// cut under the feed's 64 KiB (alpha.2's trial failed on a range past it).
    ///
    /// A hand run is of a branch's head, not of a commit, and `main` is the only
    /// branch the `release` environment (which holds the signing keys) admits
    /// besides tags: `commit` must be main's head, which the caller has just seen.
    /// A run of another commit would not be the one watched, which is looked for by
    /// the commit. The run, once it ends green.
    pub(in crate::release) fn trial(&mut self, commit: &str, notes: &Path) -> Result<u64, Error> {
        let before = self.newest_run("release.yml", Some(commit), Some("workflow_dispatch"))?;
        let newest_before = self.newest_run("release.yml", None, Some("workflow_dispatch"))?;
        self.gh(&[
            "workflow",
            "run",
            "release.yml",
            "--repo",
            REPOSITORY,
            "--ref",
            "main",
            "-F",
            &format!("notes=@{}", notes.display()),
        ])?;
        self.say(&format!(
            "trial: a hand run of release.yml asked for, on main at {}, with the notes {}",
            short(commit),
            notes.display()
        ));
        self.watch(&Watched {
            label: "trial",
            workflow: "release.yml",
            commit,
            event: Some("workflow_dispatch"),
            before,
            left: Left::Trial,
            head: Some(HeadOf {
                branch: "main",
                newest_before,
            }),
        })
    }
}
