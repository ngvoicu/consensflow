#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::{App, Listener, PhysicalPosition, WebviewWindowBuilder};

use super::*;

/// The page's origin, as the app's webview has it on this system: the app answers a
/// page of any other as a remote one.
const ORIGIN: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

const DASHED: &str = r"C:\Users\me\a — b.png";
/// What a path of a drop is answered where it is looked up: off Windows there are no
/// short names, and on Windows there is no such file.
const LOOKED_UP: &str = if cfg!(windows) {
    "it is not there any more"
} else {
    "there are no short names off Windows"
};
const NOT_DROPPED: &str = "it was not dropped on the app";
const NOT_KEPT: &str = "it is not one of the last 16 drops the app heard";

/// A handler of the events of the app's windows.
type WindowEvents = Arc<dyn Fn(&Window<MockRuntime>, &WindowEvent) + Send + Sync>;

/// A builder of the stand-in runtime, given what the app's own is given for its page
/// (`crate::with_page`), which keeps the handlers of its windows' events: the stand-in
/// runtime tells a window no event, so the tests tell them as Tauri's would.
struct Recording {
    builder: tauri::Builder<MockRuntime>,
    window_events: Vec<WindowEvents>,
}

impl crate::PageBuilder<MockRuntime> for Recording {
    fn manage<T: Send + Sync + 'static>(self, state: T) -> Self {
        Self {
            builder: self.builder.manage(state),
            ..self
        }
    }

    fn on_window_event<F>(mut self, handler: F) -> Self
    where
        F: Fn(&Window<MockRuntime>, &WindowEvent) + Send + Sync + 'static,
    {
        self.window_events.push(Arc::new(handler));
        self
    }

    fn invoke_handler<F>(self, handler: F) -> Self
    where
        F: Fn(tauri::ipc::Invoke<MockRuntime>) -> bool + Send + Sync + 'static,
    {
        Self {
            builder: self.builder.invoke_handler(handler),
            ..self
        }
    }
}

/// The app as it is built for its page (`crate::with_page`), on the stand-in runtime:
/// what it manages for drops, the handlers of its windows' events, the commands the
/// page may ask, its main window, and what the page is told of each drop.
struct Rig {
    app: App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
    window_events: Vec<WindowEvents>,
    told: Arc<Mutex<Vec<Value>>>,
}

fn app() -> Rig {
    let Recording {
        builder,
        window_events,
    } = crate::with_page(Recording {
        builder: mock_builder(),
        window_events: Vec::new(),
    });
    let app = builder
        .build(mock_context(noop_assets()))
        .expect("build mock app");
    let window = WebviewWindowBuilder::new(&app, MAIN, Default::default())
        .build()
        .expect("build mock window");
    let told = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&told);
    app.listen_any(DROP_EVENT, move |event| {
        let drop: Value = serde_json::from_str(event.payload()).expect("a drop as JSON");
        heard.lock().expect("what the page was told").push(drop);
    });
    Rig {
        app,
        window,
        window_events,
        told,
    }
}

/// Files dropped at `paths`, at the top left.
fn dropping(paths: &[&str]) -> DragDropEvent {
    DragDropEvent::Drop {
        paths: paths.iter().map(PathBuf::from).collect(),
        position: PhysicalPosition::new(0.0, 0.0),
    }
}

impl Rig {
    /// The main window is told of `event`, as Tauri tells the handlers its builder was given.
    fn tell(&self, event: DragDropEvent) {
        self.tell_to(&self.window.as_ref().window(), event);
    }

    /// `window` is told of `event`, as Tauri tells the handlers its builder was given.
    fn tell_to(&self, window: &Window<MockRuntime>, event: DragDropEvent) {
        let event = WindowEvent::DragDrop(event);
        for handler in &self.window_events {
            handler(window, &event);
        }
    }

    /// What the page has been told of drops, in order.
    fn told(&self) -> Vec<Value> {
        self.told.lock().expect("what the page was told").clone()
    }

    /// Files dropped on the window, at `paths`: the number the page is told the drop by.
    fn drop_on(&self, paths: &[&str]) -> u64 {
        let before = self.told().len();
        self.tell(dropping(paths));
        let told = self.told();
        assert_eq!(told.len(), before + 1, "the page is told of the drop");
        told[before]["id"]
            .as_u64()
            .expect("a drop told by its number")
    }

    /// What the page is answered when it asks `command` with `args`, over the app's IPC.
    fn ask(&self, command: &str, args: Value) -> Value {
        tauri::test::get_ipc_response(
            &self.window,
            tauri::webview::InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: ORIGIN.parse().expect("invoke URL"),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
        .expect("an answer")
        .deserialize::<Value>()
        .expect("JSON")
    }

    /// The short names the page is answered for `paths` of the drop numbered `id`.
    fn short_paths_of(&self, id: u64, paths: &[&str]) -> Value {
        self.ask("short_paths", json!({ "id": id, "paths": paths }))
    }
}

#[test]
fn a_drop_is_told_to_the_page_with_its_number_its_paths_and_where_it_was() {
    let rig = app();
    rig.tell(DragDropEvent::Drop {
        paths: vec![PathBuf::from(DASHED)],
        position: PhysicalPosition::new(120.5, 64.0),
    });
    rig.tell(DragDropEvent::Drop {
        paths: vec![PathBuf::from(r"C:\b.png"), PathBuf::from(r"C:\c.png")],
        position: PhysicalPosition::new(1.0, 2.0),
    });
    // A drag of no file (text from another program) is told with no number.
    rig.tell(DragDropEvent::Drop {
        paths: Vec::new(),
        position: PhysicalPosition::new(3.0, 4.0),
    });
    // The drag's other events are Tauri's own to tell.
    rig.tell(DragDropEvent::Enter {
        paths: vec![PathBuf::from(DASHED)],
        position: PhysicalPosition::new(5.0, 6.0),
    });
    rig.tell(DragDropEvent::Over {
        position: PhysicalPosition::new(7.0, 8.0),
    });
    rig.tell(DragDropEvent::Leave);
    assert_eq!(
        rig.told(),
        [
            json!({"id":1,"paths":[DASHED],"position":{"x":120.5,"y":64.0}}),
            json!({"id":2,"paths":[r"C:\b.png", r"C:\c.png"],"position":{"x":1.0,"y":2.0}}),
            json!({"id":null,"paths":[],"position":{"x":3.0,"y":4.0}}),
        ]
    );
    // The next drop of files is the third.
    assert_eq!(rig.drop_on(&[DASHED]), 3);
}

#[test]
fn the_page_is_answered_for_the_paths_of_the_drop_it_names_and_no_other() {
    let rig = app();
    let not_kept = json!({"ok":false,"error":NOT_KEPT});
    // Before any drop, none.
    assert_eq!(rig.short_paths_of(1, &[DASHED]), not_kept);
    // The drag that goes over the window and leaves is no drop.
    rig.tell(DragDropEvent::Enter {
        paths: vec![PathBuf::from(DASHED)],
        position: PhysicalPosition::new(0.0, 0.0),
    });
    rig.tell(DragDropEvent::Leave);
    assert_eq!(rig.short_paths_of(1, &[DASHED]), not_kept);
    // Three drops, the first not yet asked of while the others come: each is answered for
    // its own paths, and only for them.
    let (b, c) = (r"C:\Users\me\b — 2.png", r"C:\Users\me\c — 3.png");
    let first = rig.drop_on(&[DASHED, r"C:\Users\me\plain.png"]);
    let second = rig.drop_on(&[b]);
    let third = rig.drop_on(&[c]);
    assert_eq!(
        rig.short_paths_of(first, &[DASHED, b]),
        json!({"ok":true,"paths":[{"error":LOOKED_UP}, {"error":NOT_DROPPED}]})
    );
    assert_eq!(
        rig.short_paths_of(second, &[b, c]),
        json!({"ok":true,"paths":[{"error":LOOKED_UP}, {"error":NOT_DROPPED}]})
    );
    assert_eq!(
        rig.short_paths_of(third, &[c, DASHED]),
        json!({"ok":true,"paths":[{"error":LOOKED_UP}, {"error":NOT_DROPPED}]})
    );
    // A number no drop was given.
    assert_eq!(rig.short_paths_of(third + 1, &[c]), not_kept);
}

#[test]
fn the_app_keeps_the_last_drops_for_the_page_and_lets_the_oldest_go() {
    let rig = app();
    let first = rig.drop_on(&[DASHED]);
    let second = rig.drop_on(&[DASHED]);
    for _ in 2..PENDING {
        rig.drop_on(&[DASHED]);
    }
    assert_eq!(rig.short_paths_of(first, &[DASHED])["ok"], true);
    rig.drop_on(&[DASHED]);
    assert_eq!(
        rig.short_paths_of(first, &[DASHED]),
        json!({"ok":false,"error":NOT_KEPT})
    );
    assert_eq!(rig.short_paths_of(second, &[DASHED])["ok"], true);
}

#[test]
fn a_drop_on_another_window_of_the_app_is_not_heard() {
    let rig = app();
    let other = WebviewWindowBuilder::new(&rig.app, "other", Default::default())
        .build()
        .expect("build another mock window");
    rig.tell_to(&other.as_ref().window(), dropping(&[DASHED]));
    assert_eq!(rig.told(), Vec::<Value>::new(), "the page is told nothing");
    assert_eq!(
        rig.short_paths_of(1, &[DASHED]),
        json!({"ok":false,"error":NOT_KEPT})
    );
    // The drop on the app's window after it is the first the app heard.
    assert_eq!(rig.drop_on(&[DASHED]), 1);
}

#[test]
fn a_path_that_is_no_whole_path_to_a_file_is_refused_though_it_was_dropped() {
    let rig = app();
    let refused = [
        ("C:\\a\u{0}b.png", "its name holds a control character"),
        ("C:\\a\nb.png", "its name holds a control character"),
        (r"Users\me\a.png", NOT_WHOLE),
        (r"C:a.png", NOT_WHOLE),
        (r"\Users\me\a.png", NOT_WHOLE),
        (r"\\server", NOT_WHOLE),
        (r"C:\a\..\b.png", NOT_WHOLE),
        (r"C:\a\\b.png", NOT_WHOLE),
        (r"\\.\PhysicalDrive0", DEVICE),
        (r"\\?\GLOBALROOT\Device\HarddiskVolume1\a.png", DEVICE),
        (r"C:\Users\me\NUL", DEVICE),
        (r"C:\Users\me\com1.txt", DEVICE),
        (
            r"C:\Users\me\a?.png",
            "its name holds a character no Windows name has",
        ),
        (
            r"C:\Users\me/a.png",
            "its name holds a character no Windows name has",
        ),
    ];
    let paths: Vec<&str> = refused.iter().map(|(path, _)| *path).collect();
    let id = rig.drop_on(&paths);
    let answered = rig.short_paths_of(id, &paths);
    for (at, (path, why)) in refused.iter().enumerate() {
        assert_eq!(answered["paths"][at], json!({ "error": why }), "{path:?}");
    }
    let longest = format!(r"C:\{}", "a".repeat(LONGEST));
    assert_eq!(
        malformed(&longest),
        Some("it is longer than any Windows path")
    );
}

#[test]
fn a_whole_path_on_a_drive_or_a_share_in_its_old_or_long_form_is_one_to_look_up() {
    for path in [
        r"C:\Users\me\a — b “c”.png",
        r"d:\",
        r"C:\Users\me\folder\",
        r"\\server\share\x y\a.png",
        r"\\server\share",
        r"\\?\C:\Users\me\a.png",
        r"\\?\UNC\server\share\a.png",
        r"C:\Users\me\console.log",
        r"C:\Users\me\COM10.txt",
    ] {
        assert_eq!(malformed(path), None, "{path}");
    }
}

#[test]
fn the_page_asks_at_most_a_bounded_batch_and_one_lookup_at_a_time() {
    let rig = app();
    let many: Vec<String> = (0..=MOST).map(|n| format!(r"C:\a\{n}.png")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let id = rig.drop_on(&many);
    assert_eq!(
        rig.short_paths_of(id, &many),
        json!({"ok":false,"error":"the app looks up at most 64 short names at once"})
    );
    assert_eq!(rig.short_paths_of(id, &many[..MOST])["ok"], true);
    // While a lookup has its turn, another is refused as busy; once it has ended, one goes.
    let dropped = rig.window.state::<Dropped>().inner().clone();
    let turn = dropped.turn().expect("a turn");
    assert_eq!(rig.short_paths_of(id, &many[..1]), busy());
    drop(turn);
    assert_eq!(rig.short_paths_of(id, &many[..1])["ok"], true);
}

/// What a lookup is answered while another runs, which the page asks again.
fn busy() -> Value {
    json!({"ok":false,"busy":true,"error":"another lookup of short names is running"})
}

#[test]
fn a_lookup_no_one_waits_for_any_more_keeps_its_turn_until_it_ends_then_gives_it_back() {
    let dropped = Dropped::default();
    let (id, _) = dropped.record(&[PathBuf::from(DASHED)], None);
    let id = id.expect("a drop's number");
    let asked = || vec![DASHED.to_owned()];
    let (started, has_started) = mpsc::channel();
    let (let_go, held) = mpsc::channel::<()>();
    // Windows reads a share far away: the lookup waits until the test lets it answer.
    let far = move |_: &str| {
        let _ = started.send(());
        let _ = held.recv();
        Err("the share answered at last".to_owned())
    };
    // The page asks, and is reloaded while Windows reads: no one waits for the answer.
    let first = tauri::async_runtime::spawn(dropped.clone().looked_up(id, asked(), far));
    has_started
        .recv_timeout(Duration::from_secs(10))
        .expect("the lookup began");
    first.abort();
    let _ = tauri::async_runtime::block_on(first);
    // The new page's lookup is refused as busy while the first is read.
    let again = |dropped: &Dropped| {
        tauri::async_runtime::block_on(
            dropped
                .clone()
                .looked_up(id, asked(), |_| Err("read".to_owned())),
        )
    };
    assert_eq!(again(&dropped), busy());
    // Windows answers: the turn goes back, though no one took the answer.
    let_go.send(()).expect("let the lookup answer");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let answered = again(&dropped);
        if answered["ok"] == true {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the turn never came back: {answered}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_path_past_the_old_limit_is_asked_in_its_long_form_and_answered_in_its_old_one_where_it_fits() {
    let long = format!(r"C:\{}\a.png", "x".repeat(300));
    assert_eq!(long_form(&long), format!(r"\\?\{long}"));
    let share = format!(r"\\server\share\{}", "x".repeat(300));
    assert_eq!(
        long_form(&share),
        format!(r"\\?\UNC\server\share\{}", "x".repeat(300))
    );
    // Short enough, or long already: as it is.
    assert_eq!(long_form(DASHED), DASHED);
    assert_eq!(
        long_form(r"\\?\C:\Users\me\a.png"),
        r"\\?\C:\Users\me\a.png"
    );
    // The answer goes back to the old form where it fits, and only where the app added the long one.
    assert_eq!(
        old_form(r"\\?\C:\XXXXXX~1\A.PNG", true),
        r"C:\XXXXXX~1\A.PNG"
    );
    assert_eq!(
        old_form(r"\\?\UNC\server\share\XXXXXX~1", true),
        r"\\server\share\XXXXXX~1"
    );
    assert_eq!(
        old_form(r"\\?\C:\XXXXXX~1\A.PNG", false),
        r"\\?\C:\XXXXXX~1\A.PNG"
    );
    let still_long = format!(r"\\?\C:\{}", "y".repeat(300));
    assert_eq!(old_form(&still_long, true), still_long);
}

#[test]
fn each_way_windows_fails_to_answer_is_said_in_words_of_its_own() {
    assert_eq!(reason(2), "it is not there any more");
    assert_eq!(reason(3), "it is not there any more");
    assert_eq!(reason(5), "Windows refused access to it");
    for network in [51, 53, 59, 64, 65, 67, 121, 1203, 1222, 1231, 1232, 2250] {
        assert_eq!(
            reason(network),
            "its network share did not answer",
            "{network}"
        );
    }
    assert_eq!(reason(206), "its path is longer than Windows reads");
    assert_eq!(reason(1), "Windows answered error 1 for it");
}

#[test]
fn the_app_comes_in_front_once_for_a_drop_and_only_from_the_window_the_file_came_from() {
    assert!(may_focus(Some(7), Some(7)));
    assert!(
        !may_focus(Some(7), Some(8)),
        "the human went to another program"
    );
    assert!(!may_focus(Some(7), None));
    assert!(!may_focus(None, None), "no drop, or none off Windows");
    let dropped = Dropped::default();
    let numbered = |from| {
        let (id, _) = dropped.record(&[PathBuf::from(DASHED)], Some(from));
        id.expect("a drop's number")
    };
    let (seven, nine) = (numbered(7), numbered(9));
    assert_eq!(dropped.take_from(nine), Some(9));
    assert_eq!(dropped.take_from(seven), Some(7), "each drop its own");
    assert_eq!(dropped.take_from(seven), None, "once for each drop");
    assert_eq!(
        dropped.take_from(nine + 1),
        None,
        "none for a drop not heard"
    );
}

#[cfg(not(windows))]
#[test]
fn off_windows_the_app_never_comes_in_front_for_a_drop() {
    let rig = app();
    let id = rig.drop_on(&[DASHED]);
    assert_eq!(
        rig.ask("focus_after_drop", json!({ "id": id })),
        json!({"ok":true,"focused":false})
    );
}

/// On Windows, against the volume the tests write to (`cargo xtask app test file_drop` on
/// zeewin). Where the volume keeps no 8.3 names a test says so, and why, and asserts nothing.
#[cfg(windows)]
mod on_windows {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{FindClose, FindFirstFileW, WIN32_FIND_DATAW};

    use super::*;

    /// The 8.3 name Windows keeps for the last name of `path`, a whole path on a drive,
    /// as its folder lists it (`FindFirstFileW`, another way than the one under test);
    /// empty where it keeps none. Asked in the long form (`\\?\`), which takes a path of
    /// any length.
    fn alias(path: &Path) -> String {
        let long = format!(r"\\?\{}", path.display());
        let wide: Vec<u16> = std::ffi::OsStr::new(&long)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: an all-zero WIN32_FIND_DATAW is a valid value of the plain struct.
        let mut found: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
        // SAFETY: `wide` is NUL-terminated and outlives the call; `found` is written.
        let handle = unsafe { FindFirstFileW(wide.as_ptr(), &mut found) };
        assert!(
            handle != INVALID_HANDLE_VALUE,
            "{} is not there",
            path.display()
        );
        // SAFETY: `handle` is the search FindFirstFileW opened.
        unsafe { FindClose(handle) };
        let name = &found.cAlternateFileName;
        let end = name
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(name.len());
        String::from_utf16(&name[..end]).expect("an alias that is text")
    }

    /// A file named `name` in a folder of its own, and what it holds.
    fn file(name: &str) -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().expect("a folder");
        let path = folder.path().join(name);
        std::fs::write(&path, "the file").expect("write the file");
        (folder, path)
    }

    fn skipped(path: &Path) {
        println!(
            "skipped: the volume of {} keeps no 8.3 names (`fsutil 8dot3name query` says so for it)",
            path.display()
        );
    }

    #[test]
    fn a_file_whose_name_needs_an_alias_is_answered_its_alias_which_names_it() {
        let (_folder, path) = file("résumé — final “2”.txt");
        let alias = alias(&path);
        if alias.is_empty() {
            return skipped(&path);
        }
        let long = path.to_str().expect("a path that is text");
        let rig = app();
        let id = rig.drop_on(&[long]);
        let answered = rig.short_paths_of(id, &[long]);
        let short = answered["paths"][0]["short"]
            .as_str()
            .unwrap_or_else(|| panic!("a short name: {answered}"));
        assert_ne!(short, long);
        assert!(
            short.ends_with(&format!(r"\{alias}")),
            "{short} ends with {alias}"
        );
        assert_eq!(
            std::fs::read_to_string(short).expect("read by its short name"),
            "the file"
        );
    }

    #[test]
    fn the_alias_of_a_name_of_marks_the_console_drops_is_ascii() {
        let (_folder, path) = file("— “” →.txt");
        let alias = alias(&path);
        if alias.is_empty() {
            return skipped(&path);
        }
        assert!(alias.is_ascii(), "{alias}");
    }

    #[test]
    fn a_path_whose_names_need_no_alias_is_answered_that_windows_keeps_none_for_it() {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
        let notepad = format!(r"{root}\System32\notepad.exe");
        assert_eq!(short_name(&notepad).as_deref(), Ok(notepad.as_str()));
        let rig = app();
        let id = rig.drop_on(&[&notepad]);
        assert_eq!(
            rig.short_paths_of(id, &[&notepad])["paths"][0],
            json!({"error":"Windows keeps no short name for it"})
        );
    }

    #[test]
    fn a_path_past_the_old_limit_is_answered_its_alias() {
        let folder = tempfile::tempdir().expect("a folder");
        let mut deep = folder.path().to_path_buf();
        while deep.as_os_str().len() < 300 {
            deep.push("a folder named at length");
        }
        std::fs::create_dir_all(&deep).expect("make the folders");
        let path = deep.join("— “2”.txt");
        std::fs::write(&path, "the file").expect("write the file");
        let alias = alias(&path);
        if alias.is_empty() {
            return skipped(&path);
        }
        let long = path.to_str().expect("a path that is text");
        let rig = app();
        let id = rig.drop_on(&[long]);
        let answered = rig.short_paths_of(id, &[long]);
        let short = answered["paths"][0]["short"]
            .as_str()
            .unwrap_or_else(|| panic!("a short name: {answered}"));
        assert!(short.ends_with(&format!(r"\{alias}")), "{short}");
    }

    #[test]
    fn a_dropped_file_that_is_gone_is_answered_so() {
        let (folder, path) = file("— gone.txt");
        let long = path.to_str().expect("a path that is text").to_owned();
        std::fs::remove_file(&path).expect("remove the file");
        let rig = app();
        let id = rig.drop_on(&[&long]);
        assert_eq!(
            rig.short_paths_of(id, &[&long])["paths"][0],
            json!({"error":"it is not there any more"})
        );
        drop(folder);
    }
}
