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

/// All variables by which a harness may find its home, under `home`. On Windows the two halves
/// of the profile path, `HOMEDRIVE` and `HOMEPATH`, are of the home too.
pub fn home_vars(home: &Path) -> Vec<(&'static str, PathBuf)> {
    let mut vars = vec![
        ("HOME", home.to_path_buf()),
        ("USERPROFILE", home.to_path_buf()),
        ("CFFIXED_USER_HOME", home.to_path_buf()),
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
    ];
    if cfg!(windows) {
        if let Some((drive, path)) = drive_and_path(home) {
            vars.push(("HOMEDRIVE", PathBuf::from(drive)));
            vars.push(("HOMEPATH", PathBuf::from(path)));
        }
    }
    vars
}

/// A Windows path as `HOMEDRIVE` and `HOMEPATH` say it: `C:` and `\Users\x`, with the verbatim
/// prefix taken as the plain one. None for a path with no drive.
pub fn drive_and_path(home: &Path) -> Option<(String, String)> {
    let text = home.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let rest = text[2..].replace('/', "\\");
        let rest = if rest.starts_with('\\') {
            rest
        } else {
            format!("\\{rest}")
        };
        Some((text[..2].to_owned(), rest))
    } else {
        None
    }
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

    // The top-level process does not inherit another run's ownership claims nor its roots; a
    // test's child keeps what that test handed it through the door. The account is read now,
    // before this process gives itself its stand-in homes.
    if std::env::var_os("CF_E2E_STAND_IN").is_none() {
        std::env::remove_var(crate::door::MADE_ROOTS);
        std::env::remove_var(crate::door::account::PROTECTED_FILE);
        std::env::remove_var(crate::door::account::PROTECTED_DIGEST);
    }
    crate::door::account::begin();
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

    let forbidden = INHERITED_HARNESS_HOMES
        .get()
        .unwrap_or_else(|| panic!("stand-in inherited homes were not recorded"));
    let handed = checked_handed_roots(forbidden);
    let parent = process_temp_parent();
    if let Err(refusal) = temp_parent_is_outside(&parent, forbidden) {
        panic!("{refusal}");
    }
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

/// The protected roots of the account the run began in, as the door holds them
/// ([`crate::door::account`]): the table of stores read on the account's environment by the run's
/// first process, or the run's file of them in a child. Never built from this process's own
/// environment once it is a stand-in's.
fn inherited_harness_homes() -> Vec<PathBuf> {
    crate::door::account::protected()
}

/// What the table of stores holds below `home` for an account whose only variable is that home:
/// for the tests of what a store's spelling reaches.
#[cfg(test)]
fn personal_harness_homes(home: &Path) -> Vec<PathBuf> {
    let home = home.as_os_str().to_owned();
    crate::live::stores::roots_for(&|name| (name == "HOME").then(|| home.clone()), None)
        .into_iter()
        .map(|root| root.path)
        .collect()
}

/// Whether the folder a run would make its homes in is outside every protected one.
fn temp_parent_is_outside(parent: &Path, forbidden: &[PathBuf]) -> Result<(), String> {
    if safe_path_entry(parent, forbidden) {
        Ok(())
    } else {
        Err(format!(
            "stand-in temporary lookup: {} is inside a protected harness home",
            parent.display()
        ))
    }
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
        !launchers.iter().any(|home| may_reach(folder, home)) && safe_path_entry(folder, forbidden)
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

/// Whether `folder` is `home` or below it, **by every account of it**: the path the way it is
/// spelled, and the folder it really is, links and `..` followed as the system follows them
/// (B19). Used where a path is let in (the run's own folders): a `..` that climbs out of the
/// run, a link out of it, and a spelling that is another folder's are all "not inside".
fn same_or_inside(folder: &Path, home: &Path) -> bool {
    spelled_inside(folder, home) && spelled_inside(&real_of(folder), &real_of(home))
}

/// Whether `folder` could be `home` or below it by any account of it: as spelled, with its `..`
/// settled, or as the folder it really is. Used where a path is kept out of a harness's home
/// (B19): any one account that puts it there is enough.
fn may_reach(folder: &Path, home: &Path) -> bool {
    spelled_inside(folder, home)
        || spelled_inside(&settled(folder), &settled(home))
        || spelled_inside(&real_of(folder), &real_of(home))
}

/// Compare absolute path components, as spelled, without consulting any path or following links,
/// Windows' verbatim prefix (`\\?\C:`) taken as the plain one.
fn spelled_inside(folder: &Path, home: &Path) -> bool {
    let Ok(folder) = std::path::absolute(folder) else {
        return false;
    };
    let Ok(home) = std::path::absolute(home) else {
        return false;
    };
    let words = |path: &Path| -> Vec<String> {
        path.components()
            .map(|part| match part {
                std::path::Component::Prefix(prefix) => {
                    use std::path::Prefix::{Verbatim, VerbatimDisk, VerbatimUNC};
                    let text = prefix.as_os_str().to_string_lossy().to_lowercase();
                    match prefix.kind() {
                        VerbatimDisk(_) | Verbatim(_) | VerbatimUNC(..) => {
                            text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
                        }
                        _ => text,
                    }
                }
                other => {
                    let text = other.as_os_str().to_string_lossy();
                    if cfg!(any(windows, target_os = "macos")) {
                        text.to_lowercase()
                    } else {
                        text.into_owned()
                    }
                }
            })
            .collect()
    };
    let (mut parts, home) = (words(&folder).into_iter(), words(&home));
    home.iter().all(|part| parts.next().as_ref() == Some(part))
}

/// `path`, absolute, with its `.` and `..` words settled in the way a system settles them.
fn settled(path: &Path) -> PathBuf {
    use std::path::Component;
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// The folder `path` really is: as the system says where it is there (its links and `..`
/// followed as it follows them: `link/..` is the parent of what the link ends in); where it is
/// not, its part that is there followed through every link, the part that is not appended, its
/// `..` settled.
fn real_of(path: &Path) -> PathBuf {
    if let Ok(found) = std::fs::canonicalize(path) {
        return found;
    }
    // In the order the system walks it: each part that is there is followed through its links
    // before the next one, so a `..` after a link climbs out of what the link ends in. A part
    // that is not there is appended as it is.
    use std::path::Component;
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            Component::Normal(name) => {
                out.push(name);
                if let Ok(found) = std::fs::canonicalize(&out) {
                    out = found;
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn plain_absolute(folder: &Path) -> bool {
    folder.is_absolute()
}

/// The real path of a protected root, kept: a root is one folder for the life of the process, and
/// walking a path that is not there costs a system call for each of its parts (on Windows, a
/// good many milliseconds: the door asks this for every folder it makes).
fn real_of_root(home: &Path) -> PathBuf {
    static KEPT: OnceLock<Mutex<std::collections::HashMap<PathBuf, PathBuf>>> = OnceLock::new();
    let kept = KEPT.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Some(real) = kept
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(home)
    {
        return real.clone();
    }
    let real = real_of(home);
    kept.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(home.to_path_buf(), real.clone());
    real
}

fn safe_path_entry(folder: &Path, forbidden: &[PathBuf]) -> bool {
    // The folder's account of itself once, whatever the number of roots.
    let (settled_folder, real_folder) = (settled(folder), real_of(folder));
    !forbidden.iter().any(|home| {
        spelled_inside(folder, home)
            || spelled_inside(&settled_folder, &settled(home))
            || spelled_inside(&real_folder, &real_of_root(home))
    })
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
    use super::{drive_and_path, personal_harness_homes, safe_path_entry};
    use std::path::{Path, PathBuf};

    #[test]
    fn the_two_halves_of_a_windows_profile_path_are_of_the_home() {
        assert_eq!(
            drive_and_path(Path::new(r"C:\Users\kit\h-1\home")),
            Some(("C:".to_owned(), r"\Users\kit\h-1\home".to_owned()))
        );
        assert_eq!(
            drive_and_path(Path::new(r"\\?\D:\run\home")),
            Some(("D:".to_owned(), r"\run\home".to_owned()))
        );
        assert_eq!(drive_and_path(Path::new("/run/home")), None);
    }

    #[test]
    fn t1778_standin_home_also_bounds_macos_foundation() {
        let home = std::path::Path::new("/run-owned-home");
        let fixed = super::home_vars(home)
            .into_iter()
            .find(|(name, _)| *name == "CFFIXED_USER_HOME")
            .map(|(_, value)| value);
        assert_eq!(fixed.as_deref(), Some(home));
    }

    /// A stand-in whose case names no home is given the run's own, by the spawn door, before it
    /// could search a path with none: no program the door starts is without a home, and none has
    /// the caller's.
    #[test]
    fn standin_isolation_missing_inherited_home_refuses_before_searching_path() {
        const CHILD: &str = "CF_T2007_NO_HOME_CHILD";
        if crate::process::own_var(CHILD).is_some() {
            let home = crate::door::account::project_home();
            assert_eq!(home, None, "a child has the person's home");
            return;
        }
        let root = crate::door::made("missing-home-").unwrap();
        let path = root.path().join(".codex").join("bin");
        crate::door::make_dir(&path).unwrap();
        let mut command = crate::process::case_of(
            std::env::current_exe().unwrap(),
            "standin_process::tests::standin_isolation_missing_inherited_home_refuses_before_searching_path",
            std::time::Duration::from_secs(120),
        )
        .inheriting_env()
        .var(CHILD, "1")
        .var("PATH", &path);
        for (name, _) in super::home_vars(root.path()) {
            command = command.without(name);
        }
        let env = command.environment().unwrap();
        let home = env
            .iter()
            .find(|(name, _)| name == "HOME")
            .map(|(_, value)| PathBuf::from(value))
            .expect("the door gave no HOME");
        assert!(crate::door::is_made_or_below(&home), "{}", home.display());
        let output = command.run().unwrap();
        assert_eq!(output.code, Some(0), "{}{}", output.stdout, output.stderr);
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

    /// A stand-in told a layout of no run's is not started at all: the spawn door's seal refuses
    /// a home that is no folder this run made, before the child could look at it.
    #[test]
    fn standin_isolation_fixture_home_must_be_handed_down() {
        let temp = std::env::temp_dir();
        let foreign = temp.parent().unwrap().join("synthetic-foreign-layout");
        let refused = crate::process::case_of(
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
        .expect_err("a stand-in on a layout of no run's was started");
        assert!(
            refused
                .to_string()
                .contains("which is no folder this run made"),
            "{refused}"
        );
        assert!(!foreign.exists(), "the foreign layout was made");
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

    /// B19: the plain component check is no defence against the spellings that reach a folder
    /// by another name: a `..` that climbs out of the run, a link inside the run that ends in a
    /// harness home, and a link above one.
    #[cfg(unix)]
    #[test]
    fn b19_a_path_that_climbs_or_links_into_a_harness_home_is_not_safe_nor_inside_the_run() {
        let root = crate::door::made("path-alias-").unwrap();
        let run = root.path().join("run");
        let owner = root.path().join("owner");
        let forbidden = owner.join(".codex");
        crate::door::make_dir(&run).unwrap();
        crate::door::make_dir(&forbidden).unwrap();
        // A `..` that leaves the run for the harness home beside it.
        let climbing = run.join("home/../../owner/.codex/config.toml");
        assert!(!safe_path_entry(
            &climbing,
            std::slice::from_ref(&forbidden)
        ));
        assert!(
            !super::same_or_inside(&climbing, &run),
            "a climbing path is inside the run"
        );
        // A link in the run that ends in the home, and a path through it.
        let link = run.join("link");
        std::os::unix::fs::symlink(&forbidden, &link).unwrap();
        assert!(!safe_path_entry(
            &link.join("auth.json"),
            std::slice::from_ref(&forbidden)
        ));
        assert!(!super::same_or_inside(&link.join("auth.json"), &run));
        // `link/..` is the parent of what the link ends in, not of the link: a path that settles
        // to the run by its words and is a harness home by the system's.
        let deep = forbidden.join("sub");
        crate::door::make_dir(&deep).unwrap();
        let through = run.join("through");
        std::os::unix::fs::symlink(&deep, &through).unwrap();
        let parent_of_link = through.join("..");
        assert!(!safe_path_entry(
            &parent_of_link,
            std::slice::from_ref(&forbidden)
        ));
        assert!(!super::same_or_inside(&parent_of_link, &run));
        // A new file beside a linked folder's real parent: the leaf is not there, the parent is
        // the harness home by the system's account.
        let fresh = through.join("../fresh.json");
        assert!(!safe_path_entry(&fresh, std::slice::from_ref(&forbidden)));
        assert!(!super::same_or_inside(&fresh, &run));
        let fresh_deep = link.join("nowhere/../fresh.json");
        assert!(!safe_path_entry(
            &fresh_deep,
            std::slice::from_ref(&forbidden)
        ));
        // A link to the folder above the home: the home is below it.
        let above = run.join("above");
        std::os::unix::fs::symlink(&owner, &above).unwrap();
        assert!(!safe_path_entry(&above.join(".codex/bin"), &[forbidden]));
        // What is the run's, spelled plainly, is still its own and safe.
        let plain = run.join("home/x");
        assert!(super::same_or_inside(&plain, &run));
        assert!(safe_path_entry(&plain, &[owner.join(".codex")]));
    }

    #[test]
    fn the_protected_homes_hold_the_mac_muse_store_the_keychains_gemini_and_the_claude_file() {
        let home = Path::new(if cfg!(windows) {
            r"C:\decoy\owner"
        } else {
            "/decoy/owner"
        });
        let held = personal_harness_homes(home);
        for suffix in [
            "Library/Application Support/Muse/session-name-authority/x",
            "Library/Keychains/login.keychain-db",
            ".gemini/antigravity-cli/brain",
            ".claude.json",
        ] {
            assert!(
                !safe_path_entry(&home.join(suffix), &held),
                "{suffix} is not protected"
            );
        }
        assert!(safe_path_entry(&home.join("Library/Caches/x"), &held));
    }

    /// The door makes no folder of the run's in a place inside a protected root, whatever made
    /// the place the parent: through a link, or by a `..`.
    #[cfg(unix)]
    #[test]
    fn the_door_makes_no_folder_inside_a_protected_home() {
        let root = crate::door::made("door-protected-").unwrap();
        let held = root.path().join("owner/.claude");
        crate::door::make_dir(&held).unwrap();
        let link = root.path().join("tmp-link");
        std::os::unix::fs::symlink(&held, &link).unwrap();
        crate::door::account::TEST_PROTECTED
            .lock()
            .unwrap()
            .push(held.clone());
        let direct = crate::door::made_in(&held.join("tmp"), "h-");
        let through = crate::door::made_in(&link.join("sub/../tmp"), "h-");
        crate::door::account::TEST_PROTECTED
            .lock()
            .unwrap()
            .retain(|path| path != &held);
        for refused in [direct, through] {
            let error = refused.expect_err("a folder was made inside a protected root");
            assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        }
        assert!(!held.join("tmp").exists());
        assert!(std::fs::read_dir(&held).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_temporary_parent_inside_a_protected_home_is_refused_before_a_home_is_made() {
        let root = crate::door::made("temp-parent-").unwrap();
        let owner = root.path().join("owner");
        let held = root.path().join("owner/.codex");
        crate::door::make_dir(&held).unwrap();
        let link = root.path().join("tmp-link");
        std::os::unix::fs::symlink(&held, &link).unwrap();
        let forbidden = [held.clone()];
        assert!(super::temp_parent_is_outside(&held.join("tmp"), &forbidden).is_err());
        assert!(super::temp_parent_is_outside(&link, &forbidden).is_err());
        assert!(super::temp_parent_is_outside(&link.join("x/../y"), &forbidden).is_err());
        assert!(super::temp_parent_is_outside(&owner.join("elsewhere"), &forbidden).is_ok());
    }

    /// B19, on Windows: a verbatim spelling is the same folder as its plain one, and a
    /// folder reached by its 8.3 short name is the folder.
    #[cfg(windows)]
    #[test]
    fn b19_a_verbatim_spelling_of_a_harness_home_is_that_home() {
        let root = crate::door::made("path-verbatim-").unwrap();
        let forbidden = root.path().join("owner").join(".codex");
        crate::door::make_dir(&forbidden).unwrap();
        let plain = forbidden.join("bin");
        let verbatim = std::path::PathBuf::from(format!(r"\\?\{}", plain.display()));
        assert!(!safe_path_entry(&plain, std::slice::from_ref(&forbidden)));
        assert!(
            !safe_path_entry(&verbatim, std::slice::from_ref(&forbidden)),
            "the verbatim spelling was taken for another folder"
        );
    }
}
