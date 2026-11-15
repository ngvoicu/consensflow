//! Homes for a stand-in test process, established before libtest makes threads.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::door::Made;

static ROOT: OnceLock<Mutex<Option<Made>>> = OnceLock::new();
static FIXTURE_ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
static BUILD_COMPILER: OnceLock<Option<PathBuf>> = OnceLock::new();
static INHERITED_HARNESS_HOMES: OnceLock<Vec<PathBuf>> = OnceLock::new();
static INHERITED_LAUNCHERS: OnceLock<Vec<PathBuf>> = OnceLock::new();
static INHERITED_HOME: OnceLock<Option<std::ffi::OsString>> = OnceLock::new();
static INHERITED_LOCALAPPDATA: OnceLock<Option<std::ffi::OsString>> = OnceLock::new();
static INHERITED_PATH: OnceLock<std::ffi::OsString> = OnceLock::new();

/// A layout fixture's home, handed only to its test subprocess.
pub const FIXTURE_HOME: &str = "CF_E2E_STANDIN_FIXTURE_HOME";

extern "C" {
    fn atexit(callback: extern "C" fn()) -> i32;
}

extern "C" fn clean_up() {
    if let Some(root) = ROOT.get() {
        if let Some(mut root) = root
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            // A failed rig can have kept its report or its own folder beside
            // `home`. Do not remove those when the enclosing process ends.
            let left = std::fs::read_dir(root.path())
                .ok()
                .is_some_and(|entries| entries.flatten().any(|entry| entry.file_name() != "home"));
            if left || crate::door::Door::of_run().says_only() {
                root.keep();
            }
        }
    }
}

/// All variables by which a harness may find its home, under `home`.
pub fn home_vars(home: &Path) -> [(&'static str, PathBuf); 12] {
    [
        ("HOME", home.to_path_buf()),
        ("USERPROFILE", home.to_path_buf()),
        ("CLAUDE_CONFIG_DIR", home.join(".claude")),
        ("CODEX_HOME", home.join(".codex")),
        ("PI_CODING_AGENT_DIR", home.join(".pi/agent")),
        ("PI_CODING_AGENT_SESSION_DIR", home.join(".pi/sessions")),
        ("XDG_CONFIG_HOME", home.join(".config")),
        ("XDG_DATA_HOME", home.join(".local/share")),
        ("XDG_STATE_HOME", home.join(".local/state")),
        ("XDG_CACHE_HOME", home.join(".cache")),
        ("APPDATA", home.join("AppData/Roaming")),
        ("LOCALAPPDATA", home.join("AppData/Local")),
    ]
}

/// Begin an offline test process. Called from the test binary's constructor,
/// before libtest starts its worker threads or invokes a case.
#[allow(clippy::disallowed_methods)]
pub fn start(allow_live_matrix: bool) {
    let args: Vec<_> = std::env::args_os().collect();
    let live_matrix = allow_live_matrix
        && std::env::var_os("CF_E2E_LIVE_MATRIX").is_some_and(|v| v == "1")
        && args.iter().any(|arg| arg == "--ignored")
        && args.iter().any(|arg| arg == "--exact")
        && args.iter().any(|arg| arg == "a_board_of_real_harnesses_goes_through_its_scenario_as_the_ledger_and_the_records_say");
    if live_matrix {
        return;
    }

    INHERITED_HOME
        .set(std::env::var_os("HOME"))
        .unwrap_or_else(|_| panic!("inherited HOME selected twice"));
    INHERITED_LOCALAPPDATA
        .set(std::env::var_os("LOCALAPPDATA"))
        .unwrap_or_else(|_| panic!("inherited LOCALAPPDATA selected twice"));
    INHERITED_PATH
        .set(std::env::var_os("PATH").unwrap_or_default())
        .unwrap_or_else(|_| panic!("inherited PATH selected twice"));
    let inherited_homes = inherited_harness_homes();
    let launchers = inherited_launchers();
    require_inherited_home();
    let fixture_home = std::env::var_os(FIXTURE_HOME).map(PathBuf::from);
    std::env::remove_var(FIXTURE_HOME);
    let (path, compiler) = bounded_path(&inherited_homes);
    BUILD_COMPILER
        .set(compiler)
        .unwrap_or_else(|_| panic!("stand-in compiler was selected twice"));
    INHERITED_HARNESS_HOMES
        .set(inherited_homes)
        .unwrap_or_else(|_| panic!("stand-in inherited homes were recorded twice"));
    INHERITED_LAUNCHERS
        .set(launchers)
        .unwrap_or_else(|_| panic!("stand-in inherited launcher folders were recorded twice"));

    // The top-level process does not inherit another run's ownership claims.
    // A test's child retains what that test handed it through the door.
    if std::env::var_os("CF_E2E_STAND_IN").is_none() {
        std::env::remove_var(crate::door::MADE_ROOTS);
    }
    let forbidden = INHERITED_HARNESS_HOMES
        .get()
        .unwrap_or_else(|| panic!("stand-in inherited homes were not recorded"));
    let handed = checked_handed_roots(forbidden);
    let parent = process_temp_parent();
    require_audit_file(&parent, &handed, forbidden);
    let fixture_root = fixture_home
        .as_deref()
        .map(|home| fixture_root(home, &parent, forbidden));
    let root = crate::door::made_in(&parent, "h-")
        .unwrap_or_else(|error| panic!("make stand-in process home: {error}"));
    let home = fixture_home.unwrap_or_else(|| root.path().join("home"));
    crate::door::make_dir(&home).unwrap_or_else(|error| panic!("make stand-in home: {error}"));
    for (name, value) in home_vars(&home) {
        std::env::set_var(name, value);
    }
    std::env::set_var("PATH", path);
    let temp = if fixture_root.is_some() {
        &parent
    } else {
        root.path()
    };
    std::env::set_var("TMPDIR", temp);
    std::env::set_var("TMP", temp);
    std::env::set_var("TEMP", temp);
    std::env::set_var("CF_E2E_STAND_IN", "1");
    FIXTURE_ROOT
        .set(fixture_root)
        .unwrap_or_else(|_| panic!("stand-in fixture was selected twice"));
    ROOT.set(Mutex::new(Some(root)))
        .unwrap_or_else(|_| panic!("stand-in process began twice"));
    // This registration and all environment changes happen before test threads.
    assert_eq!(
        unsafe { atexit(clean_up) },
        0,
        "register stand-in home cleanup"
    );
}

/// The values the process was given before its stand-in home was installed.
pub(crate) fn inherited_home() -> Option<&'static std::ffi::OsStr> {
    INHERITED_HOME.get().and_then(Option::as_deref)
}

pub(crate) fn inherited_localappdata() -> Option<&'static std::ffi::OsStr> {
    INHERITED_LOCALAPPDATA.get().and_then(Option::as_deref)
}

pub(crate) fn inherited_path_value() -> Option<&'static std::ffi::OsStr> {
    INHERITED_PATH.get().map(std::ffi::OsString::as_os_str)
}

/// A stand-in needs an inherited account home to bound its searches.
#[allow(clippy::disallowed_methods)]
fn require_inherited_home() {
    assert!(
        ["HOME", "USERPROFILE"]
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty())),
        "stand-in inherited HOME lookup: neither HOME nor USERPROFILE names a home"
    );
}

/// The door's inherited roots must be safe to inspect before its registry
/// canonicalizes them. On Windows the door itself hands down canonical disk
/// paths with the system's verbatim prefix; convert only that known form.
#[allow(clippy::disallowed_methods)]
fn checked_handed_roots(forbidden: &[PathBuf]) -> Vec<PathBuf> {
    let roots: Vec<_> = std::env::var_os(crate::door::MADE_ROOTS)
        .map(|value| {
            std::env::split_paths(&value)
                .map(plain_handed_root)
                .collect()
        })
        .unwrap_or_default();
    for root in &roots {
        assert!(
            safe_path_entry(root, forbidden),
            "stand-in handed folder lookup: inside an inherited harness home"
        );
    }
    roots
}

#[cfg(windows)]
fn plain_handed_root(root: PathBuf) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let wide: Vec<u16> = root.as_os_str().encode_wide().collect();
    let prefix = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    if wide.starts_with(&prefix)
        && wide
            .get(4)
            .is_some_and(|ch| (*ch as u8).is_ascii_alphabetic() && *ch <= 0x7f)
        && wide.get(5) == Some(&(b':' as u16))
        && wide.get(6) == Some(&(b'\\' as u16))
    {
        PathBuf::from(std::ffi::OsString::from_wide(&wide[4..]))
    } else {
        root
    }
}

#[cfg(not(windows))]
fn plain_handed_root(root: PathBuf) -> PathBuf {
    root
}

/// Door initialization opens an audit file before making the process home.
/// It may do so only in a folder handed down by this stand-in's parent.
#[allow(clippy::disallowed_methods)]
fn require_audit_file(parent: &Path, handed: &[PathBuf], forbidden: &[PathBuf]) {
    let audit = std::env::var_os(crate::door::AUDIT);
    if !audit
        .as_deref()
        .is_some_and(|value| !value.is_empty() && value != "0")
    {
        return;
    }
    let named = std::env::var_os(crate::door::AUDIT_FILE).filter(|value| !value.is_empty());
    let file = named.as_ref().map_or_else(
        || parent.join(format!("consensflow-live-audit-{}.txt", std::process::id())),
        PathBuf::from,
    );
    assert!(
        std::env::var_os("CF_E2E_STAND_IN").as_deref() == Some(std::ffi::OsStr::new("1"))
            && plain_absolute(&file)
            && handed.iter().any(|root| same_or_inside(&file, root)),
        "stand-in audit file lookup: not handed down by the run"
    );
    assert!(
        safe_path_entry(&file, forbidden),
        "stand-in audit file lookup: inside an inherited harness home"
    );
    if named.is_none() {
        std::env::set_var(crate::door::AUDIT_FILE, file);
    }
}

/// The layout test's subprocess may use only a layout actually handed down by
/// its parent through the door. Check its spelling before consulting the disk.
#[allow(clippy::disallowed_methods)]
fn fixture_root(home: &Path, temp: &Path, forbidden: &[PathBuf]) -> PathBuf {
    assert!(
        std::env::var_os("CF_E2E_STAND_IN").as_deref() == Some(std::ffi::OsStr::new("1")),
        "stand-in fixture home lookup: no stand-in parent"
    );
    let root = home
        .parent()
        .unwrap_or_else(|| panic!("stand-in fixture home lookup: no root"));
    assert!(
        home.file_name().is_some_and(|name| name == "home")
            && temp.file_name().is_some_and(|name| name == "temp")
            && temp.parent() == Some(root)
            && safe_path_entry(root, forbidden),
        "stand-in fixture home lookup: invalid layout or inherited harness home"
    );
    let handed = std::env::var_os(crate::door::MADE_ROOTS)
        .map(|value| std::env::split_paths(&value).any(|made| same_or_inside(root, &made)))
        .unwrap_or(false);
    assert!(
        handed,
        "stand-in fixture home lookup: layout not handed down"
    );
    crate::door::writable(root, true)
        .unwrap_or_else(|error| panic!("stand-in fixture home lookup: {error}"));
    root.to_path_buf()
}

/// Collect the inherited harness homes without inspecting the filesystem.
#[allow(clippy::disallowed_methods)] // Read the inherited paths before replacing the environment.
fn inherited_harness_homes() -> Vec<PathBuf> {
    let mut forbidden: Vec<PathBuf> = [
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "PI_CODING_AGENT_DIR",
        "PI_CODING_AGENT_SESSION_DIR",
    ]
    .into_iter()
    .filter_map(inherited_path)
    .collect();
    for name in ["HOME", "USERPROFILE"] {
        if let Some(home) = inherited_path(name) {
            for suffix in [
                ".claude",
                ".codex",
                ".pi",
                ".config/muse",
                ".config/devin",
                ".local/share/devin",
            ] {
                forbidden.push(home.join(suffix));
            }
        }
    }
    for name in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "APPDATA",
        "LOCALAPPDATA",
    ] {
        if let Some(base) = inherited_path(name) {
            for harness in ["claude", "codex", "pi", "devin", "muse"] {
                forbidden.push(base.join(harness));
            }
        }
    }
    forbidden
}

/// An inherited path, made absolute without reading it.
#[allow(clippy::disallowed_methods)]
fn inherited_path(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name).filter(|value| !value.is_empty())?);
    std::path::absolute(path).ok()
}

fn inherited_launchers() -> Vec<PathBuf> {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(inherited_path)
        .map(|home| home.join(".local").join("bin"))
        .chain(inherited_path("LOCALAPPDATA").map(|home| home.join("Programs").join("muse")))
        .collect()
}

/// Keep inherited PATH except the account's usual launcher folder and harness homes.
/// The former can contain the person's actual harness launchers.
#[allow(clippy::disallowed_methods)]
fn bounded_path(forbidden: &[PathBuf]) -> (std::ffi::OsString, Option<PathBuf>) {
    let cargo = Path::new(env!("CARGO"));
    let compiler = cargo.with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    let compiler =
        (safe_path_entry(&compiler, forbidden) && compiler.is_file()).then_some(compiler);
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let launchers = inherited_launchers();
    let safe = std::env::split_paths(&inherited).filter(|folder| {
        !launchers.iter().any(|home| same_or_inside(folder, home))
            && safe_path_entry(folder, forbidden)
    });
    let path =
        std::env::join_paths(safe).unwrap_or_else(|_| panic!("stand-in PATH lookup: invalid path"));
    (path, compiler)
}

/// Match names as a child process's environment does on the current platform.
#[cfg(windows)]
pub(crate) fn same_environment_name(left: &str, right: &str) -> bool {
    let left: Vec<u16> = left.encode_utf16().collect();
    let right: Vec<u16> = right.encode_utf16().collect();
    windows_ordinal_eq(&left, &right)
}

#[cfg(not(windows))]
pub(crate) fn same_environment_name(left: &str, right: &str) -> bool {
    left == right
}

#[cfg(windows)]
#[allow(unsafe_code)] // The counted UTF-16 slices remain valid for this call.
fn windows_ordinal_eq(left: &[u16], right: &[u16]) -> bool {
    use windows_sys::Win32::Globalization::{CompareStringOrdinal, CSTR_EQUAL};
    if left.is_empty() || right.is_empty() {
        return left.is_empty() && right.is_empty();
    }
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return false;
    };
    // SAFETY: both pointers name initialized slices of the supplied lengths,
    // and CompareStringOrdinal does not retain them.
    unsafe {
        CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) == CSTR_EQUAL
    }
}

/// Refuse a rig PATH that points into an inherited harness home.
pub(crate) fn require_search_path(value: &str, lookup: &str) {
    if crate::process::own_var("CF_E2E_STAND_IN").as_deref() != Some("1") {
        return;
    }
    let forbidden = INHERITED_HARNESS_HOMES
        .get()
        .unwrap_or_else(|| panic!("{lookup}: no inherited-home boundary"));
    for entry in std::env::split_paths(value) {
        assert!(
            safe_path_entry(&entry, forbidden)
                && safe_path_entry(
                    &entry,
                    INHERITED_LAUNCHERS
                        .get()
                        .unwrap_or_else(|| panic!("launcher folders not recorded"))
                ),
            "{lookup}: inside an inherited harness home"
        );
    }
}

/// Compare absolute path components without consulting any path or following links.
fn same_or_inside(folder: &Path, home: &Path) -> bool {
    let Ok(folder) = std::path::absolute(folder) else {
        return false;
    };
    let Ok(home) = std::path::absolute(home) else {
        return false;
    };
    let mut parts = folder.components();
    home.components().all(|part| {
        parts.next().is_some_and(|candidate| {
            if cfg!(any(windows, target_os = "macos")) {
                candidate.as_os_str().to_string_lossy().to_lowercase()
                    == part.as_os_str().to_string_lossy().to_lowercase()
            } else {
                candidate == part
            }
        })
    })
}

fn plain_absolute(folder: &Path) -> bool {
    folder.is_absolute()
}

fn safe_path_entry(folder: &Path, forbidden: &[PathBuf]) -> bool {
    !forbidden.iter().any(|home| same_or_inside(folder, home))
}

/// Whether an inherited harness-home directory may be inspected by discovery.
pub(crate) fn outside_inherited_harness_homes(path: &Path) -> bool {
    if crate::process::own_var("CF_E2E_STAND_IN").as_deref() != Some("1") {
        return true;
    }
    let forbidden = INHERITED_HARNESS_HOMES
        .get()
        .unwrap_or_else(|| panic!("stand-in inherited homes not recorded"));
    safe_path_entry(path, forbidden)
}

/// Check an inherited PATH candidate before the harness search inspects it.
pub(crate) fn require_search_candidate(path: &Path, lookup: &str) {
    if crate::process::own_var("CF_E2E_STAND_IN").as_deref() != Some("1") {
        return;
    }
    let forbidden = INHERITED_HARNESS_HOMES
        .get()
        .unwrap_or_else(|| panic!("{lookup}: no inherited-home boundary"));
    assert!(
        safe_path_entry(path, forbidden),
        "{lookup}: inside an inherited harness home"
    );
}

/// A stand-in searches for harness executables only in its own fixtures or build.
pub(crate) fn may_search_harness(path: &Path) -> bool {
    if crate::process::own_var("CF_E2E_STAND_IN").as_deref() != Some("1") {
        return true;
    }
    let Some(root) = ROOT.get() else {
        return false;
    };
    let root = root.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(root) = root.as_ref() else {
        return false;
    };
    let fixture = FIXTURE_ROOT.get().and_then(Option::as_deref);
    let target = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(Path::parent).map(Path::to_path_buf));
    same_or_inside(path, root.path())
        || fixture.is_some_and(|root| same_or_inside(path, root))
        || target
            .as_deref()
            .is_some_and(|root| same_or_inside(path, root))
}

/// The compiler of the Cargo that built this test, if its complete path was
/// safe to inspect before the inherited homes were discarded.
pub(crate) fn build_compiler() -> Option<&'static Path> {
    BUILD_COMPILER.get().and_then(Option::as_deref)
}

/// Windows puts TEMP in the account profile; start at its public scratch folder.
#[allow(clippy::disallowed_methods)]
fn process_temp_parent() -> PathBuf {
    #[cfg(windows)]
    let parent = if std::env::var_os("CF_E2E_STAND_IN").is_some() {
        std::env::temp_dir()
    } else {
        std::env::var_os("PUBLIC")
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("stand-in temporary lookup: PUBLIC is not set"))
    };
    #[cfg(target_os = "macos")]
    let parent = {
        let temp = std::env::temp_dir();
        if let Ok(rest) = temp.strip_prefix("/var") {
            Path::new("/private/var").join(rest)
        } else if let Ok(rest) = temp.strip_prefix("/tmp") {
            Path::new("/private/tmp").join(rest)
        } else {
            temp
        }
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let parent = std::env::temp_dir();
    std::path::absolute(parent)
        .unwrap_or_else(|_| panic!("stand-in temporary lookup: invalid path"))
}

/// Refuse a harness-home lookup outside the run before examining it.
#[allow(clippy::disallowed_methods)]
pub fn require_home(path: &Path, lookup: &str) {
    if std::env::var_os("CF_E2E_STAND_IN").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let root = ROOT
        .get()
        .unwrap_or_else(|| panic!("{lookup}: no process home"));
    let root = root.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = root
        .as_ref()
        .unwrap_or_else(|| panic!("{lookup}: after home cleanup"));
    let fixture = FIXTURE_ROOT.get().and_then(Option::as_deref);
    assert!(
        same_or_inside(path, root.path()) || fixture.is_some_and(|root| same_or_inside(path, root)),
        "{lookup}: outside the stand-in run"
    );
}

/// Refuse a watched path outside the run or its built binaries.
#[allow(clippy::disallowed_methods)]
pub fn require_watch_path(path: &Path, lookup: &str) {
    if std::env::var_os("CF_E2E_STAND_IN").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let root = ROOT
        .get()
        .unwrap_or_else(|| panic!("{lookup}: no process home"));
    let root = root.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = root
        .as_ref()
        .unwrap_or_else(|| panic!("{lookup}: after home cleanup"));
    let fixture = FIXTURE_ROOT.get().and_then(Option::as_deref);
    let target = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(Path::parent).map(Path::to_path_buf));
    assert!(
        same_or_inside(path, root.path())
            || fixture.is_some_and(|root| same_or_inside(path, root))
            || target
                .as_deref()
                .is_some_and(|root| same_or_inside(path, root)),
        "{lookup}: outside the stand-in run"
    );
}

/// A synthetic foreign folder must be outside every root handed to this test
/// process, including the parent's root when the test runs again under audit.
#[cfg(test)]
pub(crate) fn unowned_tempdir() -> tempfile::TempDir {
    let temp = std::env::temp_dir();
    let parent = temp
        .ancestors()
        .find(|path| crate::door::writable(path, true).is_err())
        .expect("no unowned parent of the stand-in temp folder");
    let foreign = tempfile::Builder::new()
        .prefix("foreign-fixture-")
        .tempdir_in(parent)
        .expect("make a synthetic foreign folder");
    assert!(crate::door::writable(foreign.path(), true).is_err());
    foreign
}

#[cfg(test)]
mod tests {
    use super::safe_path_entry;

    #[test]
    fn standin_isolation_missing_inherited_home_refuses_before_searching_path() {
        let root = crate::door::made("missing-home-").unwrap();
        let path = root.path().join(".codex").join("bin");
        crate::door::make_dir(&path).unwrap();
        let mut command = crate::process::case_of(
            std::env::current_exe().unwrap(),
            "standin_process::tests::standin_isolation_missing_inherited_home_refuses_before_searching_path",
            std::time::Duration::from_secs(120),
        )
        .inheriting_env()
        .arg("--list")
        .var("PATH", &path)
        .without("CF_E2E_STAND_IN");
        for (name, _) in super::home_vars(root.path()) {
            command = command.without(name);
        }
        let output = command.run().unwrap();
        assert_ne!(output.code, Some(0), "missing inherited home was accepted");
        assert!(
            output.stderr.contains("stand-in inherited HOME lookup"),
            "{}{}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn standin_isolation_inherited_account_home_is_not_searched_on_path() {
        const CHILD: &str = "CF_T1647_ACCOUNT_PATH";
        if let Some(target) = crate::process::own_var(CHILD) {
            let path = crate::process::own_var("PATH").unwrap_or_default();
            assert!(
                !std::env::split_paths(&path)
                    .any(|entry| super::same_or_inside(&entry, std::path::Path::new(&target))),
                "inherited account-home PATH lookup was retained"
            );
            return;
        }
        let root = crate::door::made("account-path-").unwrap();
        let home = root.path().join("account");
        let target = home.join(".local").join("bin");
        crate::door::make_dir(&target).unwrap();
        let output = crate::process::case_of(
            std::env::current_exe().unwrap(),
            "standin_process::tests::standin_isolation_inherited_account_home_is_not_searched_on_path",
            std::time::Duration::from_secs(120),
        )
        .inheriting_env()
        .var("HOME", &home)
        .var("USERPROFILE", &home)
        .var("PATH", &target)
        .var(CHILD, &target)
        .without("CF_E2E_STAND_IN")
        .run()
        .unwrap();
        assert_eq!(output.code, Some(0), "{}{}", output.stdout, output.stderr);
    }

    #[test]
    fn standin_isolation_unowned_audit_file_is_refused_before_it_is_opened() {
        let temp = std::env::temp_dir();
        let foreign = temp.parent().unwrap().join("synthetic-foreign-audit-file");
        let output = crate::process::case_of(
            std::env::current_exe().unwrap(),
            "standin_process::tests::standin_isolation_unowned_audit_file_is_refused_before_it_is_opened",
            std::time::Duration::from_secs(120),
        )
        .inheriting_env()
        .arg("--list")
        .var(crate::door::AUDIT, "1")
        .var(crate::door::AUDIT_MADE, "1")
        .var(crate::door::AUDIT_FILE, &foreign)
        .run()
        .unwrap();
        assert_ne!(output.code, Some(0), "unowned audit file was accepted");
        assert!(
            output.stderr.contains("stand-in audit file lookup"),
            "{}{}",
            output.stdout,
            output.stderr
        );
        assert!(
            !output.stderr.contains("not there to add to"),
            "the door attempted to open the unowned file: {}",
            output.stderr
        );
    }

    #[test]
    fn standin_isolation_fixture_home_must_be_handed_down() {
        let temp = std::env::temp_dir();
        let foreign = temp.parent().unwrap().join("synthetic-foreign-layout");
        let output = crate::process::case_of(
            std::env::current_exe().unwrap(),
            "standin_process::tests::standin_isolation_fixture_home_must_be_handed_down",
            std::time::Duration::from_secs(120),
        )
        .inheriting_env()
        .var("CF_E2E_STAND_IN", "1")
        .var(super::FIXTURE_HOME, foreign.join("home"))
        .var("HOME", foreign.join("home"))
        .var("USERPROFILE", foreign.join("home"))
        .var("TMPDIR", foreign.join("temp"))
        .var("TMP", foreign.join("temp"))
        .var("TEMP", foreign.join("temp"))
        .run()
        .unwrap();
        assert_ne!(output.code, Some(0), "unowned fixture was accepted");
        assert!(
            output
                .stderr
                .contains("stand-in fixture home lookup: layout not handed down"),
            "{}{}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn standin_isolation_toolchain_paths_refuse_harness_homes() {
        let root = crate::door::made("toolchain-path-").unwrap();
        let forbidden = root.path().join("owner/.codex");
        crate::door::make_dir(&forbidden).unwrap();
        let compiler = forbidden.join("rustc");
        crate::door::write(&compiler, "decoy").unwrap();
        assert!(!safe_path_entry(
            &compiler,
            std::slice::from_ref(&forbidden)
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn standin_isolation_path_refuses_case_aliases() {
        let root = crate::door::made("path-case-").unwrap();
        let forbidden = root.path().join("synthetic-owner/.codex");
        crate::door::make_dir(&forbidden).unwrap();
        let alias = root.path().join("SYNTHETIC-OWNER/.CODEX/bin");
        assert!(super::same_or_inside(&alias, &forbidden));
        assert!(!safe_path_entry(&alias, &[forbidden]));
    }
}
