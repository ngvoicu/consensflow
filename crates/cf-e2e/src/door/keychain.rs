//! The Keychain's door (K3, `live-kit-isolation-doors.md` section 2): `/usr/bin/security`, the
//! Keychain's utility, is started here and nowhere else (a plain start refuses it,
//! [`super::spawn::Program::Keychain`]), with words fixed in this file.
//!
//! - **Two reads** for every run, with the machine's variables and the account's home by name:
//!   Claude Code's login item, for the seed of a run's login copy ([`login_item`]), and the names
//!   of the items, for the guard that compares them before and after ([`item_names`]).
//! - **The control**, which makes, reads and deletes an item of a folder of its own, takes only
//!   what it holds a token for. It runs where the account is a contributor's own, marked as a test
//!   machine (`CONSENSFLOW_TEST_MACHINE`), or on the hosted runner its test is made for
//!   ([`Control::required_for`]); and it acts on [`OwnItem`]s alone: an item named for a config
//!   folder this run made, that two successful listings (one before, one after) show this run
//!   gained ([`Listing::issue`]), or that [`Control::make_item`] made itself. The control takes
//!   no words of a caller's (T-2086, T-2002): `delete-generic-password` is for an item it
//!   issued, and a listing that failed issues none (it was read as an empty set before, and an
//!   item that was there already was then read as new, and deleted).

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use crate::live::keychain::{hosted_test, item_of, names, watching};
use crate::live::renew::TestMachine;
use crate::process::{own_var, Ran, Run};
use crate::{Error, Result};

/// The Keychain's utility, which no other start of the kit's starts.
const UTILITY: &str = "/usr/bin/security";

/// `/usr/bin/security` with `args`, the machine's variables and the account's home by name. A
/// stand-in cannot reach the real program: an accidental call fails naming its caller.
fn security<I, S>(caller: &str, args: I, limit: Duration) -> Result<Ran>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    #[cfg(test)]
    if let Some(result) = crate::live::keychain::fake::next() {
        return Ok(result);
    }
    assert!(watching(), "stand-in run attempted security from {caller}");
    let home = if hosted_test() {
        crate::standin_process::inherited_home().map(std::path::PathBuf::from)
    } else {
        crate::door::account::project_home()
    }
    .ok_or_else(|| {
        Error::Daemon(format!(
            "the Keychain is read for {caller} on the account's home, and this run has none"
        ))
    })?;
    Run::operation("keychain", UTILITY)
        .args(args)
        .var("HOME", home)
        .limit(limit)
        .run()
}

/// The first of the two reads: Claude Code's login item, of the account `user`, for the seed of a
/// run's login copy ([`crate::live::login`]).
pub fn login_item(user: &str, limit: Duration) -> Result<Ran> {
    security(
        "keychain::login_item",
        crate::live::login::keychain_args(user),
        limit,
    )
}

/// The second: the names of the Keychain's items, for the guard that compares them before and
/// after a run ([`crate::live::login::keychain_services`]).
pub fn item_names(limit: Duration) -> Result<Ran> {
    security("keychain::item_names", ["dump-keychain"], limit)
}

/// The token of the control's right to act: the account is a test machine's own, or this is the
/// hosted runner the control's test is made for.
#[derive(Debug, Clone)]
pub struct Control(());

impl Control {
    /// The control for `what`, where the account is a marked test machine's or the hosted
    /// runner's; refused, and why, anywhere else.
    pub fn required_for(what: &str) -> Result<Control> {
        #[cfg(test)]
        if crate::live::keychain::fake::active() {
            return Ok(Control(()));
        }
        if hosted_test() {
            return Ok(Control(()));
        }
        TestMachine::required_for(what).map(|_| Control(()))
    }

    /// Claude Code, once, on the account's own home (`HOME`: where its Keychain lookup is made)
    /// and a config folder this run made (`CLAUDE_CONFIG_DIR` among `vars`), against the case's own
    /// servers: the one start of the kit that gives a program the account's home, behind the
    /// control's token. The program must be Claude Code's (a file of the machine's harness
    /// resolution, by name), `vars` may not name a home of the account's, and the update switches
    /// every start has are added. Refused anywhere the control is.
    pub fn start_claude(
        &self,
        program: &Path,
        args: &[&str],
        vars: Vec<(String, String)>,
        cwd: &Path,
        limit: Duration,
    ) -> Result<Ran> {
        let _ = self;
        let config = |name: &str| name.eq_ignore_ascii_case("CLAUDE_CONFIG_DIR");
        for (name, value) in &vars {
            if crate::door::spawn::HOME_VARS
                .iter()
                .chain(&crate::door::spawn::PROFILE_HALVES)
                .any(|home| home.eq_ignore_ascii_case(name) && !config(home))
            {
                return Err(Error::Daemon(format!(
                    "the Keychain control gives {name} itself; a caller names none of the homes"
                )));
            }
            if config(name) && !crate::door::is_made_or_below(Path::new(value)) {
                return Err(Error::Daemon(format!(
                    "the Keychain control runs Claude Code on a config folder this run made, and {value} is none"
                )));
            }
        }
        if !vars.iter().any(|(name, _)| config(name)) {
            return Err(Error::Daemon(
                "the Keychain control runs Claude Code on a config folder this run made, and none is named".to_owned(),
            ));
        }
        // The program is Claude Code's, by the file it resolves to as any start is judged: the
        // control starts nothing else on the account's home.
        let words: Vec<String> = args.iter().map(|word| (*word).to_owned()).collect();
        let given: Vec<(std::ffi::OsString, std::ffi::OsString)> = vars
            .iter()
            .map(|(name, value)| (name.into(), value.into()))
            .collect();
        match crate::door::program_started(program, &words, &given) {
            Ok(crate::door::spawn::Program::Harness("claude")) => {}
            Ok(_) => {
                return Err(Error::Daemon(format!(
                    "the Keychain control runs Claude Code, and {} is another program",
                    program.display()
                )));
            }
            Err(why) => {
                return Err(Error::Daemon(format!(
                    "the Keychain control does not start {}: {why}",
                    program.display()
                )));
            }
        }
        let home = crate::live::machine::home()?;
        Run::operation("keychain-control", program)
            .args(args)
            .vars(vars)
            .var("HOME", home)
            .vars(
                crate::live::launcher::updates_off_of(crate::live::Harness::Claude)
                    .iter()
                    .copied(),
            )
            .cwd(cwd)
            .limit(limit)
            .run()
    }

    /// Makes the item Claude Code would make for `folder`, a config folder this run made, holding
    /// a made-up value, and says it is this run's ([`OwnItem`]): the hosted test's way to put an
    /// item in the Keychain for the guard to find. Refused where `folder` is no folder this run
    /// made, or an item of that name is there already (it is then no item of this run's).
    pub fn make_item(&self, folder: &Path) -> Result<OwnItem> {
        if !crate::door::is_made_or_below(folder) {
            return Err(Error::Daemon(format!(
                "{} is no folder this run made, so no item is made for it",
                folder.display()
            )));
        }
        let name = item_of(folder);
        let before = Listing::take()?;
        if before.0.contains(&name) {
            return Err(Error::Daemon(format!(
                "the Keychain holds an item `{name}` already: it is no item of this run's"
            )));
        }
        let user = account_user()?;
        let made = security(
            "keychain::make_item",
            [
                "add-generic-password",
                "-s",
                &name,
                "-a",
                &user,
                "-w",
                "a made-up value",
            ],
            Duration::from_secs(60),
        )?;
        if made.code != Some(0) {
            return Err(Error::Daemon(format!(
                "security could not make the item {name}: code {:?}, timed out {}, stderr: {}",
                made.code, made.timed_out, made.stderr
            )));
        }
        Ok(OwnItem { name, user })
    }
}

/// The user the item is made for: the account's own name.
fn account_user() -> Result<String> {
    own_var("USER")
        .filter(|user| !user.is_empty())
        .ok_or_else(|| {
            Error::Daemon("this run has no USER, which names the account of the item".to_owned())
        })
}

/// The names of the Keychain's items at one moment, **listed successfully**: a listing that
/// failed is an error, never an empty set (T-2002 finding, `tests/live_keychain.rs:315`).
#[derive(Debug, Clone)]
pub struct Listing(BTreeSet<String>);

impl Listing {
    /// The names now: an error where the Keychain cannot be listed here.
    pub fn take() -> Result<Listing> {
        names()
            .map(Listing)
            .ok_or_else(|| Error::Daemon("the login Keychain cannot be listed here".to_owned()))
    }

    /// Of a listing before and one after, the item Claude Code made for `folder`, a config folder
    /// this run made: an item of that name that `self` (before) did not hold and `after` does.
    /// None where it was there already, or is not there now: it is then no item this run
    /// gained, and none the control may read or delete.
    pub fn issue(&self, after: &Listing, folder: &Path) -> Option<OwnItem> {
        if !crate::door::is_made_or_below(folder) {
            return None;
        }
        let name = item_of(folder);
        (after.0.contains(&name) && !self.0.contains(&name)).then(|| OwnItem {
            name,
            user: account_user().ok().unwrap_or_default(),
        })
    }

    /// The names gained between `self` and `after`.
    pub fn gained(&self, after: &Listing) -> BTreeSet<String> {
        after.0.difference(&self.0).cloned().collect()
    }
}

/// An item of the login Keychain that this run made or gained, named for a config folder it made:
/// the only item the control reads or deletes. Only [`Listing::issue`] and
/// [`Control::make_item`] make one.
#[derive(Debug)]
pub struct OwnItem {
    name: String,
    user: String,
}

impl OwnItem {
    /// The item's name, as the Keychain holds it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The names of the top-level keys that the item holds (made-up data alone), and the number
    /// of entries of each that is an object; never a value.
    pub fn shape(&self, _control: &Control) -> Vec<String> {
        let Ok(ran) = security(
            "keychain::shape",
            [
                "find-generic-password",
                "-s",
                &self.name,
                "-a",
                &self.user,
                "-w",
            ],
            Duration::from_secs(60),
        ) else {
            return Vec::new();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(ran.stdout.trim()) else {
            return Vec::new();
        };
        value
            .as_object()
            .map(|keys| {
                keys.iter()
                    .map(|(key, inner)| match inner.as_object() {
                        Some(entries) => format!("{key}({})", entries.len()),
                        None => key.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Deletes the item, which this run made or gained: the one `delete-generic-password` of the
    /// kit. Whether it was there.
    pub fn forget(self, _control: &Control) -> Result<bool> {
        let ran = security(
            "keychain::forget",
            [
                "delete-generic-password",
                "-s",
                &self.name,
                "-a",
                &self.user,
            ],
            Duration::from_secs(60),
        )?;
        Ok(ran.code == Some(0))
    }
}

#[cfg(test)]
mod tests {
    use crate::live::keychain::fake::{listing, Listings};

    use super::*;

    fn issued(folder: &Path, before: &[&str], after: &[&str]) -> Option<OwnItem> {
        let _listings = Listings::new([listing(before, 0), listing(after, 0)]);
        let before = Listing::take().unwrap();
        let after = Listing::take().unwrap();
        before.issue(&after, folder)
    }

    /// T-2002 finding 1: a listing that failed is an error, never an empty set (an item that was
    /// there already was read as gained when the "before" listing came back empty).
    #[test]
    fn a_listing_that_failed_is_an_error_and_never_an_empty_set() {
        let _listings = Listings::new([listing(&[], 1)]);
        assert!(Listing::take().is_err());
    }

    /// An item is issued only for a folder this run made, and only when a successful listing before
    /// and a successful listing after show it gained.
    #[test]
    fn an_item_is_issued_only_for_a_folder_this_run_made_and_gained_between_two_listings() {
        let folder = crate::door::made("k-").unwrap();
        let name = item_of(folder.path());
        assert!(issued(folder.path(), &[], &[&name]).is_some());
        // There already: it is no item this run gained.
        assert!(issued(folder.path(), &[&name], &[&name]).is_none());
        // Not there after.
        assert!(issued(folder.path(), &[], &[]).is_none());
        // A folder this run did not make, whatever the Keychain shows.
        let foreign = crate::standin_process::unowned_tempdir();
        let other = foreign.path().join("a-folder-nobody-made");
        let other_name = item_of(&other);
        assert!(issued(&other, &[], &[&other_name]).is_none());
    }

    /// What the control reads of an item it issued is the names of its keys, never a value.
    #[test]
    fn what_the_control_reads_of_its_item_is_the_shape_and_never_a_value() {
        let folder = crate::door::made("k-").unwrap();
        let name = item_of(folder.path());
        let item = issued(folder.path(), &[], &[&name]).unwrap();
        let _listings = Listings::new([crate::process::Ran {
            code: Some(0),
            stdout: r#"{"mcpOAuth":{"a":1,"b":2},"token":"SECRET"}"#.to_owned(),
            stderr: String::new(),
            timed_out: false,
            held: false,
        }]);
        let control = Control::required_for("a test of the control").unwrap();
        let shape = item.shape(&control);
        assert_eq!(shape, vec!["mcpOAuth(2)".to_owned(), "token".to_owned()]);
        assert!(!format!("{shape:?}").contains("SECRET"));
    }
}

#[cfg(all(test, unix))]
mod control_tests {
    use super::*;

    /// The control's one start of Claude Code: on the account's own home and a config folder this
    /// run made, with the update switches, and never with a home a caller names or a config folder
    /// that is no folder of this run's. A stand-in program stands for Claude Code: nothing live.
    #[test]
    fn the_control_starts_claude_on_the_accounts_home_and_a_folder_this_run_made_and_no_other() {
        let control = Control(());
        let run = crate::door::made("cf-control-start-").unwrap();
        let program = run.path().join("claude");
        let seen = run.path().join("seen");
        crate::door::write_executable(&program, format!("#!/bin/sh\nenv > '{}'\n", seen.display()))
            .unwrap();
        // A file the run made is Claude Code's where the kit says it is its wrapper of Claude.
        crate::door::register_wrapper(&program, "claude");
        let config = run.path().join("config");
        crate::door::make_dir(&config).unwrap();
        let named = |config: &Path| {
            vec![
                ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
                (
                    "CLAUDE_CONFIG_DIR".to_owned(),
                    config.to_string_lossy().into_owned(),
                ),
            ]
        };
        control
            .start_claude(
                &program,
                &[],
                named(&config),
                run.path(),
                Duration::from_secs(20),
            )
            .unwrap();
        let env = std::fs::read_to_string(&seen).unwrap();
        let home = crate::live::machine::home().unwrap();
        assert!(
            env.lines()
                .any(|line| line == format!("HOME={}", home.display())),
            "{env}"
        );
        assert!(
            env.lines()
                .any(|line| line == format!("CLAUDE_CONFIG_DIR={}", config.display())),
            "{env}"
        );
        for (name, value) in crate::live::launcher::updates_off_of(crate::live::Harness::Claude) {
            assert!(
                env.lines().any(|line| line == format!("{name}={value}")),
                "{name}: {env}"
            );
        }
        // A home a caller names, a config folder nobody made, and none named: refused, nothing run.
        let mut with_home = named(&config);
        with_home.push(("HOME".to_owned(), "/somewhere".to_owned()));
        let foreign = crate::standin_process::unowned_tempdir();
        for vars in [with_home, named(foreign.path()), Vec::new()] {
            let _ = std::fs::remove_file(&seen);
            let refused = control
                .start_claude(&program, &[], vars, run.path(), Duration::from_secs(20))
                .unwrap_err();
            assert!(
                refused.to_string().contains("Keychain control"),
                "{refused}"
            );
            assert!(!seen.exists(), "a refused start ran");
        }
        // The config folder's name is matched in any case, as the homes are.
        let lower = vec![(
            "claude_config_dir".to_owned(),
            foreign.path().to_string_lossy().into_owned(),
        )];
        let refused = control
            .start_claude(&program, &[], lower, run.path(), Duration::from_secs(20))
            .unwrap_err()
            .to_string();
        // It is refused as a folder no run made, not as no folder named.
        assert!(
            refused.contains(&foreign.path().to_string_lossy().into_owned()),
            "{refused}"
        );
        assert!(!seen.exists(), "a refused start ran");
        // Any program but Claude Code's is refused, whatever it is named: a file of the run's the
        // kit does not know as Claude's, the Keychain's utility, cargo, a shell given a command.
        let other = run.path().join("other");
        crate::door::write_executable(&other, format!("#!/bin/sh\nenv > '{}'\n", seen.display()))
            .unwrap();
        // Decoys of the run's own, named as the programs they stand for.
        let decoy = |name: &str| {
            let path = run.path().join(name);
            crate::door::write_executable(
                &path,
                format!("#!/bin/sh\necho {name} >> '{}'\n", seen.display()),
            )
            .unwrap();
            path
        };
        let (security, cargo) = (decoy("security"), decoy("cargo"));
        for (program, args) in [
            (other.as_path(), vec![]),
            (security.as_path(), vec!["find-generic-password"]),
            (cargo.as_path(), vec!["--version"]),
            (Path::new("/bin/sh"), vec!["-c", "echo no"]),
        ] {
            let refused = control
                .start_claude(
                    program,
                    &args,
                    named(&config),
                    run.path(),
                    Duration::from_secs(20),
                )
                .unwrap_err();
            assert!(
                refused.to_string().contains("Keychain control"),
                "{}: {refused}",
                program.display()
            );
            assert!(!seen.exists(), "{} ran", program.display());
        }
    }
}
