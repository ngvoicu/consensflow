//! A file dropped on the app's window, as the app hears it itself (`hear`): only
//! on the window the page is in, `main`. Each drop is given a number and told to
//! the page by `DROP_EVENT`, as Tauri tells its own (`tauri://drag-drop`: the
//! paths, and where the pointer is), with that number; the app keeps the last
//! `PENDING`, which the page types one after the other, and the page asks of a
//! drop by its number, never of a path it names on its own:
//!
//! - the short names (8.3) Windows keeps for its paths (`short_paths`). A Codex
//!   window on Windows drops a character outside the Basic Multilingual Plane
//!   typed into it (`untakeable` in `app/ui/core/file-drop.js`), so the page types
//!   a file dropped on one by its short name where the long one holds such a
//!   character. A path that was not one of that drop's is
//!   refused, and so is one that is no whole path to a file or a folder on a
//!   drive or a share; at most `MOST` are looked up at once, one lookup at a
//!   time; a path past Windows' old limit is asked in its long form (`\\?\`);
//!   and each failure is said in words of its own, which the page shows.
//! - the app's window in front once the page has typed them (`focus_after_drop`):
//!   once for each drop, and only while the window in front is still the one the
//!   file came from, so a human who has gone on to another program is left
//!   there. Off Windows, never: a Mac leaves the app where it is, as Terminal.app
//!   does.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::{json, Value};
use tauri::{
    AppHandle, DragDropEvent, Emitter, EventTarget, Manager, PhysicalPosition, Runtime,
    WebviewWindow, Window, WindowEvent,
};

/// The event a drop is told to the page by: `{id, paths, position}`.
pub const DROP_EVENT: &str = "file-drop";
/// The window the page is in, the one drop the app hears.
const MAIN: &str = "main";
/// The most drops the app keeps for the page to ask of: the oldest goes as another comes.
const PENDING: usize = 16;
/// The most paths one lookup takes.
const MOST: usize = 64;
/// The longest path Windows takes in its old form, its NUL left out.
const OLD_LONGEST: usize = 259;
/// The longest path Windows takes at all, in UTF-16 units.
const LONGEST: usize = 32_767;
/// What is said of a path that names a device.
const DEVICE: &str = "it names a device, not a file";
/// What is said of a path that is not whole.
const NOT_WHOLE: &str = "it is no whole path to a file";

/// The drops on the app's window the page may ask of, as the window was told of them.
#[derive(Clone, Default)]
pub struct Dropped(Arc<Shared>);

#[derive(Default)]
struct Shared {
    drops: Mutex<Drops>,
    /// Whether a lookup is running.
    busy: AtomicBool,
}

#[derive(Default)]
struct Drops {
    /// The number the last drop was given; the first is 1.
    last: u64,
    /// The last `PENDING` drops, the oldest first.
    kept: VecDeque<Heard>,
}

/// One drop, as the app heard it.
struct Heard {
    id: u64,
    /// Its paths, those that are text.
    paths: Vec<String>,
    /// The window in front as it was dropped, the program it came from; none off Windows,
    /// and none once the page has brought the app's window in front for it.
    from: Option<usize>,
}

/// Hears a file dropped on the app's window, `main`, and tells the page of it with
/// the number it is kept by: a drop of no file (text from another program) is told
/// with none, and kept by none. The drag's other events are no drop, and a drop on
/// any other window is not heard.
pub fn hear<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let WindowEvent::DragDrop(DragDropEvent::Drop { paths, position }) = event else {
        return;
    };
    if window.label() != MAIN {
        return;
    }
    let Some(dropped) = window.try_state::<Dropped>() else {
        return;
    };
    let (id, paths) = dropped.record(paths, foreground());
    let _ = window.emit_to(
        EventTarget::labeled(MAIN),
        DROP_EVENT,
        told(id, &paths, position),
    );
}

/// What the page is told of a drop: its number, its paths, and where it was, as
/// Tauri says it (`{x, y}`).
fn told(id: Option<u64>, paths: &[String], position: &PhysicalPosition<f64>) -> Value {
    json!({"id": id, "paths": paths, "position": {"x": position.x, "y": position.y}})
}

impl Dropped {
    /// Keeps a drop of `paths`, those that are text, and answers its number and them:
    /// none for a drop of no path, which is not kept.
    fn record(&self, paths: &[PathBuf], from: Option<usize>) -> (Option<u64>, Vec<String>) {
        let paths: Vec<String> = paths
            .iter()
            .filter_map(|path| path.to_str())
            .map(str::to_owned)
            .collect();
        if paths.is_empty() {
            return (None, paths);
        }
        let mut drops = self.drops();
        drops.last += 1;
        let id = drops.last;
        if drops.kept.len() == PENDING {
            drops.kept.pop_front();
        }
        drops.kept.push_back(Heard {
            id,
            paths: paths.clone(),
            from,
        });
        (Some(id), paths)
    }

    fn drops(&self) -> MutexGuard<'_, Drops> {
        self.0.drops.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The paths of the drop numbered `id`, if it is kept.
    fn paths_of(&self, id: u64) -> Option<Vec<String>> {
        let drops = self.drops();
        let heard = drops.kept.iter().find(|heard| heard.id == id)?;
        Some(heard.paths.clone())
    }

    /// The lookup's turn, unless another one has it.
    fn turn(&self) -> Option<Turn> {
        (!self.0.busy.swap(true, Ordering::AcqRel)).then(|| Turn(self.clone()))
    }

    /// The window the drop numbered `id` came from, once: after it, none.
    fn take_from(&self, id: u64) -> Option<usize> {
        let mut drops = self.drops();
        let heard = drops.kept.iter_mut().find(|heard| heard.id == id)?;
        heard.from.take()
    }
}

/// A lookup's turn, given up when it ends, however it ends.
struct Turn(Dropped);

impl Drop for Turn {
    fn drop(&mut self) {
        self.0 .0.busy.store(false, Ordering::Release);
    }
}

impl Dropped {
    /// The short name of each of `paths` of the drop numbered `id`, as `short_name`
    /// reads it, in order (`{"short": …}`), or why it has none (`{"error": …}`). Off the
    /// main thread: a path on a network share may take a while. The lookup's turn is
    /// the task's that reads them, and goes back when that task ends, whether or not
    /// anyone still waits for it (a page reloaded while Windows read a far share); a
    /// lookup asked meanwhile is refused as `busy`, which the page asks again.
    async fn looked_up<F>(self, id: u64, paths: Vec<String>, short_name: F) -> Value
    where
        F: Fn(&str) -> Result<String, String> + Send + 'static,
    {
        if paths.len() > MOST {
            return json!({"ok":false,"error":format!("the app looks up at most {MOST} short names at once")});
        }
        let Some(dropped) = self.paths_of(id) else {
            return json!({"ok":false,"error":format!("it is not one of the last {PENDING} drops the app heard")});
        };
        let Some(turn) = self.turn() else {
            return json!({"ok":false,"busy":true,"error":"another lookup of short names is running"});
        };
        tauri::async_runtime::spawn_blocking(move || {
            let answered = answer(&paths, &dropped, &short_name);
            drop(turn);
            answered
        })
        .await
        .unwrap_or_else(
            |error| json!({"ok":false,"error":format!("the short names were not read: {error}")}),
        )
    }
}

/// The short name of each of `paths`, or why it has none, in order: of the paths
/// `dropped`, the drop's they were asked of, each as `short_name` reads it.
fn answer(
    paths: &[String],
    dropped: &[String],
    short_name: &impl Fn(&str) -> Result<String, String>,
) -> Value {
    let answered: Vec<Value> = paths
        .iter()
        .map(|path| match look_up(path, dropped, short_name) {
            Ok(short) => json!({ "short": short }),
            Err(why) => json!({ "error": why }),
        })
        .collect();
    json!({"ok":true,"paths":answered})
}

/// The short name of each of `paths` of the drop numbered `id`, as Windows keeps it
/// (`Dropped::looked_up`).
#[tauri::command]
pub async fn short_paths<R: Runtime>(app: AppHandle<R>, id: u64, paths: Vec<String>) -> Value {
    match app.try_state::<Dropped>() {
        Some(dropped) => {
            dropped
                .inner()
                .clone()
                .looked_up(id, paths, short_name)
                .await
        }
        None => json!({"ok":false,"error":"the app hears no drops"}),
    }
}

/// Brings the app's window in front after the page has typed the paths of the drop
/// numbered `id`: once, and only while the window in front is the one the file came from.
#[tauri::command]
pub fn focus_after_drop<R: Runtime>(window: WebviewWindow<R>, id: u64) -> Value {
    let Some(dropped) = window.try_state::<Dropped>() else {
        return json!({"ok":false,"error":"the app hears no drops"});
    };
    if !may_focus(dropped.take_from(id), foreground()) {
        return json!({"ok":true,"focused":false});
    }
    match window.set_focus() {
        Ok(()) => json!({"ok":true,"focused":true}),
        Err(error) => json!({"ok":false,"error":error.to_string()}),
    }
}

/// Whether the app's window may come in front: the window in front now is the one
/// the file came from, the human having gone nowhere since.
fn may_focus(from: Option<usize>, now: Option<usize>) -> bool {
    from.is_some() && from == now
}

/// The short name of `path`, one of the paths `dropped`, as `short_name` reads it, or
/// why it has none.
fn look_up(
    path: &str,
    dropped: &[String],
    short_name: &impl Fn(&str) -> Result<String, String>,
) -> Result<String, String> {
    if let Some(why) = malformed(path) {
        return Err(why.to_owned());
    }
    if !dropped.iter().any(|one| one == path) {
        return Err("it was not dropped on the app".to_owned());
    }
    let asked = long_form(path);
    let short = short_name(&asked)?;
    if short == asked {
        return Err("Windows keeps no short name for it".to_owned());
    }
    Ok(old_form(&short, asked != path))
}

/// Why `path` is no path the app looks up: none for a whole path to a file or a
/// folder on a drive (`C:\…`) or a share (`\\server\share\…`), in its long form
/// (`\\?\`) or not.
fn malformed(path: &str) -> Option<&'static str> {
    if path.chars().any(|c| c < ' ' || c == '\u{7f}') {
        return Some("its name holds a control character");
    }
    if path.encode_utf16().count() > LONGEST {
        return Some("it is longer than any Windows path");
    }
    let Some(below) = below_root(path) else {
        return Some(if path.starts_with(r"\\.\") || path.starts_with(r"\\?\") {
            DEVICE
        } else {
            NOT_WHOLE
        });
    };
    let mut names = below.split('\\').peekable();
    while let Some(name) = names.next() {
        // A folder's path may end with its separator, and a drive's root is one.
        if name.is_empty() && names.peek().is_none() {
            break;
        }
        if name.is_empty() || name == "." || name == ".." {
            return Some(NOT_WHOLE);
        }
        if name.contains(['<', '>', ':', '"', '|', '?', '*', '/']) {
            return Some("its name holds a character no Windows name has");
        }
        if is_device(name) {
            return Some(DEVICE);
        }
    }
    None
}

/// What is below the root of `path`, a whole path on a drive or a share; none for any other.
fn below_root(path: &str) -> Option<&str> {
    if let Some(share) = path.strip_prefix(r"\\?\UNC\") {
        return below_share(share);
    }
    if let Some(drive) = path.strip_prefix(r"\\?\") {
        return below_drive(drive);
    }
    match path.strip_prefix(r"\\") {
        Some(share) => below_share(share),
        None => below_drive(path),
    }
}

/// `C:\…`: what is below the drive's root.
fn below_drive(path: &str) -> Option<&str> {
    let bytes = path.as_bytes();
    let rooted =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    rooted.then(|| &path[3..])
}

/// `server\share\…`: what is below the share, which a server and a share name.
fn below_share(path: &str) -> Option<&str> {
    let mut parts = path.splitn(3, '\\');
    parts
        .next()
        .filter(|server| !matches!(*server, "" | "." | "?"))?;
    parts.next().filter(|share| !share.is_empty())?;
    Some(parts.next().unwrap_or(""))
}

/// Whether `name` is one Windows takes for a device wherever it is (`NUL`, `COM1.txt`).
fn is_device(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let stem = stem.to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|port| {
        stem.strip_prefix(port).is_some_and(|number| {
            matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
                || matches!(number, "\u{b9}" | "\u{b2}" | "\u{b3}")
        })
    })
}

/// `path` as Windows takes it past its old limit: in its long form (`\\?\C:\…`,
/// `\\?\UNC\server\share\…`); as it is where it is short enough, or long already.
fn long_form(path: &str) -> String {
    if path.starts_with(r"\\?\") || path.encode_utf16().count() <= OLD_LONGEST {
        return path.to_owned();
    }
    match path.strip_prefix(r"\\") {
        Some(share) => format!(r"\\?\UNC\{share}"),
        None => format!(r"\\?\{path}"),
    }
}

/// A short name Windows answered for a path asked in the long form the app put it
/// in (`added`): in its old form again where that is short enough.
fn old_form(short: &str, added: bool) -> String {
    if !added {
        return short.to_owned();
    }
    let old = match short.strip_prefix(r"\\?\UNC\") {
        Some(share) => format!(r"\\{share}"),
        None => short.strip_prefix(r"\\?\").unwrap_or(short).to_owned(),
    };
    if old.encode_utf16().count() <= OLD_LONGEST {
        old
    } else {
        short.to_owned()
    }
}

/// Why Windows answered no short name, by the error it gave; tested everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
fn reason(code: u32) -> String {
    match code {
        // ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND.
        2 | 3 => "it is not there any more".to_owned(),
        // ERROR_ACCESS_DENIED.
        5 => "Windows refused access to it".to_owned(),
        // The share or its server did not answer: ERROR_REM_NOT_LIST, ERROR_BAD_NETPATH,
        // ERROR_UNEXP_NET_ERR, ERROR_NETNAME_DELETED, ERROR_NETWORK_ACCESS_DENIED,
        // ERROR_BAD_NET_NAME, ERROR_SEM_TIMEOUT, ERROR_NO_NET_OR_BAD_PATH, ERROR_NO_NETWORK,
        // ERROR_NETWORK_UNREACHABLE, ERROR_HOST_UNREACHABLE, ERROR_NOT_CONNECTED.
        51 | 53 | 59 | 64 | 65 | 67 | 121 | 1203 | 1222 | 1231 | 1232 | 2250 => {
            "its network share did not answer".to_owned()
        }
        // ERROR_FILENAME_EXCED_RANGE.
        206 => "its path is longer than Windows reads".to_owned(),
        _ => format!("Windows answered error {code} for it"),
    }
}

/// What Windows answers for `asked`: its short name (each folder and the file
/// by the one it keeps, the others as they are), or why it answered none.
#[cfg(windows)]
fn short_name(asked: &str) -> Result<String, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

    let long: Vec<u16> = std::ffi::OsStr::new(asked)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `long` is a NUL-terminated wide string that outlives the call;
    // with no buffer, Windows answers the length the short name takes, its NUL too.
    let needed = unsafe { GetShortPathNameW(long.as_ptr(), std::ptr::null_mut(), 0) };
    if needed == 0 {
        // SAFETY: reads the error the call before left on this thread.
        return Err(reason(unsafe { GetLastError() }));
    }
    let mut short = vec![0_u16; usize::try_from(needed).unwrap_or(0)];
    // SAFETY: `short` holds `needed` units, which Windows writes the name and its
    // NUL into, and answers how many it wrote less the NUL.
    let written = unsafe { GetShortPathNameW(long.as_ptr(), short.as_mut_ptr(), needed) };
    if written == 0 {
        // SAFETY: as above.
        return Err(reason(unsafe { GetLastError() }));
    }
    if written >= needed {
        return Err("its name changed while it was read".to_owned());
    }
    short.truncate(usize::try_from(written).unwrap_or(0));
    String::from_utf16(&short).map_err(|_| "its short name is no text".to_owned())
}

#[cfg(not(windows))]
fn short_name(_asked: &str) -> Result<String, String> {
    Err("there are no short names off Windows".to_owned())
}

/// The window in front now, as a number: none off Windows.
#[cfg(windows)]
fn foreground() -> Option<usize> {
    // SAFETY: takes nothing, and answers a window's handle or null.
    let window = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    (!window.is_null()).then_some(window as usize)
}

#[cfg(not(windows))]
fn foreground() -> Option<usize> {
    None
}

#[cfg(test)]
mod tests;
