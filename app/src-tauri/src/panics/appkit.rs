//! AppKit's dispatch of an event, run so that what unwinds out of it ends there (T-908).
//!
//! tao 0.35 overrides `sendEvent:` of its application and its window classes with `extern "C"`
//! functions (`platform_impl/macos/app.rs`, `window.rs`) that call AppKit's own. objc2's sends
//! are `C-unwind`, so an Objective-C exception AppKit raises under them, or a panic of a
//! `C-unwind` method AppKit calls (objc2's `define_class!`: muda's menu actions, wry's web view,
//! the tray), unwinds up to tao's function, and Rust aborts there: AppKit's handler above it never
//! runs, and nothing is told. So AppKit's own `-[NSApplication sendEvent:]` and
//! `-[NSWindow sendEvent:]`, which tao's call, are each replaced by one that runs it and catches
//! what unwinds out of it, below tao's frame. (tao 0.37.2 made its application's `send_event`
//! `C-unwind`, and so AppKit's handler would see it; Tauri 2.11 is on tao 0.35.)

use std::ffi::CStr;
use std::mem;
use std::panic::{self, AssertUnwindSafe};
use std::sync::OnceLock;

use objc2::exception::{self, Exception};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{class, msg_send, sel};

/// A `sendEvent:` method: the receiver, the selector, the event.
pub(crate) type SendEvent = unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject);

/// One class's `sendEvent:`, run where what unwinds out of it is caught.
pub(crate) struct Guarded {
    /// What the dispatch is, as the log says it.
    edge: &'static str,
    /// The class's own `sendEvent:`, once the guard is in its place.
    original: OnceLock<SendEvent>,
}

static APPLICATION: Guarded = Guarded::new("AppKit's dispatch of an event to the app");
static WINDOW: Guarded = Guarded::new("AppKit's dispatch of an event to a window");

/// Guards AppKit's dispatch of every event: the application's, and each window's. Called by
/// `run` before the event loop starts.
pub(crate) fn guard_event_dispatch() {
    APPLICATION.install(class!(NSApplication), application_send_event);
    WINDOW.install(class!(NSWindow), window_send_event);
}

unsafe extern "C-unwind" fn application_send_event(
    this: &AnyObject,
    sel: Sel,
    event: *mut AnyObject,
) {
    // SAFETY: the guard is called as the method it replaced, with its arguments.
    unsafe { APPLICATION.send(this, sel, event) }
}

unsafe extern "C-unwind" fn window_send_event(this: &AnyObject, sel: Sel, event: *mut AnyObject) {
    // SAFETY: as above.
    unsafe { WINDOW.send(this, sel, event) }
}

impl Guarded {
    pub(crate) const fn new(edge: &'static str) -> Self {
        Self {
            edge,
            original: OnceLock::new(),
        }
    }

    /// Puts `guard` in the place of `class`'s own `sendEvent:`, once: a second call leaves the
    /// first in place, so the guard never runs itself as the original. False for a class that
    /// has none of its own, whose superclass's is not to be replaced for it.
    pub(crate) fn install(&'static self, class: &AnyClass, guard: SendEvent) -> bool {
        let selector = sel!(sendEvent:);
        let methods = class.instance_methods();
        let Some(method) = methods.iter().find(|method| method.name() == selector) else {
            return false;
        };
        self.original.get_or_init(|| {
            // SAFETY: the guard has the method's signature, and the IMP it replaces is one.
            unsafe {
                let replaced = method.set_implementation(mem::transmute::<SendEvent, Imp>(guard));
                mem::transmute::<Imp, SendEvent>(replaced)
            }
        });
        true
    }

    /// The class's own `sendEvent:`, and what unwound out of it stopped here.
    ///
    /// # Safety
    ///
    /// The arguments are those the replaced method was called with.
    pub(crate) unsafe fn send(&self, this: &AnyObject, sel: Sel, event: *mut AnyObject) {
        let Some(original) = self.original.get().copied() else {
            return;
        };
        let ran = panic::catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: the original is called as it would have been.
            exception::catch(AssertUnwindSafe(|| unsafe { original(this, sel, event) }))
        }));
        match ran {
            Ok(Ok(())) => {}
            Ok(Err(exception)) => super::stopped(&format!(
                "{} raised {}",
                self.edge,
                described(exception.as_deref())
            )),
            Err(_) => super::stopped(&format!("{} panicked", self.edge)),
        }
    }
}

/// An exception as it is written: its class, name and reason, and the stack it was raised on.
fn described(exception: Option<&Exception>) -> String {
    let Some(exception) = exception else {
        return "an Objective-C exception that is nil".to_string();
    };
    let mut text = format!("{exception:?}");
    if !exception.class().responds_to(sel!(callStackSymbols)) {
        return text;
    }
    objc2::rc::autoreleasepool(|_| {
        // SAFETY: an NSException's `callStackSymbols` is an array of strings, or nil.
        let symbols: Option<Retained<AnyObject>> =
            unsafe { msg_send![exception, callStackSymbols] };
        let Some(symbols) = symbols else {
            return;
        };
        text.push_str("\nraised on:");
        // SAFETY: as above.
        let count: usize = unsafe { msg_send![&*symbols, count] };
        for index in 0..count {
            // SAFETY: as above; `UTF8String` is the string's own, read before the pool ends.
            let line = unsafe {
                let symbol: Retained<AnyObject> = msg_send![&*symbols, objectAtIndex: index];
                let utf8: *const std::ffi::c_char = msg_send![&*symbol, UTF8String];
                if utf8.is_null() {
                    continue;
                }
                CStr::from_ptr(utf8).to_string_lossy().into_owned()
            };
            text.push('\n');
            text.push_str(&line);
        }
    });
    text
}

/// Whether the guards' originals are `application` and `window`, AppKit's own.
#[cfg(test)]
pub(crate) fn guards_appkits(application: Imp, window: Imp) -> bool {
    let holds = |guarded: &Guarded, imp: Imp| {
        guarded
            .original
            .get()
            .is_some_and(|original| *original as usize == imp as usize)
    };
    holds(&APPLICATION, application) && holds(&WINDOW, window)
}
