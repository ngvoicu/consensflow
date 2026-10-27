//! What macOS lets this process do with the screen: control it (Accessibility)
//! and see it (Screen & System Audio Recording). macOS keeps both for the app a
//! process is attributed to, the one that started it (ConsensFlow for its daemon
//! and every program in its panes, a terminal for what runs in it), so what this
//! process is granted is what a pane's program is. Asking prompts nothing: both
//! calls only read.

/// Whether this process may control the computer and see the screen, as macOS
/// says now; none where the system keeps no such grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenGrants {
    pub accessibility: bool,
    pub screen_recording: bool,
}

/// What macOS grants this process now.
#[cfg(target_os = "macos")]
pub fn screen_grants() -> Option<ScreenGrants> {
    #[link(name = "ApplicationServices", kind = "framework")]
    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
    }
    // SAFETY: both take nothing, return a bool, and only read the process's
    // grants (neither shows a prompt).
    #[allow(unsafe_code)]
    let (accessibility, screen_recording) =
        unsafe { (AXIsProcessTrusted(), CGPreflightScreenCaptureAccess()) };
    Some(ScreenGrants {
        accessibility,
        screen_recording,
    })
}

/// No system but macOS keeps these grants.
#[cfg(not(target_os = "macos"))]
pub fn screen_grants() -> Option<ScreenGrants> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The calls link and answer: on macOS with what the system says (a test
    /// runner is attributed to its terminal, which may have either), elsewhere none.
    #[test]
    fn the_grants_are_read_on_macos_and_none_elsewhere() {
        let said = screen_grants();
        assert_eq!(said.is_some(), cfg!(target_os = "macos"));
        assert_eq!(screen_grants(), said, "asking again says the same");
    }
}
