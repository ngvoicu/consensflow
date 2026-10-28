//! A panic's hook and a panic's edge are the whole process's, and an abort ends it, so each test
//! runs in a process of its own, its binary run again: the child does what the test is about, the
//! test reads what it left.

use super::*;
use std::process::Output;

/// Set in the child: the log it writes in.
const LOG: &str = "CONSENSFLOW_TEST_PANIC_LOG";

/// In the child, the log it was given; in the test, None.
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
    assert!(
        written.contains("consensflow: thread 'deliberate' panicked at ")
            && written.contains("src/panics/tests.rs:"),
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
    extern "C" fn cannot_unwind() {
        panic!("the panic of T-908 that aborts");
    }
    if the_childs_log().is_some() {
        record_in(the_childs_log);
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
        tell_with(|| println!("told the person"));
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
    static DOES: AtomicU8 = AtomicU8::new(0);
    static GUARDED: Guarded = Guarded::new("the test's dispatch");

    unsafe extern "C-unwind" fn dispatch(_: &AnyObject, _: Sel, _: *mut AnyObject) {
        match DOES.load(Ordering::Acquire) {
            RAISE => objc2::exception::throw(objc_test::exception()),
            PANIC => panic!("the panic of T-908 under a dispatch"),
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
        tell_with(|| println!("told the person"));
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
        for does in [RAISE, PANIC, 0] {
            DOES.store(does, Ordering::Release);
            let _: () = unsafe { msg_send![&*app, sendEvent: std::ptr::null_mut::<AnyObject>()] };
        }
        println!("the app runs on");
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
