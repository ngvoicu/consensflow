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

use std::fs;
use std::path::PathBuf;

use cf_e2e::live::{self, Harness, Installed};
#[cfg(target_os = "macos")]
use cf_e2e::process::{own_var, Run};
use cf_e2e::rig::Rig;

use crate::{Outcome, FAKE_AGENT};

// The cases that make links and run a shell updater are the Unix machines': `links`. These hold
// on every machine, with a launcher that is a file, as Windows installs one: the guard has no
// link to read there, and where a file ends and its bytes are all it has.
#[path = "owner_launchers/links.rs"]
#[cfg(unix)]
mod links;

/// A config of the kit's that keeps its findings for the report (an eval's): a folder of the case's own,
/// and nothing to say when the run ends. The guard leaves the Keychain to such a config, so a rig
/// that seats a Claude Code on it does not depend on what other processes do to the machine's Keychain.
#[derive(Debug)]
pub(crate) struct Folder(pub(crate) PathBuf);

impl cf_e2e::rig::ClaudeConfig for Folder {
    fn folder(&self) -> cf_e2e::Result<PathBuf> {
        Ok(self.0.clone())
    }

    fn described(&self) -> String {
        "a folder of the case's own".to_owned()
    }

    fn finish(&self) -> cf_e2e::Result {
        Ok(())
    }

    fn keeps_its_findings(&self) -> bool {
        true
    }
}

/// A lock between the cases that make an item in the machine's Keychain (exclusive) and the cases whose
/// guard lists it (shared): nextest runs each case in a process of its own, and a rig that began before
/// an item was made would end for it. It is a file in the temporary folder, held until this is dropped.
#[cfg(unix)]
pub(crate) struct KeychainLock(std::fs::File);

#[cfg(unix)]
impl KeychainLock {
    pub(crate) fn take(exclusive: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(std::env::temp_dir().join("cf-e2e-keychain-cases.lock"))?;
        if exclusive {
            file.lock()?;
        } else {
            file.lock_shared()?;
        }
        Ok(Self(file))
    }
}

#[cfg(unix)]
impl Drop for KeychainLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// A pretend owner whose Claude Code launcher is a file (`%USERPROFILE%\.local\bin\claude.exe`).
struct Owner {
    home: tempfile::TempDir,
}

impl Owner {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let owner = Self {
            home: tempfile::tempdir()?,
        };
        fs::create_dir_all(owner.kit())?;
        let launcher = owner.launcher();
        fs::create_dir_all(launcher.parent().ok_or("a launcher has a folder")?)?;
        fs::write(&launcher, "claude-code-binary")?;
        Ok(owner)
    }

    fn launcher(&self) -> PathBuf {
        let name = if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        };
        self.home.path().join(".local").join("bin").join(name)
    }

    /// The owner's Claude Code on a config of the kit's that keeps its findings: the guard does not
    /// list the Keychain for it, so what other processes do to the machine's does not fail a case
    /// about the launchers.
    fn claude(&self) -> Installed {
        self.plain_claude()
            .with_claude_config(std::sync::Arc::new(Folder(self.kit())))
    }

    /// The config folder of the kit's, below the owner's home.
    fn kit(&self) -> PathBuf {
        self.home.path().join("kit-claude")
    }

    /// The owner's Claude Code with no config of the kit's: the guard lists the Keychain for it.
    fn plain_claude(&self) -> Installed {
        Installed::at(Harness::Claude, self.launcher(), self.home.path())
    }
}

#[test]
fn a_run_that_leaves_a_launcher_that_is_a_file_alone_ends_clean() -> Outcome {
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&[owner.claude()], FAKE_AGENT))?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_run_that_overwrites_a_launcher_that_is_a_file_fails_the_rig_that_ends_and_names_it() -> Outcome
{
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&[owner.claude()], FAKE_AGENT))?;
    // An updater that installed itself over the owner's copy: no link moved, the bytes differ.
    fs::write(owner.launcher(), "a newer claude-code-binary")?;
    let failed = rig.close().unwrap_err().to_string();
    assert!(
        failed.contains("THE RUN CHANGED THE OWNER'S CLAUDE LAUNCHER"),
        "{failed}"
    );
    assert!(failed.contains("is not as it was"), "{failed}");
    assert!(
        failed.contains(
            &owner
                .launcher()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        ),
        "{failed}"
    );
    // The way back for a link is a command; a file has none, and the kit does not write it.
    assert!(!failed.contains("ln -sfn"), "{failed}");
    assert_eq!(
        fs::read_to_string(owner.launcher())?,
        "a newer claude-code-binary"
    );
    Ok(())
}

#[test]
fn the_windows_of_a_rig_are_told_to_update_no_harness() -> Outcome {
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&[owner.claude()], FAKE_AGENT))?;
    // Claude Code's two (the binary reads both: 2.1.263, 2.1.314 and 2.1.316) and Muse's launcher's.
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

// The owner's Keychain (T-1102): the guard of Claude Code lists the names of its items around the
// run, and the rig that ends fails for one gained. On a Mac only, which is the machine with the
// Keychain; the case writes one item of its own, of the name Claude Code would give a throwaway
// folder, and deletes that item by that name.

/// A Keychain item of the name Claude Code gives a config folder, made here and deleted by that
/// name when this is dropped, however the case ends.
#[cfg(target_os = "macos")]
struct Item {
    name: String,
    user: String,
}

#[cfg(target_os = "macos")]
impl Item {
    fn make(folder: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let name = live::keychain::item_of(folder);
        let user = own_var("USER").ok_or("no USER")?;
        let made = Run::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-s",
                &name,
                "-a",
                &user,
                "-w",
                "a made-up value",
            ])
            .inheriting_env()
            .run()?;
        if made.code != Some(0) {
            return Err(format!("security could not make the item {name}: {:?}", made.code).into());
        }
        Ok(Self { name, user })
    }
}

#[cfg(target_os = "macos")]
impl Drop for Item {
    fn drop(&mut self) {
        let _ = Run::new("/usr/bin/security")
            .args([
                "delete-generic-password",
                "-s",
                &self.name,
                "-a",
                &self.user,
            ])
            .inheriting_env()
            .run();
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_run_whose_claude_code_left_an_item_in_the_keychain_fails_the_rig_that_ends_and_names_it(
) -> Outcome {
    let _lock = KeychainLock::take(true)?;
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&[owner.plain_claude()], FAKE_AGENT))?;
    // What a Claude Code writes in the Keychain for a throwaway config folder, after the rig began.
    let folder = tempfile::tempdir()?;
    let item = Item::make(folder.path())?;
    let failed = rig.close().unwrap_err().to_string();
    assert!(
        failed.contains("THE RUN LEFT AN ITEM IN THE OWNER'S KEYCHAIN"),
        "{failed}"
    );
    assert!(failed.contains(&item.name), "{failed}");
    assert!(
        failed.contains(&format!(
            "security delete-generic-password -s \"{}\"",
            item.name
        )),
        "{failed}"
    );
    // The launcher is as it was: only the Keychain is said.
    assert!(!failed.contains("LAUNCHER"), "{failed}");
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn a_claude_whose_config_keeps_its_findings_is_not_listed_by_the_guard_and_its_rig_ignores_another_processs_item(
) -> Outcome {
    // An eval's config names the item of its own folder, and keeps what it finds for its report:
    // a rig that failed as well, for an item some other process made (the Keychain is the whole
    // machine's), would end an eval with `closed?` and cost it the report (T-1044, T-1102).
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&[owner.claude()], FAKE_AGENT))?;
    // An item made for a folder that is none of the run's.
    let other = tempfile::tempdir()?;
    let _item = Item::make(other.path())?;
    rig.close()?;
    Ok(())
}
