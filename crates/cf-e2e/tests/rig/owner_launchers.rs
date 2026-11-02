//! The owner's launchers of the harnesses, held to what a live run may do to them without
//! spending a token (no harness is started here but a stand-in for Claude Code's updater): a
//! live run keeps the person's `HOME` for its windows (their project folders are below it), so
//! a harness that installs itself writes the person's `~/.local/bin` unless it is told not to,
//! and the kit has to say it did.
//!
//! The case of T-1082 (the alpha.1 Mac release check, T-1068): a live approval run with Muse in
//! a seat gave every window a throwaway `XDG_DATA_HOME`, and ten seconds after its REPL
//! mounted, Claude Code's native updater found its versions folder empty, installed the
//! newest version there (2.1.316, the version the owner already ran) and re-pointed the
//! owner's `~/.local/bin/claude` at it, a link into the folder the run removed when it ended.
//! Their `claude` command was a dangling link. Nothing the kit hashed was touched: it held the
//! owner's login, settings and projects, and not their launchers.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use cf_e2e::live::{self, Harness, Installed};
use cf_e2e::process::Run;
use cf_e2e::rig::Rig;

use crate::{Outcome, FAKE_AGENT};

type Made<T> = Result<T, Box<dyn std::error::Error>>;

/// The version the pretend owner runs.
const OWNERS: &str = "2.1.316";

/// A pretend owner: a home with Claude Code installed as its native installer does
/// (`~/.local/bin/claude` is a link into `~/.local/share/claude/versions/<version>`) and
/// Devin's (`~/.local/bin/devin` through `_versions/current` to a version's folder).
struct Owner {
    home: tempfile::TempDir,
}

impl Owner {
    fn new() -> Made<Self> {
        let owner = Self {
            home: tempfile::tempdir()?,
        };
        owner.executable(
            &format!(".local/share/claude/versions/{OWNERS}"),
            "claude-code-binary",
        )?;
        owner.link(
            ".local/bin/claude",
            &owner.path(&format!(".local/share/claude/versions/{OWNERS}")),
        )?;
        owner.executable(
            ".local/share/devin/cli/_versions/3000.11.1/bin/devin",
            "devin",
        )?;
        owner.link(
            ".local/share/devin/cli/_versions/current",
            Path::new("3000.11.1"),
        )?;
        owner.link(
            ".local/bin/devin",
            &owner.path(".local/share/devin/cli/_versions/current/bin/devin"),
        )?;
        Ok(owner)
    }

    fn path(&self, below: &str) -> PathBuf {
        self.home.path().join(below)
    }

    fn executable(&self, below: &str, text: &str) -> Made<()> {
        let file = self.path(below);
        fs::create_dir_all(file.parent().ok_or("a file has a folder")?)?;
        fs::write(&file, text)?;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755))?;
        Ok(())
    }

    /// A link at `below` to `to`, made over one that is there as an installer does: a link
    /// of another name, then a rename over it.
    fn link(&self, below: &str, to: &Path) -> Made<()> {
        let at = self.path(below);
        fs::create_dir_all(at.parent().ok_or("a link has a folder")?)?;
        let staged = at.with_extension(format!("tmp-{}", std::process::id()));
        symlink(to, &staged)?;
        fs::rename(&staged, &at)?;
        Ok(())
    }

    fn claude(&self) -> Installed {
        Installed::at(
            Harness::Claude,
            self.path(".local/bin/claude"),
            self.home.path(),
        )
    }

    fn devin(&self) -> Installed {
        Installed::at(
            Harness::Devin,
            self.path(".local/bin/devin"),
            self.home.path(),
        )
    }

    /// The target of the link at `below`, as it reads.
    fn points(&self, below: &str) -> Made<PathBuf> {
        Ok(fs::read_link(self.path(below))?)
    }
}

/// What Claude Code's native updater does ten seconds after its REPL mounts, as the
/// binary's own log says it (2.1.316, `Atomically updated symlink`): unless an updater
/// switch is set, it installs the newest version under `$XDG_DATA_HOME/claude/versions` and
/// re-points `$HOME/.local/bin/claude` at it, which it takes for its own because the link
/// is a symlink into a folder of versions. The newest is the version the owner runs, so
/// the bytes are the same.
const UPDATER: &str = r#"#!/bin/sh
if [ -n "$DISABLE_AUTOUPDATER" ] || [ -n "$DISABLE_UPDATES" ]; then exit 0; fi
data="${XDG_DATA_HOME:-$HOME/.local/share}/claude/versions"
mkdir -p "$data" "$HOME/.local/bin"
cp "$HOME/.local/bin/claude" "$data/NEWEST"
ln -s "$data/NEWEST" "$HOME/.local/bin/claude.tmp.$$"
mv -f "$HOME/.local/bin/claude.tmp.$$" "$HOME/.local/bin/claude"
"#;

/// The updater run as a window of `rig` would run it: with the variables the rig gives its
/// windows, and nothing else.
fn run_updater(rig: &Rig, script: &Path) -> Made<()> {
    let names = [
        "HOME",
        "XDG_DATA_HOME",
        "DISABLE_AUTOUPDATER",
        "DISABLE_UPDATES",
        "PATH",
    ];
    let vars = names
        .iter()
        .filter_map(|name| Some((*name, rig.var(name)?.to_owned())));
    let ran = Run::new("/bin/sh").arg(script).vars(vars).run()?;
    assert_eq!(ran.code, Some(0), "{}", ran.output());
    Ok(())
}

#[test]
fn a_run_that_leaves_the_owners_launchers_alone_ends_clean() -> Outcome {
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&[owner.claude(), owner.devin()], FAKE_AGENT))?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_run_that_re_points_the_owners_launcher_at_a_copy_of_the_same_binary_fails_the_rig_that_ends(
) -> Outcome {
    let owner = Owner::new()?;
    let was = owner.points(".local/bin/claude")?;
    let rig = Rig::start(live::rig(&[owner.claude()], FAKE_AGENT))?;
    // The run's updater: the same version, installed in the rig's folder, and the link taken
    // there. Not one byte of the binary differs from the owner's.
    let elsewhere = tempfile::tempdir()?;
    let copy = elsewhere.path().join("claude/versions").join(OWNERS);
    fs::create_dir_all(copy.parent().ok_or("a copy has a folder")?)?;
    fs::copy(&was, &copy)?;
    owner.link(".local/bin/claude", &copy)?;
    let failed = rig.close().unwrap_err().to_string();
    assert!(
        failed.contains("~/.local/bin/claude now points at"),
        "{failed}"
    );
    // The way back is in the words: the exact target the link had.
    assert!(
        failed.contains(&format!("ln -sfn {} ~/.local/bin/claude", was.display())),
        "{failed}"
    );
    Ok(())
}

#[test]
fn a_run_that_installs_a_version_among_the_owners_fails_the_rig_that_ends_and_names_it() -> Outcome
{
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&[owner.claude()], FAKE_AGENT))?;
    owner.executable(".local/share/claude/versions/2.1.999", "a newer binary")?;
    let failed = rig.close().unwrap_err().to_string();
    assert!(
        failed.contains("~/.local/share/claude/versions gained 2.1.999"),
        "{failed}"
    );
    Ok(())
}

#[test]
fn a_run_that_overwrites_the_binary_the_owners_launcher_points_at_fails_the_rig_that_ends(
) -> Outcome {
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&[owner.claude()], FAKE_AGENT))?;
    owner.executable(
        &format!(".local/share/claude/versions/{OWNERS}"),
        "a different binary",
    )?;
    let failed = rig.close().unwrap_err().to_string();
    assert!(
        failed.contains(&format!("versions/{OWNERS} is not as it was")),
        "{failed}"
    );
    Ok(())
}

#[test]
fn a_run_that_moves_devins_current_version_fails_the_rig_that_ends_though_the_launcher_is_the_same_link(
) -> Outcome {
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&[owner.devin()], FAKE_AGENT))?;
    owner.executable(
        ".local/share/devin/cli/_versions/3000.11.2/bin/devin",
        "devin",
    )?;
    owner.link(
        ".local/share/devin/cli/_versions/current",
        Path::new("3000.11.2"),
    )?;
    let failed = rig.close().unwrap_err().to_string();
    // `~/.local/bin/devin` still reads the link it read; where it ends is another file.
    assert!(
        failed.contains("~/.local/bin/devin now ends at"),
        "{failed}"
    );
    Ok(())
}

#[test]
fn a_case_that_gives_the_rig_a_claude_folder_of_its_own_is_held_to_the_launcher_all_the_same(
) -> Outcome {
    let owner = Owner::new()?;
    let folder = tempfile::tempdir()?;
    let rig = Rig::start(live::rig_with_claude_config(
        &[owner.claude()],
        FAKE_AGENT,
        folder.path(),
    ))?;
    owner.link(".local/bin/claude", Path::new("/nowhere/claude"))?;
    let failed = rig.close().unwrap_err().to_string();
    assert!(
        failed.contains("~/.local/bin/claude now points at /nowhere/claude"),
        "{failed}"
    );
    Ok(())
}

#[test]
fn the_windows_of_a_rig_are_told_to_update_no_harness() -> Outcome {
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&[owner.claude()], FAKE_AGENT))?;
    // Claude Code's two (the binary reads both, 2.1.314 and 2.1.316) and Muse's launcher's.
    for name in [
        "DISABLE_AUTOUPDATER",
        "DISABLE_UPDATES",
        "MUSE_NO_AUTO_UPDATE",
    ] {
        assert_eq!(rig.var(name), Some("1"), "{name}");
    }
    rig.close()?;
    Ok(())
}

#[test]
fn a_claude_that_updates_itself_unless_it_is_told_not_to_leaves_the_launcher_alone_in_the_rig_and_is_caught_in_one_that_does_not_tell_it(
) -> Outcome {
    let owner = Owner::new()?;
    let script = owner.path("updater.sh");
    fs::write(&script, UPDATER)?;
    let elsewhere = tempfile::tempdir()?;
    // The XDG folder a seat for Muse gives every window of a rig: the home is the owner's, and
    // the data folder is the run's.
    let data = elsewhere.path().join("data");
    fs::create_dir_all(&data)?;
    let data = data.to_string_lossy().into_owned();
    let was = owner.points(".local/bin/claude")?;

    let told = Rig::start(live::rig(&[owner.claude()], FAKE_AGENT).var("XDG_DATA_HOME", &data))?;
    run_updater(&told, &script)?;
    assert_eq!(owner.points(".local/bin/claude")?, was);
    told.close()?;

    // The control: a rig whose case took the switches out, which is what the kit did
    // before T-1082. The launcher moves, and the rig that ends says so.
    let untold = Rig::start(
        live::rig(&[owner.claude()], FAKE_AGENT)
            .var("XDG_DATA_HOME", &data)
            .unset("DISABLE_AUTOUPDATER")
            .unset("DISABLE_UPDATES"),
    )?;
    run_updater(&untold, &script)?;
    assert_ne!(owner.points(".local/bin/claude")?, was);
    let failed = untold.close().unwrap_err().to_string();
    assert!(
        failed.contains("~/.local/bin/claude now points at"),
        "{failed}"
    );
    Ok(())
}
