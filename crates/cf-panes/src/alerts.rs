//! The alerts the system's notification centre has on screen (T-1194). A privacy dialog is one:
//! a window of `UserNotificationCenter` at the alert level, drawn on the screen of the user
//! logged in. The list of windows names each one's owner and layer with no grant (no pixel is
//! read, which Screen Recording would be asked for), and says nothing of whose alert it is.
//! The daemon uses it to end the wait it holds a window in while a dialog is up: when none is
//! listed any more, the dialog was answered.

/// The program that draws the system's alerts, privacy dialogs among them.
const OWNER: &str = "UserNotificationCenter";

/// A window of the screen, as the list of windows names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Window {
    pub owner: String,
    pub layer: i32,
}

/// How many of `windows` are alerts: the notification centre's own, above the level of an
/// ordinary window.
pub(crate) fn count(windows: &[Window]) -> usize {
    windows
        .iter()
        .filter(|window| window.owner == OWNER && window.layer > 0)
        .count()
}

/// How many alerts are on screen now; none said where the system gave no list (no window
/// server: a Mac nobody is logged in at).
pub(crate) fn on_screen() -> Option<usize> {
    #[cfg(target_os = "macos")]
    {
        macos::windows().map(|windows| count(&windows))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{c_char, c_void};

    use super::Window;

    type CFTypeRef = *const c_void;

    const ON_SCREEN_ONLY: u32 = 1;
    const NULL_WINDOW: u32 = 0;
    const UTF8: u32 = 0x0800_0100;
    const SINT32: isize = 3;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
        static kCGWindowOwnerName: CFTypeRef;
        static kCGWindowLayer: CFTypeRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFArrayGetCount(array: CFTypeRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
        fn CFDictionaryGetValue(dictionary: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
        fn CFStringGetCString(
            string: CFTypeRef,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> u8;
        fn CFNumberGetValue(number: CFTypeRef, kind: isize, out: *mut c_void) -> u8;
        fn CFRelease(object: CFTypeRef);
    }

    /// The windows on screen, each with its owner's name and layer, or none where the system
    /// gives no list.
    pub(super) fn windows() -> Option<Vec<Window>> {
        // SAFETY: the call takes two integers; it returns a list the caller owns (a `Copy`
        // function), or null.
        let list = unsafe { CGWindowListCopyWindowInfo(ON_SCREEN_ONLY, NULL_WINDOW) };
        if list.is_null() {
            return None;
        }
        // SAFETY: `list` is a live CFArray of CFDictionary, held until the release below; every
        // value read from it is borrowed from it and copied before then.
        let windows = unsafe {
            (0..CFArrayGetCount(list))
                .filter_map(|index| {
                    let window = CFArrayGetValueAtIndex(list, index);
                    (!window.is_null()).then(|| read(window))
                })
                .collect()
        };
        // SAFETY: `list` is owned here and not used again.
        unsafe { CFRelease(list) };
        Some(windows)
    }

    /// One window's owner and layer.
    ///
    /// # Safety
    /// `window` is a live CFDictionary of the list of windows.
    unsafe fn read(window: CFTypeRef) -> Window {
        // SAFETY: both keys are constants of CoreGraphics, and `window` is live.
        let (owner, layer) = unsafe {
            (
                CFDictionaryGetValue(window, kCGWindowOwnerName),
                CFDictionaryGetValue(window, kCGWindowLayer),
            )
        };
        let mut name = [0 as c_char; 256];
        let owner = if owner.is_null() {
            String::new()
        } else {
            // SAFETY: `owner` is a CFString borrowed from the window; the buffer is writable for
            // its whole declared size.
            let copied = unsafe { CFStringGetCString(owner, name.as_mut_ptr(), 256, UTF8) };
            if copied == 0 {
                String::new()
            } else {
                // SAFETY: a successful copy leaves a NUL-terminated string in the buffer.
                unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }
                    .to_string_lossy()
                    .into_owned()
            }
        };
        let mut level: i32 = 0;
        if !layer.is_null() {
            // SAFETY: `layer` is a CFNumber borrowed from the window; `level` is writable for an
            // `i32`, which is the size of the kind asked.
            unsafe { CFNumberGetValue(layer, SINT32, std::ptr::from_mut(&mut level).cast()) };
        }
        Window {
            owner,
            layer: level,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(owner: &str, layer: i32) -> Window {
        Window {
            owner: owner.to_owned(),
            layer,
        }
    }

    #[test]
    fn an_alert_is_a_window_of_the_notification_centre_above_the_level_of_an_ordinary_window() {
        // Recorded on a Mac with two privacy dialogs up, among its other windows
        // (`CGWindowListCopyWindowInfo`, 2026-11-03): two alerts at layer 8, and windows of
        // ordinary apps at layer 0.
        let screen = [
            window("Window Server", -2_147_483_624),
            window("Safari", 0),
            window("UserNotificationCenter", 8),
            window("Finder", 0),
            window("UserNotificationCenter", 8),
            window("ConsensFlow", 0),
        ];
        assert_eq!(count(&screen), 2);
        assert_eq!(count(&screen[..2]), 0);
        // A window of the same program that is an ordinary window is no alert, nor is another
        // app's window at a high layer.
        assert_eq!(count(&[window("UserNotificationCenter", 0)]), 0);
        assert_eq!(count(&[window("Dock", 20)]), 0);
        assert_eq!(count(&[]), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_list_of_windows_is_read_without_a_grant_and_whatever_it_holds() {
        // On a Mac nobody is logged in at there is no list (none said); where there is one the
        // count is a count of it. Either way nothing is asked of the user.
        if let Some(windows) = macos::windows() {
            assert_eq!(
                on_screen(),
                Some(count(&macos::windows().unwrap_or(windows)))
            );
        } else {
            assert_eq!(on_screen(), None);
        }
    }
}
