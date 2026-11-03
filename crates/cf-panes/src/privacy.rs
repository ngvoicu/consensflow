//! The watch of the system's privacy dialogs, as the app starts it (T-1194): while a window is
//! open, a reader of the system's log tells the daemon of a dialog raised in the app's name for
//! a process of a window, and of a log it cannot read. Only a Mac has it; elsewhere the watch
//! is a handle that does nothing, and the Windows leg is as it was.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cf_bridge::Bridge;

use crate::pty::PaneTable;

/// The watch, running. It ends when it is stopped or dropped.
pub struct PrivacyWatch {
    stop: Arc<AtomicBool>,
}

impl PrivacyWatch {
    /// Ends the watch: its reader is asked to end, and none is started again.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for PrivacyWatch {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts the watch of `panes`' windows, or says why its thread did not start. Its events go to
/// `bridge()`, whichever bridge is the daemon's when one is told (the daemon is started again, and
/// its bridge is another).
#[cfg(target_os = "macos")]
pub fn watch_privacy_dialogs(
    panes: Arc<PaneTable>,
    bridge: impl Fn() -> Option<Bridge> + Send + Sync + 'static,
) -> Result<PrivacyWatch, std::io::Error> {
    use crate::tcc::real::{BridgeTells, LogStream, TableWindows, ThreadTime};
    use crate::tcc::watch::{supervise, Stream};

    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    std::thread::Builder::new()
        .name("privacy-watch".to_owned())
        .spawn(move || {
            let start = |app: u32| -> Result<Box<dyn Stream>, String> {
                LogStream::of_the_log(app).map(|stream| Box::new(stream) as Box<dyn Stream>)
            };
            supervise(
                std::process::id(),
                &start,
                &TableWindows(panes),
                &BridgeTells(bridge),
                &ThreadTime,
                &flag,
            );
        })?;
    Ok(PrivacyWatch { stop })
}

/// Not a Mac: no dialog of the system's is watched.
#[cfg(not(target_os = "macos"))]
pub fn watch_privacy_dialogs(
    _panes: Arc<PaneTable>,
    _bridge: impl Fn() -> Option<Bridge> + Send + Sync + 'static,
) -> Result<PrivacyWatch, std::io::Error> {
    Ok(PrivacyWatch {
        stop: Arc::new(AtomicBool::new(true)),
    })
}
