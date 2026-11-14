//! Homes for a stand-in test process, established before libtest makes threads.

use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::door::Made;

static ROOT: OnceLock<Mutex<Option<Made>>> = OnceLock::new();
static FIXTURE_ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
static BUILD_COMPILER: OnceLock<Option<PathBuf>> = OnceLock::new();
static INHERITED_HARNESS_HOMES: OnceLock<Vec<PathBuf>> = OnceLock::new();

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

    let fixture_home = std::env::var_os(FIXTURE_HOME).map(PathBuf::from);
    std::env::remove_var(FIXTURE_HOME);
    let (path, compiler, inherited_homes) = bounded_path();
    BUILD_COMPILER
        .set(compiler)
        .unwrap_or_else(|_| panic!("stand-in compiler was selected twice"));
    INHERITED_HARNESS_HOMES
        .set(inherited_homes)
        .unwrap_or_else(|_| panic!("stand-in inherited homes were recorded twice"));

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
            "stand-in handed folder lookup: forbidden, linked or ambiguous path"
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
        "stand-in audit file lookup: forbidden, linked or ambiguous path"
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
        "stand-in fixture home lookup: invalid or linked layout"
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

/// A stand-in must not search inherited PATH entries in a harness home. Do
/// this before replacing the variables that name the person's directories.
#[allow(clippy::disallowed_methods)] // Read the inherited paths before replacing the environment.
fn bounded_path() -> (std::ffi::OsString, Option<PathBuf>, Vec<PathBuf>) {
    let mut forbidden: Vec<PathBuf> = [
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "PI_CODING_AGENT_DIR",
        "PI_CODING_AGENT_SESSION_DIR",
    ]
    .into_iter()
    .filter_map(std::env::var_os)
    .filter(|value| !value.is_empty())
    .filter_map(|value| std::path::absolute(value).ok())
    .collect();
    for name in ["HOME", "USERPROFILE"] {
        if let Some(home) = std::env::var_os(name).filter(|value| !value.is_empty()) {
            let home = PathBuf::from(home);
            for suffix in [
                ".claude",
                ".codex",
                ".pi",
                ".config/muse",
                ".config/devin",
                ".local/share/devin",
            ] {
                if let Ok(path) = std::path::absolute(home.join(suffix)) {
                    forbidden.push(path);
                }
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
        if let Some(base) = std::env::var_os(name).filter(|value| !value.is_empty()) {
            for harness in ["claude", "codex", "pi", "devin", "muse"] {
                if let Ok(path) = std::path::absolute(Path::new(&base).join(harness)) {
                    forbidden.push(path);
                }
            }
        }
    }
    // Cargo gives its own absolute executable to a test at compile time. Its
    // compiler is normally beside it, even if a runner's PATH reaches rustc
    // through a link that the stand-in cannot safely keep. Check every part
    // before stat'ing the file; never follow a link into a harness home.
    let cargo = Path::new(env!("CARGO"));
    let compiler = cargo.with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    let compiler = (safe_path_entry(&compiler, &forbidden)
        && std::fs::symlink_metadata(&compiler).is_ok_and(|entry| entry.is_file()))
    .then_some(compiler);
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let safe =
        std::env::split_paths(&inherited).filter(|folder| safe_path_entry(folder, &forbidden));
    let path =
        std::env::join_paths(safe).unwrap_or_else(|_| panic!("stand-in PATH lookup: invalid path"));
    (path, compiler, forbidden)
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

/// Refuse a rig's PATH override before a child can search an inherited harness home.
pub(crate) fn require_search_path(value: &str, lookup: &str, system: &[String]) {
    if crate::process::own_var("CF_E2E_STAND_IN").as_deref() != Some("1") {
        return;
    }
    let forbidden = INHERITED_HARNESS_HOMES
        .get()
        .unwrap_or_else(|| panic!("{lookup}: no inherited-home boundary"));
    for entry in std::env::split_paths(value) {
        assert!(
            safe_path_entry(&entry, forbidden),
            "{lookup}: forbidden, linked, relative or ambiguous directory"
        );
        let system_folder = system.iter().any(|folder| {
            let folder = Path::new(folder);
            same_or_inside(&entry, folder) && same_or_inside(folder, &entry)
        });
        if !system_folder {
            // An arbitrary directory can itself contain a launcher link into
            // an inherited home. Only the run, its built tools, and the named
            // hosted Muse are allowed besides the system's command folders.
            require_watch_path(&entry, lookup);
        }
    }
}

fn plain_absolute(folder: &Path) -> bool {
    if !folder.is_absolute()
        || folder
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return false;
    }
    // A verbatim, UNC or DOS short-name path may spell a different inherited
    // home without looking like it lexically. Do not inspect it at all.
    #[cfg(windows)]
    if folder.components().any(|part| match part {
        Component::Prefix(prefix) => !matches!(prefix.kind(), std::path::Prefix::Disk(_)),
        Component::Normal(name) => name.to_string_lossy().contains('~'),
        _ => false,
    }) {
        return false;
    }
    true
}

fn same_or_inside(folder: &Path, home: &Path) -> bool {
    #[cfg(windows)]
    {
        let folder = folder.to_string_lossy().replace('/', "\\");
        let home = home.to_string_lossy().replace('/', "\\");
        let home = home
            .strip_prefix(r"\\?\")
            .unwrap_or(&home)
            .trim_end_matches('\\');
        let folder: Vec<u16> = folder.encode_utf16().collect();
        let home: Vec<u16> = home.encode_utf16().collect();
        folder.get(..home.len()).is_some_and(|prefix| {
            windows_ordinal_eq(prefix, &home)
                && (folder.len() == home.len() || folder.get(home.len()) == Some(&(b'\\' as u16)))
        })
    }
    #[cfg(target_os = "macos")]
    {
        let normalizer = icu_normalizer::DecomposingNormalizerBorrowed::new_nfd();
        let folder_text = folder.to_string_lossy();
        let home_text = home.to_string_lossy();
        let folder_fold = icu_casemap::CaseMapperBorrowed::new().fold_string(&folder_text);
        let home_fold = icu_casemap::CaseMapperBorrowed::new().fold_string(&home_text);
        let folder = normalizer.normalize(&folder_fold);
        let home = normalizer.normalize(&home_fold);
        folder == home || folder.starts_with(&format!("{}/", home.trim_end_matches('/')))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        folder.starts_with(home)
    }
}

fn safe_path_entry(folder: &Path, forbidden: &[PathBuf]) -> bool {
    if !plain_absolute(folder) || forbidden.iter().any(|home| same_or_inside(folder, home)) {
        return false;
    }
    let mut checked = PathBuf::new();
    for part in folder.components() {
        checked.push(part);
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        if std::fs::symlink_metadata(&checked).is_ok_and(|entry| {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                entry.file_type().is_symlink() || entry.file_attributes() & 0x400 != 0
            }
            #[cfg(not(windows))]
            {
                entry.file_type().is_symlink()
            }
        }) {
            return false;
        }
    }
    true
}

/// The compiler of the Cargo that built this test, if its complete path was
/// safe to inspect before the inherited homes were discarded.
pub(crate) fn build_compiler() -> Option<&'static Path> {
    BUILD_COMPILER.get().and_then(Option::as_deref)
}

/// Windows normally puts TEMP inside the person's profile. Use the public
/// scratch directory for the first process, then nest children in the folder
/// their parent made. Refuse an unsafe or linked parent before making a home.
#[allow(clippy::disallowed_methods)] // This is the inherited environment before test threads.
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
        // macOS spells its system temp path through /var, a known system
        // link. Use its real spelling before checking every component.
        if std::env::var_os("GITHUB_ACTIONS").as_deref() == Some(std::ffi::OsStr::new("true"))
            && std::env::var_os("CF_E2E_STAND_IN").is_none()
        {
            PathBuf::from("/private/tmp")
        } else if let Ok(rest) = temp.strip_prefix("/var") {
            Path::new("/private/var").join(rest)
        } else if let Ok(rest) = temp.strip_prefix("/tmp") {
            Path::new("/private/tmp").join(rest)
        } else {
            temp
        }
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let parent = if std::env::var_os("CF_E2E_STAND_IN").is_some() {
        std::env::temp_dir()
    } else {
        PathBuf::from("/tmp")
    };
    let parent = std::path::absolute(&parent)
        .unwrap_or_else(|_| panic!("stand-in temporary lookup: invalid path"));
    assert!(
        plain_absolute(&parent),
        "stand-in temporary lookup: aliased or invalid path"
    );
    for name in ["HOME", "USERPROFILE"] {
        if let Some(home) = std::env::var_os(name).filter(|home| !home.is_empty()) {
            let home = std::path::absolute(home)
                .unwrap_or_else(|_| panic!("stand-in home lookup: invalid path"));
            assert!(
                !same_or_inside(&parent, &home),
                "stand-in temporary lookup: inside the inherited {name}"
            );
        }
    }
    let mut checked = PathBuf::new();
    for part in parent.components() {
        checked.push(part);
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        if std::fs::symlink_metadata(&checked).is_ok_and(|entry| {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                entry.file_type().is_symlink() || entry.file_attributes() & 0x400 != 0
            }
            #[cfg(not(windows))]
            {
                entry.file_type().is_symlink()
            }
        }) {
            panic!("stand-in temporary lookup: linked parent");
        }
    }
    parent
}

/// Refuse a stand-in lookup before examining a path outside this process's
/// home. A link inside it is refused too, before following it anywhere.
#[allow(clippy::disallowed_methods)]
pub fn require_home(path: &Path, lookup: &str) {
    if std::env::var_os("CF_E2E_STAND_IN").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let root = ROOT
        .get()
        .unwrap_or_else(|| panic!("stand-in lookup has no process home"));
    let root = root.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = root
        .as_ref()
        .unwrap_or_else(|| panic!("stand-in lookup after home cleanup"));
    let absolute = std::path::absolute(path).unwrap_or_else(|_| PathBuf::from(path));
    let base = if absolute.starts_with(root.path()) {
        root.path()
    } else {
        FIXTURE_ROOT
            .get()
            .and_then(Option::as_deref)
            .filter(|fixture| absolute.starts_with(fixture))
            .unwrap_or_else(|| panic!("{lookup}: outside the stand-in run"))
    };
    let suffix = absolute
        .strip_prefix(base)
        .unwrap_or_else(|_| panic!("{lookup}: invalid root"));
    let mut checked = base.to_path_buf();
    for part in suffix.components() {
        match part {
            Component::Normal(name) => checked.push(name),
            _ => panic!("{lookup}: path leaves the stand-in run"),
        }
        if std::fs::symlink_metadata(&checked).is_ok_and(|entry| {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                entry.file_type().is_symlink() || entry.file_attributes() & 0x400 != 0
            }
            #[cfg(not(windows))]
            {
                entry.file_type().is_symlink()
            }
        }) {
            panic!("{lookup}: link in a stand-in home");
        }
    }
}

/// Check each link before a launcher watch follows it. The only additional
/// places a stand-in may watch are the test's built binaries and the pinned
/// Muse installed on a disposable hosted runner.
#[allow(clippy::disallowed_methods)] // The process mode is set before test threads.
pub fn require_watch_path(path: &Path, lookup: &str) {
    if std::env::var_os("CF_E2E_STAND_IN").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let root = ROOT
        .get()
        .unwrap_or_else(|| panic!("stand-in lookup has no process home"));
    let root = root.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = root
        .as_ref()
        .unwrap_or_else(|| panic!("stand-in lookup after home cleanup"));
    let mut roots = vec![root.path().to_path_buf()];
    if let Some(Some(fixture)) = FIXTURE_ROOT.get() {
        roots.push(fixture.clone());
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(target) = exe.parent().and_then(Path::parent) {
            roots.push(target.to_path_buf());
        }
    }
    if std::env::var_os("GITHUB_ACTIONS").as_deref() == Some(std::ffi::OsStr::new("true"))
        && std::env::var_os("CF_E2E_HOSTED_MUSE").as_deref() == Some(std::ffi::OsStr::new("1"))
    {
        if let Some(temp) = std::env::var_os("RUNNER_TEMP") {
            roots.push(PathBuf::from(temp).join("muse-echo"));
        }
    }
    let normalize = |path: &Path| {
        let absolute =
            std::path::absolute(path).unwrap_or_else(|_| panic!("{lookup}: invalid path"));
        let mut clean = PathBuf::new();
        for part in absolute.components() {
            match part {
                Component::ParentDir => {
                    assert!(clean.pop(), "{lookup}: path leaves the stand-in run");
                }
                Component::CurDir => {}
                other => clean.push(other.as_os_str()),
            }
        }
        clean
    };
    let mut pending = normalize(path);
    for _ in 0..40 {
        let base = roots
            .iter()
            .filter(|base| pending.starts_with(base))
            .max_by_key(|base| base.components().count())
            .unwrap_or_else(|| panic!("{lookup}: outside the stand-in run"));
        let suffix: Vec<_> = pending
            .strip_prefix(base)
            .unwrap_or_else(|_| panic!("{lookup}: invalid root"))
            .components()
            .collect();
        let mut checked = base.to_path_buf();
        let mut followed = false;
        for (index, part) in suffix.iter().enumerate() {
            checked.push(part.as_os_str());
            let Ok(entry) = std::fs::symlink_metadata(&checked) else {
                break;
            };
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                assert!(
                    entry.file_attributes() & 0x400 == 0 || entry.file_type().is_symlink(),
                    "{lookup}: unsupported reparse point"
                );
            }
            if entry.file_type().is_symlink() {
                let target = std::fs::read_link(&checked)
                    .unwrap_or_else(|_| panic!("{lookup}: unreadable link"));
                let mut next = if target.is_absolute() {
                    target
                } else {
                    checked
                        .parent()
                        .unwrap_or_else(|| panic!("{lookup}: invalid link"))
                        .join(target)
                };
                for remaining in &suffix[index + 1..] {
                    next.push(remaining.as_os_str());
                }
                pending = normalize(&next);
                followed = true;
                break;
            }
        }
        if !followed {
            return;
        }
    }
    panic!("{lookup}: too many links");
}

#[cfg(test)]
mod tests {
    use super::safe_path_entry;

    #[test]
    fn standin_isolation_unowned_audit_file_is_refused_before_it_is_opened() {
        let temp = std::env::temp_dir();
        let foreign = temp.parent().unwrap().join("synthetic-foreign-audit-file");
        let output = crate::process::Run::new(std::env::current_exe().unwrap())
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
        let output = crate::process::Run::new(std::env::current_exe().unwrap())
            .inheriting_env()
            .args([
                "--exact",
                "standin_process::tests::standin_isolation_fixture_home_must_be_handed_down",
            ])
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

        #[cfg(unix)]
        {
            let link = root.path().join("linked-toolchain");
            crate::door::symlink(&forbidden, &link).unwrap();
            assert!(!safe_path_entry(
                &link.join("rustc"),
                std::slice::from_ref(&forbidden)
            ));
        }
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

    #[cfg(target_os = "macos")]
    #[test]
    fn standin_isolation_path_refuses_unicode_aliases() {
        let root = crate::door::made("path-unicode-").unwrap();
        let forbidden = root.path().join("synthetic-\u{00e9}-owner/.codex");
        crate::door::make_dir(&forbidden).unwrap();
        let alias = root.path().join("synthetic-e\u{0301}-owner/.codex/bin");
        assert!(!safe_path_entry(&alias, &[forbidden]));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn standin_isolation_path_refuses_unicode_casefold_aliases() {
        let root = crate::door::made("path-casefold-").unwrap();
        for (owner, alias) in [
            ("synthetic-Stra\u{00df}e-owner", "synthetic-STRASSE-owner"),
            ("synthetic-\u{03a3}-owner", "synthetic-\u{03c2}-owner"),
        ] {
            let forbidden = root.path().join(owner).join(".codex");
            crate::door::make_dir(&forbidden).unwrap();
            let alias = root.path().join(alias).join(".codex/bin");
            assert!(!safe_path_entry(&alias, &[forbidden]));
        }
    }

    #[cfg(windows)]
    #[test]
    fn standin_isolation_path_refuses_windows_aliases() {
        let root = crate::door::made("path-alias-").unwrap();
        let forbidden = root.path().join("synthetic-owner/.codex");
        crate::door::make_dir(&forbidden).unwrap();
        let extended =
            std::path::PathBuf::from(format!(r"\\?\{}", forbidden.display())).join("bin");
        assert!(!safe_path_entry(
            &extended,
            std::slice::from_ref(&forbidden)
        ));
        let extended_home = std::path::PathBuf::from(format!(r"\\?\{}", forbidden.display()));
        assert!(!safe_path_entry(&forbidden.join("bin"), &[extended_home]));
        let shortened = std::path::Path::new(r"C:\synthetic\OWNER~1\.codex\bin");
        assert!(!safe_path_entry(shortened, &[forbidden]));
    }
}
