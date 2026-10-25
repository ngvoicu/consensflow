//! What a terminal holds that its reader has not read: how many bytes the program on
//! it wrote that the side that reads its output (a window's pane host, holding the
//! pseudoterminal's other end) has not taken yet. A program that writes to a terminal
//! nobody reads waits in its write once the terminal's queue is full, and so does its
//! whole event loop if it writes as Node does on Unix: the long run asks this of a
//! window it reads at work for too long (its stall evidence).
//!
//! macOS says it of the program's own end of the terminal (`TIOCOUTQ`), which any
//! process of the owner may open by its device. It is opened without being made anyone's
//! controlling terminal, asked, and closed in the same call, and never read or written:
//! held open, it would keep the reader from seeing the program's end. Linux's
//! pseudoterminal keeps no such count on that end, and Windows' output goes through a
//! console host into a pipe only the reader holds: neither is asked.

#[cfg(target_os = "macos")]
use std::io;
#[cfg(target_os = "macos")]
use std::path::Path;

/// How many bytes the program on the terminal `device` (`/dev/ttys014`) wrote that its
/// reader has not read; or why the terminal could not be asked.
#[cfg(target_os = "macos")]
pub fn unread(device: &Path) -> io::Result<u64> {
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;

    use nix::libc;

    let opened = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
        .open(device)?;
    let mut queued: libc::c_int = 0;
    #[allow(unsafe_code)]
    // SAFETY: the descriptor is `opened`'s, open for this call; TIOCOUTQ writes one int,
    // and `queued` is one, writable.
    let said = unsafe { libc::ioctl(opened.as_raw_fd(), libc::TIOCOUTQ, &mut queued) };
    let asked = if said == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(u64::try_from(queued).unwrap_or_default())
    };
    drop(opened);
    asked
}
