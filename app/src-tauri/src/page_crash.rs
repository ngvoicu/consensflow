//! A page whose WebView2 process failed is loaded again, as macOS's runtime already loads a
//! page again when WebKit's web-content process ends (tauri-runtime-wry). Nothing on Windows
//! did: the page stayed gone, it acknowledged no window's output, and the pane host stops
//! reading a window once 1 MiB of its output is owed, so every agent that printed that much
//! waited on its terminal until the app restarted (T-454). The page loaded again subscribes
//! again, which lets the windows off what the page before was sent (`subscribe_output`).

//!
//! Every failure is said in the app's log, by its kind, whatever is done with it. The loads
//! are kept apart, and given up on when a page fails as fast as it is loaded
//! (`page_reloads`): a page that is given up on is said, and the app has to be started
//! again.

use std::time::Instant;

use tauri::{Runtime, WebviewWindow};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_PPAPI_BROKER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_PPAPI_PLUGIN_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    COREWEBVIEW2_PROCESS_FAILED_KIND_SANDBOX_HELPER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_UNKNOWN_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_UTILITY_PROCESS_EXITED,
};
use webview2_com::ProcessFailedEventHandler;

use crate::page_reloads::{Reload, Reloads, AT_MOST, WITHIN};

/// Whether a failure of `kind` is the page's to be loaded again: its own process ended or
/// stopped answering. The browser's process ending takes the whole web view with it, which
/// loading cannot bring back; a frame's, the GPU's or a helper's leaves the page running.
fn loads_again(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> bool {
    kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
        || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
}

/// The process a failure of `kind` was of, in words for the log.
fn process_of(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> &'static str {
    match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => {
            "the web view's browser process ended: the page is gone until the app starts again"
        }
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => "the page's process ended",
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => {
            "the page's process stopped answering"
        }
        COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED => {
            "the process of a frame of the page ended"
        }
        COREWEBVIEW2_PROCESS_FAILED_KIND_UTILITY_PROCESS_EXITED => "a utility process ended",
        COREWEBVIEW2_PROCESS_FAILED_KIND_SANDBOX_HELPER_PROCESS_EXITED => {
            "the sandbox's helper process ended"
        }
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED => "the GPU's process ended",
        COREWEBVIEW2_PROCESS_FAILED_KIND_PPAPI_PLUGIN_PROCESS_EXITED => "a plugin's process ended",
        COREWEBVIEW2_PROCESS_FAILED_KIND_PPAPI_BROKER_PROCESS_EXITED => {
            "a plugin broker's process ended"
        }
        COREWEBVIEW2_PROCESS_FAILED_KIND_UNKNOWN_PROCESS_EXITED => "a process of WebView2's ended",
        _ => "a process of WebView2's failed",
    }
}

/// What the log says of a failure of `kind`, and of what `reload` does with it (none for a
/// kind that is not loaded again).
fn said(kind: COREWEBVIEW2_PROCESS_FAILED_KIND, reload: Option<Reload>) -> String {
    let what = process_of(kind);
    let done = match reload {
        None => String::new(),
        Some(Reload::Now) => ": loading it again".into(),
        Some(Reload::After(wait)) => format!(
            ": loading it again in {:.1} s, as loads are kept apart",
            wait.as_secs_f64()
        ),
        Some(Reload::Due) => ": it is loaded again already".into(),
        Some(Reload::GivenUp) => format!(
            ": it was loaded again {AT_MOST} times in {} s, and is not loaded again; \
             start the app again",
            WITHIN.as_secs()
        ),
        Some(Reload::GaveUp) => ": it is not loaded again (given up on)".into(),
    };
    format!("consensflow: {what} (kind {}){done}", kind.0)
}

/// Has `window`'s page loaded again whenever its process fails, saying so in the app's log.
/// A web view that cannot be asked is said in the log too, and the app runs on without it.
pub(crate) fn load_again_when_it_fails<R: Runtime>(window: &WebviewWindow<R>) {
    let later = window.clone();
    let asked = window.with_webview(move |platform| {
        let mut reloads = Reloads::default();
        let handler = ProcessFailedEventHandler::create(Box::new(move |webview, args| {
            let (Some(webview), Some(args)) = (webview, args) else {
                return Ok(());
            };
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            // SAFETY: the event's own arguments, alive for the call, written into a local.
            unsafe { args.ProcessFailedKind(&mut kind)? };
            let reload = loads_again(kind).then(|| reloads.failed(Instant::now()));
            eprintln!("{}", said(kind, reload));
            match reload {
                // SAFETY: the web view the event came from, on the thread it came on.
                Some(Reload::Now) => unsafe { webview.Reload()? },
                Some(Reload::After(wait)) => {
                    let window = later.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(wait);
                        if let Err(error) = window.reload() {
                            eprintln!(
                                "consensflow: the failed page could not be loaded again: {error}"
                            );
                        }
                    });
                }
                _ => {}
            }
            Ok(())
        }));
        let mut token = 0;
        // SAFETY: the window's own controller, alive while its web view is, asked on the
        // main thread `with_webview` runs on; the token is a local the call writes.
        let added = unsafe {
            platform
                .controller()
                .CoreWebView2()
                .and_then(|webview| webview.add_ProcessFailed(&handler, &mut token))
        };
        if let Err(error) = added {
            eprintln!("consensflow: a failed page will not be loaded again: {error}");
        }
    });
    // Asked of the event loop, which does not answer only once it has stopped: the app is
    // then ending, and starts with no reload rather than not at all.
    if let Err(error) = asked {
        eprintln!("consensflow: a failed page will not be loaded again: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_pages_own_process_ending_or_hanging_loads_it_again() {
        assert!(loads_again(
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
        ));
        assert!(loads_again(
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
        ));
        for kind in [
            COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
        ] {
            assert!(!loads_again(kind), "{}", kind.0);
        }
    }

    #[test]
    fn every_failure_is_said_by_its_kind_and_with_what_is_done_with_it() {
        use std::time::Duration;
        let ended = COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED;
        assert_eq!(
            said(ended, Some(Reload::Now)),
            "consensflow: the page's process ended (kind 1): loading it again"
        );
        assert_eq!(
            said(ended, Some(Reload::After(Duration::from_millis(4200)))),
            "consensflow: the page's process ended (kind 1): loading it again in 4.2 s, as \
             loads are kept apart"
        );
        assert!(said(ended, Some(Reload::GivenUp)).contains("start the app again"));
        assert_eq!(
            said(
                COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
                None
            ),
            "consensflow: the web view's browser process ended: the page is gone until the \
             app starts again (kind 0)"
        );
        // Every kind WebView2 names is said in words of its own, and one it may add later too.
        let kinds = (0..=9).map(COREWEBVIEW2_PROCESS_FAILED_KIND);
        let words: std::collections::HashSet<_> = kinds.clone().map(process_of).collect();
        assert_eq!(words.len(), 10);
        assert!(!words.contains(process_of(COREWEBVIEW2_PROCESS_FAILED_KIND(99))));
    }
}
