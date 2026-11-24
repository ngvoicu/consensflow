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
use cf_e2e::rig::Rig;

use crate::{Outcome, FAKE_AGENT};

// The cases that make links and run a shell updater are the Unix machines': `links`. These hold
// on every machine, with a launcher that is a file, as Windows installs one: the guard has no
// link to read there, and where a file ends and its bytes are all it has.
#[path = "owner_launchers/links.rs"]
#[cfg(unix)]
mod links;

/// A config folder of the case's own, with nothing to say when the run ends. The launcher guard
/// is seated even when the case gives the rig its own config.
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

/// A pretend owner whose Claude Code launcher is a file (`%USERPROFILE%\.local\bin\claude.exe`).
struct Owner {
    home: cf_e2e::door::Made,
}

impl Owner {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let owner = Self {
            home: cf_e2e::door::made("t-")?,
        };
        cf_e2e::door::make_dir(owner.kit())?;
        let launcher = owner.launcher();
        cf_e2e::door::make_dir(launcher.parent().ok_or("a launcher has a folder")?)?;
        cf_e2e::door::write(&launcher, "claude-code-binary")?;
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
    cf_e2e::door::write(owner.launcher(), "a newer claude-code-binary")?;
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

/// A Keychain item of the name Claude Code gives a config folder, made here by the control (an
/// item this run issued, and no other) and deleted by that name when this is dropped, however the
/// case ends.
#[cfg(target_os = "macos")]
struct Item {
    own: Option<live::keychain::OwnItem>,
    name: String,
    control: live::keychain::Control,
}

#[cfg(target_os = "macos")]
impl Item {
    fn make(folder: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let control = live::keychain::Control::required_for("owner_launchers::Item::make")?;
        let own = control.make_item(folder)?;
        Ok(Self {
            name: own.name().to_owned(),
            own: Some(own),
            control,
        })
    }
}

#[cfg(target_os = "macos")]
impl Drop for Item {
    fn drop(&mut self) {
        if let Some(own) = self.own.take() {
            let _ = own.forget(&self.control);
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "real Keychain test: disposable GitHub hosted macOS runner only"]
fn a_run_whose_claude_code_left_an_item_in_the_keychain_fails_the_rig_that_ends_and_names_it(
) -> Outcome {
    require_hosted()?;
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&[owner.plain_claude()], FAKE_AGENT))?;
    // What a Claude Code writes in the Keychain for a throwaway config folder, after the rig began.
    let folder = cf_e2e::door::made("t-")?;
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
#[ignore = "real Keychain test: disposable GitHub hosted macOS runner only"]
fn a_claude_whose_config_keeps_its_findings_is_not_listed_by_the_guard_and_its_rig_ignores_another_processs_item(
) -> Outcome {
    require_hosted()?;
    // A config that records its own findings names the item of its own folder. The guard
    // leaves the shared Keychain to it, so another process's item cannot fail this rig.
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&[owner.claude()], FAKE_AGENT))?;
    // An item made for a folder that is none of the run's.
    let other = cf_e2e::door::made("t-")?;
    let _item = Item::make(other.path())?;
    rig.close()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn require_hosted() -> Outcome {
    if !live::keychain::hosted_test() {
        return Err("real Keychain tests require the disposable GitHub hosted Mac job".into());
    }
    Ok(())
}
