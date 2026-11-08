//! A panic's hook and a panic's edge are the whole process's, and an abort ends it, so each test
//! runs in a process of its own, its binary run again: the child does what the test is about, the
//! test reads what it left.

use super::*;
use std::process::Output;
use std::time::Duration;

/// Set in the child: the log it writes in.
const LOG: &str = "CONSENSFLOW_TEST_PANIC_LOG";

/// In the child, the log it was given; in the test, None.
#[allow(clippy::disallowed_methods)] // The test tells itself it is its own child by a CONSENSFLOW_TEST_* variable.
fn the_childs_log() -> Option<PathBuf> {
    std::env::var_os(LOG).map(PathBuf::from)
}

/// Runs the test `name` again in a child, with a log of its own; what it said, and its log.
fn in_a_child(name: &str) -> (Output, String) {
    let home = tempfile::tempdir().expect("home");
    let log = home.path().join("app.log");
    let ran = crate::testing::command(std::env::current_exe().expect("the test binary"))
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(LOG, &log)
        .env_remove("RUST_BACKTRACE")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run the test in a child");
    let written = std::fs::read_to_string(&log).unwrap_or_default();
    (ran, written)
}

/// How many times the child's person was told.
static TOLD_TIMES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The child's teller: it says it told, and on which thread.
fn tell_by_printing() {
    tell_with(|| {
        let thread = std::thread::current();
        println!(
            "told the person on {}",
            thread.name().unwrap_or("an unnamed thread")
        );
        TOLD_TIMES.fetch_add(1, Ordering::AcqRel);
    });
}

/// Waits for the child's person to be told, and a little more for a second telling to show.
fn wait_for_the_telling() {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while TOLD_TIMES.load(Ordering::Acquire) == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(300));
}

fn said(ran: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    )
}

#[test]
fn a_panic_is_written_to_the_error_log_with_its_thread_place_and_backtrace() {
    if the_childs_log().is_some() {
        record_in(the_childs_log);
        let _ = std::thread::Builder::new()
            .name("deliberate".into())
            .spawn(|| panic!("the deliberate panic of T-908"))
            .expect("a thread")
            .join();
        return;
    }
    let (ran, written) = in_a_child(
        "panics::tests::a_panic_is_written_to_the_error_log_with_its_thread_place_and_backtrace",
    );
    assert!(ran.status.success(), "{}", said(&ran));
    // The place as the platform spells a path: `app\src-tauri\src\panics\tests.rs` on Windows.
    let place = ["src", "panics", "tests.rs:"].join(std::path::MAIN_SEPARATOR_STR);
    assert!(
        written.contains("consensflow: thread 'deliberate' panicked at ")
            && written.contains(&place),
        "{written}"
    );
    assert!(
        written.contains("\nthe deliberate panic of T-908\n"),
        "{written}"
    );
    assert!(
        written.contains("backtrace:\n")
            && written.contains(
                "a_panic_is_written_to_the_error_log_with_its_thread_place_and_backtrace"
            ),
        "the backtrace names the panic's frames: {written}"
    );
}

/// A panic that reaches a function that cannot unwind ends the process in an abort, as tao's
/// `extern "C"` functions end a panic under them: it is written down before the abort.
#[test]
fn a_panic_that_ends_in_an_abort_is_written_before_it() {
    /// What is dropped on the way takes its time, as on a loaded runner, where the unwind to the
    /// function that cannot unwind took longer than 20 ms (CI's Windows, run 40950847078, T-946).
    struct Slow;
    impl Drop for Slow {
        fn drop(&mut self) {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    fn slowly() {
        let _slow = Slow;
        panic!("the panic of T-908 that aborts");
    }
    extern "C" fn cannot_unwind() {
        slowly();
    }
    if the_childs_log().is_some() {
        // The panic's writing takes twice the window an abort is a panic's within, so a stamp
        // taken as the panic began would be that old when its abort is described: the window is
        // counted from the end of the writing (T-929). What it then holds is the unwind alone,
        // which the slow drop above stretches as a loaded runner did, and no window narrower
        // than the product's is assumed (T-946).
        static WRITTEN: AtomicBool = AtomicBool::new(false);
        record_in(|| {
            if !WRITTEN.swap(true, Ordering::Relaxed) {
                std::thread::sleep(UNWOUND_WITHIN * 2);
            }
            the_childs_log()
        });
        cannot_unwind();
        return;
    }
    let (ran, written) =
        in_a_child("panics::tests::a_panic_that_ends_in_an_abort_is_written_before_it");
    assert!(!ran.status.success(), "it aborts: {}", said(&ran));
    #[cfg(unix)]
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&ran.status),
        Some(libc::SIGABRT)
    );
    let panic = written.find("\nthe panic of T-908 that aborts\n");
    let abort = written.find(&format!("\n{CANNOT_UNWIND}\n"));
    assert!(
        panic.is_some() && abort.is_some() && panic < abort,
        "the panic, then the abort: {written}"
    );
    assert!(!written.contains("No panic began"), "{written}");
}

/// The field crash of 2026-10-28, made again: an Objective-C exception raised under a Rust
/// `extern "C"` function, as AppKit's under tao's `sendEvent:`. Rust aborts with the one line
/// the live app's log held; the hook says it was no panic of Rust's.
#[cfg(target_os = "macos")]
#[test]
fn an_objective_c_exception_into_a_function_that_cannot_unwind_is_said_to_be_one() {
    extern "C" fn cannot_unwind() {
        objc2::exception::throw(objc_test::exception());
    }
    if the_childs_log().is_some() {
        record_in(the_childs_log);
        cannot_unwind();
        return;
    }
    let (ran, written) = in_a_child(
        "panics::tests::an_objective_c_exception_into_a_function_that_cannot_unwind_is_said_to_be_one",
    );
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&ran.status),
        Some(libc::SIGABRT),
        "{}",
        said(&ran)
    );
    assert_eq!(written.matches(" panicked at ").count(), 1, "{written}");
    assert!(
        written.contains(&format!("\n{CANNOT_UNWIND}\n")),
        "{written}"
    );
    assert!(
        written.contains("No panic began on this thread before it")
            && written.contains("most likely an Objective-C one"),
        "{written}"
    );
}

#[test]
fn a_panic_in_a_callback_is_stopped_at_its_edge_and_told_once() {
    if the_childs_log().is_some() {
        record_in(the_childs_log);
        tell_by_printing();
        assert_eq!(
            contained("a test's callback", || -> u8 {
                panic!("the first of T-908")
            }),
            None
        );
        assert_eq!(
            contained("a test's callback", || -> u8 {
                panic!("the second of T-908")
            }),
            None
        );
        assert_eq!(contained("a test's callback", || 7), Some(7));
        println!("the app runs on");
        wait_for_the_telling();
        return;
    }
    let (ran, written) =
        in_a_child("panics::tests::a_panic_in_a_callback_is_stopped_at_its_edge_and_told_once");
    let said = said(&ran);
    assert!(ran.status.success(), "{said}");
    assert!(said.contains("the app runs on"), "{said}");
    assert_eq!(
        said.matches("told the person").count(),
        1,
        "told once: {said}"
    );
    // On a thread of its own, after the callback's edge: never in the frame that stopped it.
    assert!(
        said.contains("told the person on consensflow-tell"),
        "{said}"
    );
    for panic in ["the first of T-908", "the second of T-908"] {
        assert!(written.contains(&format!("\n{panic}\n")), "{written}");
    }
    assert_eq!(
        written
            .matches(
                "consensflow: a test's callback panicked; the app stopped it there and runs on"
            )
            .count(),
        2,
        "{written}"
    );
}

#[test]
fn the_person_is_told_where_the_log_is() {
    let told = told(Some(Path::new("/Users/test/.consensflow/app/app.log")));
    assert!(
        told.contains("/Users/test/.consensflow/app/app.log"),
        "{told}"
    );
    assert!(told.contains("kept running"), "{told}");
}

/// An exception AppKit raises, and a panic of a `C-unwind` method it calls, under tao's
/// `extern "C"` `sendEvent:`: with AppKit's dispatch guarded, each is stopped below tao's frame,
/// written down with what it said, told once, and the app runs on. The classes stand for
/// `NSApplication` (its `sendEvent:` guarded) and tao's `TaoApp` (an override that calls it).
#[cfg(target_os = "macos")]
#[test]
fn what_unwinds_out_of_a_guarded_dispatch_stops_there_below_taos_frame() {
    use std::sync::atomic::AtomicU8;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, NSObject, Sel};
    use objc2::{msg_send, sel, ClassType};

    use super::appkit::Guarded;

    const RAISE: u8 = 1;
    const PANIC: u8 = 2;
    const RAISE_BADLY: u8 = 3;
    static DOES: AtomicU8 = AtomicU8::new(0);
    static GUARDED: Guarded = Guarded::new("the test's dispatch");

    unsafe extern "C-unwind" fn dispatch(_: &AnyObject, _: Sel, _: *mut AnyObject) {
        match DOES.load(Ordering::Acquire) {
            RAISE => objc2::exception::throw(objc_test::exception()),
            PANIC => panic!("the panic of T-908 under a dispatch"),
            RAISE_BADLY => objc2::exception::throw(objc_test::exception_whose_stack_raises()),
            _ => println!("dispatched"),
        }
    }
    unsafe extern "C-unwind" fn guard(this: &AnyObject, sel: Sel, event: *mut AnyObject) {
        unsafe { GUARDED.send(this, sel, event) }
    }
    /// tao's `send_event`, as it is: `extern "C"`, calling its superclass's.
    extern "C" fn tao_send_event(this: &AnyObject, _: Sel, event: *mut AnyObject) {
        let superclass = AnyClass::get(c"CfT908Application").expect("the class");
        let _: () = unsafe { msg_send![super(this, superclass), sendEvent: event] };
    }

    if the_childs_log().is_some() {
        record_in(the_childs_log);
        tell_by_printing();
        let mut application =
            ClassBuilder::new(c"CfT908Application", NSObject::class()).expect("a new class");
        // SAFETY: the function has the method's signature.
        unsafe {
            application.add_method(
                sel!(sendEvent:),
                dispatch as unsafe extern "C-unwind" fn(_, _, _),
            )
        };
        let application = application.register();
        let mut tao = ClassBuilder::new(c"CfT908TaoApp", application).expect("a new class");
        // SAFETY: as above.
        unsafe { tao.add_method(sel!(sendEvent:), tao_send_event as extern "C" fn(_, _, _)) };
        let tao = tao.register();
        assert!(GUARDED.install(application, guard));
        assert!(
            GUARDED.install(application, guard),
            "a second install keeps the first"
        );
        let app: Retained<AnyObject> = unsafe { msg_send![tao, new] };
        for does in [RAISE_BADLY, RAISE, PANIC, 0] {
            DOES.store(does, Ordering::Release);
            let _: () = unsafe { msg_send![&*app, sendEvent: std::ptr::null_mut::<AnyObject>()] };
        }
        println!("the app runs on");
        wait_for_the_telling();
        return;
    }
    let (ran, written) = in_a_child(
        "panics::tests::what_unwinds_out_of_a_guarded_dispatch_stops_there_below_taos_frame",
    );
    let said = said(&ran);
    assert!(ran.status.success(), "{said}\n{written}");
    assert!(
        said.contains("dispatched") && said.contains("the app runs on"),
        "{said}"
    );
    assert_eq!(
        said.matches("told the person").count(),
        1,
        "told once: {said}"
    );
    assert!(
        written.contains("consensflow: the test's dispatch raised exception ")
            && written.contains("'CfT908Exception' reason: AppKit's stand-in raised it")
            && written.contains("\nraised on:\n"),
        "{written}"
    );
    assert!(
        written.contains("\nthe panic of T-908 under a dispatch\n")
            && written
                .contains("consensflow: the test's dispatch panicked; the app stopped it there"),
        "{written}"
    );
    // What raised as it was being said is stopped as well, still below tao's frame (T-929).
    assert!(
        written.contains(
            "consensflow: the test's dispatch raised or panicked, and saying which raised or \
             panicked in turn; the app stopped it there"
        ),
        "{written}"
    );
    assert!(!written.contains(CANNOT_UNWIND), "{written}");
}

/// The app's own classes: `NSApplication`'s and `NSWindow`'s own `sendEvent:` are the guards, and
/// a second call leaves AppKit's as the originals; a class with no `sendEvent:` of its own is not
/// guarded, its superclass's in its name.
#[cfg(target_os = "macos")]
#[test]
fn appkits_dispatch_to_the_app_and_to_each_window_is_guarded() {
    use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Imp, Sel};
    use objc2::{class, sel};

    fn own_send_event(class: &AnyClass) -> Option<Imp> {
        class
            .instance_methods()
            .iter()
            .find(|method| method.name() == sel!(sendEvent:))
            .map(|method| method.implementation())
    }
    if the_childs_log().is_some() {
        let application = own_send_event(class!(NSApplication)).expect("AppKit's own");
        let window = own_send_event(class!(NSWindow)).expect("AppKit's own");
        appkit::guard_event_dispatch();
        appkit::guard_event_dispatch();
        for (class, appkits) in [
            (class!(NSApplication), application),
            (class!(NSWindow), window),
        ] {
            let now = own_send_event(class).expect("a sendEvent:");
            assert_ne!(now as usize, appkits as usize, "{class:?} is not guarded");
            println!("{} guarded", class.name().to_string_lossy());
        }
        assert!(
            appkit::guards_appkits(application, window),
            "the originals are AppKit's"
        );
        static INHERITED: appkit::Guarded = appkit::Guarded::new("a window's subclass");
        unsafe extern "C-unwind" fn inherited(_: &AnyObject, _: Sel, _: *mut AnyObject) {}
        let subclass = ClassBuilder::new(c"CfT908Window", class!(NSWindow))
            .expect("a new class")
            .register();
        assert!(
            !INHERITED.install(subclass, inherited),
            "a class with NSWindow's sendEvent: and none of its own is not guarded"
        );
        assert!(
            appkit::guards_appkits(application, window),
            "and NSWindow's own is left as it was"
        );
        return;
    }
    let (ran, _) =
        in_a_child("panics::tests::appkits_dispatch_to_the_app_and_to_each_window_is_guarded");
    let said = said(&ran);
    assert!(ran.status.success(), "{said}");
    assert!(
        said.contains("NSApplication guarded") && said.contains("NSWindow guarded"),
        "{said}"
    );
}

#[cfg(target_os = "macos")]
use super::appkit;

/// What the macOS tests raise.
#[cfg(target_os = "macos")]
mod objc_test {
    use objc2::exception::Exception;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    fn string(text: &std::ffi::CStr) -> *mut AnyObject {
        unsafe { msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()] }
    }

    /// An NSException whose `callStackSymbols` raises in turn, as one of a class of its own may.
    pub(super) fn exception_whose_stack_raises() -> Retained<Exception> {
        use objc2::runtime::{AnyClass, ClassBuilder, Sel};
        use objc2::sel;

        unsafe extern "C-unwind" fn raises(_: &AnyObject, _: Sel) -> *mut AnyObject {
            objc2::exception::throw(exception())
        }
        let class = AnyClass::get(c"CfT929BadException").unwrap_or_else(|| {
            let mut class =
                ClassBuilder::new(c"CfT929BadException", class!(NSException)).expect("a new class");
            // SAFETY: the function has the method's signature.
            unsafe {
                class.add_method(
                    sel!(callStackSymbols),
                    raises as unsafe extern "C-unwind" fn(_, _) -> _,
                )
            };
            class.register()
        });
        let name = string(c"CfT929BadException");
        let reason = string(c"its stack raises");
        unsafe {
            msg_send![
                class,
                exceptionWithName: name,
                reason: reason,
                userInfo: std::ptr::null_mut::<AnyObject>()
            ]
        }
    }

    /// An NSException, named and with its reason, as AppKit raises one.
    pub(super) fn exception() -> Retained<Exception> {
        let name = string(c"CfT908Exception");
        let reason = string(c"AppKit's stand-in raised it");
        unsafe {
            msg_send![
                class!(NSException),
                exceptionWithName: name,
                reason: reason,
                userInfo: std::ptr::null_mut::<AnyObject>()
            ]
        }
    }
}

/// `run` records the panics and guards AppKit's dispatch before the app is built, tells the
/// person through its dialog, and each callback of the app's own the event loop calls is
/// contained: read from the sources, as `run` is the shipping app's start and no test runs it.
#[test]
fn the_app_records_its_panics_and_contains_its_callbacks_from_its_start() {
    let lib = include_str!("../lib.rs");
    let run = &lib[lib.find("pub fn run()").expect("run")..];
    let built = run.find("tauri::Builder::default()").expect("the builder");
    for wired in [
        "panics::record_in(error_log);",
        "panics::appkit::guard_event_dispatch();",
    ] {
        assert!(run.find(wired).is_some_and(|at| at < built), "{wired}");
    }
    for wired in [
        "panics::tell_with(",
        r#"panics::contained("the app's handler of its event loop""#,
    ] {
        assert!(run.contains(wired), "{wired}");
    }
    assert!(lib.contains(r#"panics::contained("the app's handler of a window's event""#));
    let updates = include_str!("../updates.rs");
    assert!(updates.contains(r#"crate::panics::contained("the app's handler of its menu""#));
}

/// The page's origin, as the app's webview has it on this system: Tauri refuses a page of any
/// other the app's commands, as a remote one's.
const PAGE_ORIGIN: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

/// A command the page asks through the app's own commands (`crate::page_commands`), on Tauri's
/// stand-in runtime, and what the page was answered: the response, or the panic that left the
/// app's edge, which in the app is an abort.
fn asked_of_the_page(command: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    let app = crate::with_page(mock_builder())
        .build(mock_context(noop_assets()))
        .expect("build mock app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("build mock webview");
    let request = tauri::webview::InvokeRequest {
        cmd: command.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: PAGE_ORIGIN.parse().expect("invoke URL"),
        body: tauri::ipc::InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    let answered = panic::catch_unwind(AssertUnwindSafe(|| get_ipc_response(&webview, request)))
        .map_err(|panicked| {
            // The stand-in's wait for the answer panics when the request's responder is dropped
            // unanswered, as a task that panicked drops it: in the app the page's promise then
            // never settles.
            if panicked
                .downcast_ref::<String>()
                .is_some_and(|said| said.contains("Failed to receive result from command"))
            {
                format!("{command}'s request was dropped unanswered")
            } else {
                format!("{command}'s panic left the app's edge")
            }
        })?;
    match answered {
        Ok(body) => body.deserialize().map_err(|error| error.to_string()),
        Err(refused) => Err(format!("refused: {refused}")),
    }
}

/// `asked_of_the_page` on a thread of its own, its answer waited for up to `limit`: none where
/// the page was never answered.
fn asked_within(
    command: &'static str,
    args: serde_json::Value,
    limit: Duration,
) -> Option<Result<serde_json::Value, String>> {
    let (answer, answered) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = answer.send(asked_of_the_page(command, args));
    });
    answered.recv_timeout(limit).ok()
}

/// A panic in an asynchronous command the page asks is stopped in its task (T-1050): Tauri's
/// task of the command catches it too, but never answers the page, whose promise then never
/// settles, and tells nobody. Written down, told once, and the page answered with the error.
#[test]
fn a_panic_in_an_async_command_the_page_asks_is_answered_and_told() {
    if the_childs_log().is_some() {
        record_in(the_childs_log);
        tell_by_printing();
        let answer = asked_within(
            "selftest_panic_async",
            serde_json::json!({}),
            Duration::from_secs(20),
        );
        assert_eq!(
            answer,
            Some(Ok(
                serde_json::json!({"ok": false, "error": failed("selftest_panic_async")})
            )),
            "the page was answered of the command whose task panicked (None: never)"
        );
        println!("the app runs on");
        wait_for_the_telling();
        return;
    }
    let (ran, written) =
        in_a_child("panics::tests::a_panic_in_an_async_command_the_page_asks_is_answered_and_told");
    let said = said(&ran);
    // The child's own failure is written in its log, by the hook it records its panics with.
    assert!(ran.status.success(), "{said}\nthe child's log:\n{written}");
    assert!(said.contains("the app runs on"), "{said}");
    assert_eq!(
        said.matches("told the person").count(),
        1,
        "told once: {said}"
    );
    assert!(
        written.contains(&format!("\n{}\n", crate::SELFTEST_PANIC)),
        "{written}"
    );
    assert!(
        written.contains(
            "consensflow: the app's command selftest_panic_async panicked; the app stopped it \
             there and runs on"
        ),
        "{written}"
    );
}

/// A panic in a command the page asks is stopped at the app's edge (T-1050): written down, the
/// person told once, the page answered with the error, and the app answers the page after it.
/// In the app the command runs in the webview's own frame, which cannot unwind: past the edge
/// the panic aborts the app.
#[test]
fn a_panic_in_a_command_the_page_asks_is_answered_and_the_app_runs_on() {
    if the_childs_log().is_some() {
        record_in(the_childs_log);
        tell_by_printing();
        let answer = asked_of_the_page("selftest_panic", serde_json::json!({}));
        assert_eq!(
            answer,
            Ok(serde_json::json!({"ok": false, "error": failed("selftest_panic")}))
        );
        // Asked after it, a command is answered as ever: here the self-test's channel, which
        // refuses an app not started for a self-test.
        let after = asked_of_the_page(
            "selftest_report",
            serde_json::json!({"event": "after", "data": {}}),
        );
        assert_eq!(
            after,
            Ok(serde_json::json!({"ok": false, "error": "self-test reporting is not enabled"}))
        );
        println!("the app runs on");
        wait_for_the_telling();
        return;
    }
    let (ran, written) = in_a_child(
        "panics::tests::a_panic_in_a_command_the_page_asks_is_answered_and_the_app_runs_on",
    );
    let said = said(&ran);
    // The child's own failure is written in its log, by the hook it records its panics with.
    assert!(ran.status.success(), "{said}\nthe child's log:\n{written}");
    assert!(said.contains("the app runs on"), "{said}");
    assert_eq!(
        said.matches("told the person").count(),
        1,
        "told once: {said}"
    );
    assert!(
        written.contains(&format!("\n{}\n", crate::SELFTEST_PANIC)),
        "{written}"
    );
    assert!(
        written.contains(
            "consensflow: the app's command selftest_panic panicked; the app stopped it there and \
             runs on"
        ),
        "{written}"
    );
}

/// The app's sources as the rule reads them: every file under `src`, but the tests'.
fn the_apps_sources() -> Vec<(PathBuf, String)> {
    fn walk(folder: &Path, found: &mut Vec<(PathBuf, String)>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(folder)
            .expect("read a folder of the sources")
            .map(|entry| entry.expect("an entry").path())
            .collect();
        entries.sort();
        for path in entries {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                if name != "tests" {
                    walk(&path, found);
                }
            } else if name.ends_with(".rs") && name != "tests.rs" {
                let text = std::fs::read_to_string(&path).expect("read a source");
                // A file's own tests are its `mod tests` at its end.
                let code = text
                    .find("#[cfg(test)]\nmod tests {")
                    .map_or(text.as_str(), |at| &text[..at]);
                found.push((path, code.to_owned()));
            }
        }
    }
    let mut found = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    found
}

/// A `#[tauri::command]` as the rule reads it: its name, whether it is asynchronous, and what
/// its body starts with.
struct Declared {
    name: String,
    asynchronous: bool,
    body: String,
}

/// Each `#[tauri::command]` of `code`.
fn commands_in(code: &str) -> Vec<Declared> {
    code.match_indices("#[tauri::command]")
        .map(|(at, _)| {
            let after = &code[at..];
            let fn_at = after.find("fn ").expect("a command's fn");
            let name = &after[fn_at + 3..];
            let end = name
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .expect("a command's name ends");
            let body = &name[name.find('{').expect("a command's body") + 1..];
            Declared {
                name: name[..end].to_owned(),
                asynchronous: after[..fn_at].contains("async "),
                body: body.trim_start().chars().take(120).collect(),
            }
        })
        .collect()
}

/// The name of each `#[tauri::command]` of `code`.
fn commands_of(code: &str) -> Vec<String> {
    commands_in(code)
        .into_iter()
        .map(|command| command.name)
        .collect()
}

/// Every command of the app, synchronous or not, is one of the page's commands
/// (`crate::page_commands`), the app's one handler of the page's invokes, and that handler runs
/// each of them inside `answered`, past which a panic does not go (T-1050): a command added to
/// the app is run there, or this fails, and so it does on a second handler, of the app's or a
/// plugin's of its own. An asynchronous command runs as a task, past `answered`: its body is run
/// inside `settled`, under its own name.
#[test]
fn every_command_of_the_app_is_run_where_its_panic_is_stopped() {
    let sources = the_apps_sources();
    let mut commands: Vec<String> = sources
        .iter()
        .flat_map(|(_, code)| commands_of(code))
        .collect();
    commands.sort();
    assert!(
        commands.len() >= 17 && commands.contains(&"selftest_panic".to_owned()),
        "the rule reads the commands: {commands:?}"
    );

    let handlers: Vec<String> = sources
        .iter()
        .flat_map(|(path, code)| {
            code.matches("generate_handler!")
                .map(move |_| path.display().to_string())
        })
        .collect();
    assert_eq!(
        handlers.len(),
        1,
        "one list of the page's commands: {handlers:?}"
    );
    let given: Vec<String> = sources
        .iter()
        .flat_map(|(_, code)| {
            code.match_indices("invoke_handler(").map(move |(at, _)| {
                let rest = &code[at..];
                rest[..rest.find(')').map_or(rest.len(), |end| end + 1)].to_owned()
            })
        })
        .collect();
    assert_eq!(
        given,
        [
            "invoke_handler(self, handler)",
            "invoke_handler(page_commands()"
        ],
        "the page's commands are the one handler the app is given"
    );

    let lib = include_str!("../lib.rs");
    let page = &lib[lib
        .find("pub(crate) fn page_commands")
        .expect("page_commands")..];
    let list =
        &page[page.find("generate_handler![").expect("the list") + "generate_handler![".len()..];
    // Up to the line that closes it: an entry's `#[cfg(…)]` holds a `]` of its own.
    let list = &list[..list.find("\n    ];").expect("the list's end")];
    let mut listed: Vec<String> = list
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("#["))
        .map(|line| {
            let path = line.trim_end_matches(',');
            path.rsplit("::").next().unwrap_or(path).to_owned()
        })
        .collect();
    listed.sort();
    assert_eq!(
        listed, commands,
        "every command of the app is one of the page's"
    );

    // An asynchronous command runs as a task, past `answered`: its body is run inside `settled`,
    // under its own name.
    let unsettled: Vec<String> = sources
        .iter()
        .flat_map(|(_, code)| commands_in(code))
        .filter(|command| command.asynchronous)
        .filter(|command| {
            let settled = format!("panics::settled(\"{}\", ", command.name);
            !(command.body.starts_with(&format!("crate::{settled}"))
                || command.body.starts_with(&settled))
        })
        .map(|command| command.name)
        .collect();
    assert_eq!(
        unsettled,
        Vec::<String>::new(),
        "every asynchronous command's body is run inside `panics::settled`"
    );

    let guarded = page
        .find("move |invoke| {\n        panics::answered(invoke, |invoke| {")
        .expect("the page's commands are run inside `answered`");
    assert_eq!(
        lib.matches("handler(invoke)").count(),
        1,
        "the list is run in one place"
    );
    assert!(
        page.find("handler(invoke)").is_some_and(|at| at > guarded),
        "the list is run inside `answered`"
    );
}
