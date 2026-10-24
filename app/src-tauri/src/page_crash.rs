//! A page whose WebView2 process failed is loaded again, as macOS's runtime already loads a
//! page again when WebKit's web-content process ends (tauri-runtime-wry). Nothing on Windows
//! did: the page stayed gone, it acknowledged no window's output, and the pane host stops
//! reading a window once 1 MiB of its output is owed, so every agent that printed that much
//! waited on its terminal until the app restarted (T-454). The page loaded again subscribes
//! again, which lets the windows off what the page before was sent (`subscribe_output`).

use tauri::{Runtime, WebviewWindow};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
};
use webview2_com::ProcessFailedEventHandler;

/// Whether a failure of `kind` is the page's to be loaded again: its own process ended or
/// stopped answering. The browser's process ending takes the whole web view with it, which
/// loading cannot bring back; a frame's, the GPU's or a helper's leaves the page running.
fn loads_again(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> bool {
    kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
        || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
}

/// Has `window`'s page loaded again whenever its process fails, saying so in the app's log.
/// A web view that cannot be asked is said in the log too, and the app runs on without it.
pub(crate) fn load_again_when_it_fails<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    window.with_webview(|platform| {
        let handler = ProcessFailedEventHandler::create(Box::new(|webview, args| {
            let (Some(webview), Some(args)) = (webview, args) else {
                return Ok(());
            };
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            // SAFETY: the event's own arguments, alive for the call, written into a local.
            unsafe { args.ProcessFailedKind(&mut kind)? };
            if loads_again(kind) {
                eprintln!(
                    "consensflow: the page's process failed (kind {}): loading it again",
                    kind.0
                );
                // SAFETY: the web view the event came from, on the thread it came on.
                unsafe { webview.Reload()? };
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
    };

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
}
