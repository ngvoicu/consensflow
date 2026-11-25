//! The account that started the run, and the protection that travels from it (K3,
//! `live-kit-isolation-doors.md` section 1).
//!
//! The run's first process reads the account's environment once, here, and makes the list of
//! protected roots from the table of stores ([`crate::live::stores`]). It writes the list in a
//! file of a folder it made, and every child the door starts is told that file
//! ([`PROTECTED_FILE`]) and the digest of what was written in it ([`PROTECTED_DIGEST`]). A child
//! reads the file and checks its contents against the digest: it binds to the list the run
//! wrote, never to a later file at that path, and never builds the list from its own
//! environment, which is the run's (a stand-in Codex writing `memories` in its private
//! `CODEX_HOME` keeps the owner's roots and is not refused its own). A process told the run's
//! folders ([`super::MADE_ROOTS`]) and no such file stops.
//!
//! The roots file itself, and every folder it is in, are protected for the run's life: no write,
//! rename or removal of the door's reaches them.
//!
//! A person who sets these variables by hand before `cargo test` gives that run those roots:
//! the same known limit [`super::MADE_ROOTS`] has. `cargo xtask live` clears both.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use sha2::{Digest as _, Sha256};

/// The variable that names the file of the run's protected roots, for a child the door starts.
pub const PROTECTED_FILE: &str = "CF_LIVE_PROTECTED_FILE";

/// The variable that holds the digest of what the run wrote in that file.
pub const PROTECTED_DIGEST: &str = "CF_LIVE_PROTECTED_DIGEST";

/// The account as the run's first process found it, or as its parent handed it on.
#[derive(Debug)]
struct Held {
    /// The protected roots, as spelled.
    roots: Vec<PathBuf>,
    /// Where they are written for a child, and the digest of what was written: none until the
    /// first child is started.
    file: OnceLock<(PathBuf, String)>,
    /// The person's home, for the rig's placement of project folders: the first process's own
    /// `HOME` (`USERPROFILE` on Windows where `HOME` is not set). A child has none.
    home: Option<PathBuf>,
}

static ACCOUNT: OnceLock<Held> = OnceLock::new();

/// The digest of `text`, in hex.
fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The roots `text` holds (a JSON array of paths), where its digest is `expected`.
pub fn read_roots(text: &str, expected: &str) -> Result<Vec<PathBuf>, String> {
    if digest(text) != expected {
        return Err(
            "the protected roots' file is not the one the run wrote (its digest differs)".into(),
        );
    }
    let roots: Vec<String> = serde_json::from_str(text)
        .map_err(|failed| format!("the protected roots' file is no list of paths: {failed}"))?;
    Ok(roots.into_iter().map(PathBuf::from).collect())
}

/// The account of this process, from `lookup` (its environment as it was started): a child's
/// from the file it was handed, the first process's from the table of stores.
fn account_of(lookup: &dyn Fn(&str) -> Option<OsString>) -> Result<Held, String> {
    let handed = lookup(PROTECTED_FILE).filter(|value| !value.is_empty());
    if let Some(file) = handed {
        let expected = lookup(PROTECTED_DIGEST)
            .and_then(|value| value.into_string().ok())
            .ok_or_else(|| format!("{PROTECTED_FILE} is set and {PROTECTED_DIGEST} is not"))?;
        let file = PathBuf::from(file);
        let text = std::fs::read_to_string(&file).map_err(|failed| {
            format!(
                "the protected roots' file {} cannot be read: {failed}",
                file.display()
            )
        })?;
        let roots = read_roots(&text, &expected)?;
        return Ok(Held {
            roots,
            file: OnceLock::from((file, expected)),
            home: None,
        });
    }
    if lookup(super::MADE_ROOTS).is_some_and(|value| !value.is_empty()) {
        return Err(format!(
            "this process was told the run's folders ({}) and not the roots it must not write in ({PROTECTED_FILE}): it stops",
            super::MADE_ROOTS
        ));
    }
    let roots = crate::live::stores::roots(lookup)
        .into_iter()
        .map(|root| root.path)
        .collect();
    let home = ["HOME", "USERPROFILE"]
        .into_iter()
        .find_map(|name| lookup(name).filter(|value| !value.is_empty()))
        .map(PathBuf::from);
    Ok(Held {
        roots,
        file: OnceLock::new(),
        home,
    })
}

/// Reads the account now, before anything changes this process's environment: the offline
/// test process calls it before it gives itself its stand-in homes. Later calls are the first's.
#[allow(clippy::disallowed_methods)] // The one reader of the account's own environment.
pub fn begin() {
    let _ = ACCOUNT.get_or_init(|| {
        account_of(&|name| std::env::var_os(name)).unwrap_or_else(|why| panic!("{why}"))
    });
}

fn account() -> &'static Held {
    begin();
    ACCOUNT.get().unwrap_or_else(|| unreachable!("begun above"))
}

/// Roots a test adds to the protected ones, for the case that shows the door refuses a place in
/// one (the account's own cannot be made up).
#[cfg(test)]
pub(crate) static TEST_PROTECTED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// A root a test adds to the protected ones while it is held, and takes away when dropped.
#[cfg(test)]
pub(crate) struct TestRoot(PathBuf);

#[cfg(test)]
impl TestRoot {
    pub(crate) fn add(root: &Path) -> TestRoot {
        TEST_PROTECTED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(root.to_path_buf());
        TestRoot(root.to_path_buf())
    }
}

#[cfg(test)]
impl Drop for TestRoot {
    fn drop(&mut self) {
        TEST_PROTECTED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|root| root != &self.0);
    }
}

/// Every root the door protects now: the account's, and the file of them with the folder it is
/// in, once it is written.
pub fn protected() -> Vec<PathBuf> {
    let account = account();
    #[allow(unused_mut)]
    let mut roots = account.roots.clone();
    // The file is the door's own, taken only as the process that wrote it ends; once gone it
    // holds nothing, and the folders it was in are the run's to take again.
    if let Some((file, _)) = account.file.get() {
        if std::fs::symlink_metadata(file).is_ok() {
            roots.push(file.clone());
        }
    }
    #[cfg(test)]
    roots.extend(
        TEST_PROTECTED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned(),
    );
    roots
}

/// The account's home as this process's environment says it now: `HOME`, else `USERPROFILE`. A
/// stand-in run's is its decoy account's: the kit reads the files of the account its process is
/// in, and the protected roots are the real account's, which the first call to [`begin`] read.
#[allow(clippy::disallowed_methods)] // One of the two readers of the account's own environment.
pub(super) fn home_of_this_process() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

/// The variable `name` as this process's environment says it now.
#[allow(clippy::disallowed_methods)] // One of the readers of the account's own environment.
pub(super) fn lookup_in_this_process(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// The folder the variable `name` of a store's row names in this process's environment, where
/// set and not empty.
#[allow(clippy::disallowed_methods)] // One of the two readers of the account's own environment.
pub(super) fn variable_of_this_process(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The person's home, where the rig places project folders: the first process's own, never a
/// child's, and in no environment the door gives.
pub fn project_home() -> Option<PathBuf> {
    account().home.clone()
}

/// What a child the door starts is told of the protection: the file of the roots and its digest,
/// the file written in a folder this run makes the first time a child is started.
pub fn child_vars() -> Result<Vec<(String, String)>, String> {
    // A test that adds roots hands its children a file of its own with them: a child keeps the
    // roots of the run and is refused the test's decoys as it is the account's.
    #[cfg(test)]
    {
        let extra = TEST_PROTECTED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if !extra.is_empty() {
            let mut roots = account().roots.clone();
            roots.extend(extra);
            let (path, digest) = write_roots(&roots)?;
            return Ok(vec![
                (
                    PROTECTED_FILE.to_owned(),
                    path.to_string_lossy().into_owned(),
                ),
                (PROTECTED_DIGEST.to_owned(), digest),
            ]);
        }
    }
    let (path, digest) = own_file()?;
    let mut vars = vec![
        (
            PROTECTED_FILE.to_owned(),
            path.to_string_lossy().into_owned(),
        ),
        (PROTECTED_DIGEST.to_owned(), digest),
    ];
    // A child has no home of the person's to find cargo's and rustup's folders by (its own are the
    // run's): it is told the ones this process works with.
    let home = project_home();
    for (name, below) in [("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")] {
        let held = own_variable(name).map(PathBuf::from).or_else(|| {
            home.as_ref()
                .map(|home| home.join(below))
                .filter(|dir| dir.is_dir())
        });
        if let Some(held) = held {
            vars.push((name.to_owned(), held.to_string_lossy().into_owned()));
        }
    }
    Ok(vars)
}

/// A variable of this process's environment, as the door's own reads take it.
#[allow(clippy::disallowed_methods)] // The door reads the test's environment.
fn own_variable(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// The file of the account's roots and the digest of what was written in it, written the first
/// time it is asked for.
pub(crate) fn own_file() -> Result<(PathBuf, String), String> {
    static WRITING: Mutex<()> = Mutex::new(());
    let account = account();
    if account.file.get().is_none() {
        // One writer at a time; the judge the writing asks reads the file's place unlocked.
        let _writing = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
        if account.file.get().is_none() {
            let _ = account.file.set(write_roots(&account.roots)?);
        }
    }
    Ok(account
        .file
        .get()
        .cloned()
        .unwrap_or_else(|| unreachable!("written above")))
}

/// Writes `roots` in a new file of a folder this run makes: its path and the digest of what was
/// written.
fn write_roots(roots: &[PathBuf]) -> Result<(PathBuf, String), String> {
    let text = serde_json::to_string(
        &roots
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
    )
    .map_err(|failed| failed.to_string())?;
    let folder = super::made("cf-live-roots-")
        .map_err(|failed| format!("the protected roots' folder cannot be made: {failed}"))?
        .hand_on();
    let file = folder.join("protected-roots.json");
    {
        use std::io::Write as _;
        let mut open = super::writes::create_new(&file).map_err(|failed| failed.to_string())?;
        open.write_all(text.as_bytes())
            .map_err(|failed| failed.to_string())?;
    }
    remove_at_exit(&folder);
    Ok((file, digest(&text)))
}

/// The door's own folders taken as the process that made them ends: the roots' file and its
/// folder, the scratch home of programs whose case names none.
static AT_EXIT: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

extern "C" {
    fn atexit(callback: extern "C" fn()) -> i32;
}

extern "C" fn remove_door_folders() {
    let folders = std::mem::take(&mut *AT_EXIT.lock().unwrap_or_else(PoisonError::into_inner));
    // The roots' file goes last of all, once nothing of the door's is left to judge.
    for folder in folders.iter().rev() {
        // The door's own folders, which it made by these literal paths and took nothing into
        // that was not its own.
        let _ = std::fs::remove_dir_all(folder);
    }
}

/// Takes `folder`, one of the door's own, when this process ends.
#[allow(unsafe_code)] // `atexit`, as the offline test process registers its own clean-up.
pub(crate) fn remove_at_exit(folder: &Path) {
    let mut folders = AT_EXIT.lock().unwrap_or_else(PoisonError::into_inner);
    if folders.is_empty() {
        // SAFETY: the callback takes a list under its lock and touches only the paths in it.
        let _ = unsafe { atexit(remove_door_folders) };
    }
    folders.push(folder.to_path_buf());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup<'a>(vars: &'a HashMap<&str, OsString>) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| vars.get(name).cloned()
    }

    #[test]
    fn a_child_binds_to_the_roots_the_run_wrote_and_to_no_later_file_at_that_path() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("protected-roots.json");
        let written = serde_json::to_string(&["/owner/.codex", "/owner/.claude"]).unwrap();
        std::fs::write(&file, &written).unwrap();
        let vars: HashMap<&str, OsString> = [
            (PROTECTED_FILE, file.clone().into_os_string()),
            (PROTECTED_DIGEST, OsString::from(digest(&written))),
            // The child's own homes are the run's, and no root is made of them.
            ("HOME", OsString::from("/run/home")),
            ("CODEX_HOME", OsString::from("/run/home/.codex")),
        ]
        .into();
        let account = account_of(&lookup(&vars)).unwrap();
        assert_eq!(
            account.roots,
            [
                PathBuf::from("/owner/.codex"),
                PathBuf::from("/owner/.claude")
            ]
        );
        assert_eq!(account.home, None, "a child has no person's home");
        // A well-formed list with a root left out, put at the same path later, is refused.
        let later = serde_json::to_string(&["/owner/.claude"]).unwrap();
        std::fs::write(&file, later).unwrap();
        let refused = account_of(&lookup(&vars)).unwrap_err();
        assert!(refused.contains("not the one the run wrote"), "{refused}");
    }

    #[test]
    fn a_process_told_the_runs_folders_and_no_roots_stops() {
        let vars: HashMap<&str, OsString> =
            [(super::super::MADE_ROOTS, OsString::from("/run/made"))].into();
        let refused = account_of(&lookup(&vars)).unwrap_err();
        assert!(refused.contains(PROTECTED_FILE), "{refused}");
    }

    #[test]
    fn the_first_process_makes_its_roots_from_the_table_of_its_own_account() {
        let base = PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" });
        let (owner, elsewhere) = (base.join("owner"), base.join("elsewhere").join("codex"));
        let vars: HashMap<&str, OsString> = [
            ("HOME", owner.clone().into_os_string()),
            ("CODEX_HOME", elsewhere.clone().into_os_string()),
        ]
        .into();
        let account = account_of(&lookup(&vars)).unwrap();
        assert!(account.roots.contains(&owner.join(".codex")));
        assert!(account.roots.contains(&elsewhere));
        assert_eq!(account.home.as_deref(), Some(owner.as_path()));
    }
}
