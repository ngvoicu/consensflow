//! What a program started here is left of this one's descriptors: its standard
//! streams, and nothing else.
//!
//! macOS cannot open a socket, a pipe or a terminal close-on-exec: std opens a socket or a
//! pipe and makes it so in a second call, and portable-pty does the same with a terminal. A
//! program started on another thread between the two is left the descriptor, open, and keeps
//! it for as long as it runs. A listener this process closed goes on taking connections, and
//! nothing serves them; its port cannot be bound again; the end of a pipe that is closed here
//! is not the end of it, and whoever reads the other side waits for it; a terminal whose
//! window was closed never hangs up. Seen in the tests as a closed feed that still answered, a
//! free port that was taken, and a host that would not end with its input.
//!
//! Where that is in the product is the app, which runs many threads: the pane host's make the
//! windows' terminals, and the main thread starts the daemon, which runs as long as the app
//! does and would hold a window's terminal past its close. The daemon's start (the app's
//! daemon command) is where this matters, and the app's other starts beside it: the login
//! shell it asks for its PATH (a login file may start a program that runs on), the tools an
//! update is checked with, the app an update installed, started as this one ends (it would
//! hold what this one had for as long as the human keeps it open; Tauri's own restart starts
//! it bare), and the page a release is downloaded from. The daemon and the Codex supervisor
//! start their programs on the one thread that opens their sockets and pipes, so no start of
//! theirs falls between the two calls today; every program is started so all the same, for a
//! second thread there would open the window with nothing to say so, and the app may start
//! programs through this crate. So are the test rig's, whose cases run side by side.
//!
//! So a program, in the moment between its start and its exec, closes every descriptor it
//! holds above the standard streams that is not close-on-exec: the copy it was left, which
//! whatever this process does to its own after is no matter to. (The close-on-exec ones are
//! left to the exec, as std's own pipe that tells the start's failure must be.) That is done
//! in the new process, which is why it is started by fork and exec (std does, for a program
//! that has something to run first). macOS offers `POSIX_SPAWN_CLOEXEC_DEFAULT` to the same
//! end, through no call std makes: a start of this crate's own by `posix_spawn` would be a
//! process outside tokio's, its wait, its streams and its end when dropped written again, at
//! every place a program starts.
//!
//! The system lists them; a process with more than one listing holds has the rest asked for
//! one by one, as far as its table goes, which the system says too. Not as far as its limit on
//! descriptors, nor the system's on any process (`kern.maxfilesperproc`): each is lowered as
//! easily below one that is open as above it. A program whose start the system will not tell
//! these (a sandbox that denies `process-info`) is not started: it fails with the system's
//! word, `EPERM`, rather than run holding what it may.
//!
//! What this does not do is the moment before. A program started by fork holds every
//! descriptor of this process, close-on-exec or not, from the fork to the exec; one started by
//! `posix_spawn` holds them for the one call. Measured on this Mac under load (a load average
//! of 21 to 27), 300 starts of `/usr/bin/true` three times over (`what_a_start_by_fork_costs`,
//! in a test build): `posix_spawn` returns in 0.15 ms at the median, 0.2 to 0.5 ms at the 99th
//! of them; a start by fork, the hook run, in 0.9 to 1.0 ms, 1.3 to 2.3 ms at the 99th. 256
//! MiB more of this process resident changes neither; 4,200 descriptors more open make them
//! 0.3 ms and 1.5 to 1.7 ms. So a start costs about a millisecond more, and in that
//! millisecond the program holds what this process has. A descriptor closed here in that
//! moment is held until the program gets to run: a listener takes connections into its
//! backlog, a pipe is not at its end. One of them is std's own: the pipe a start by fork is
//! told its exec's failure on, read here to its end, is open and not yet close-on-exec for a
//! moment as it is made, and a program another thread starts in that moment by a start that
//! does not close what it was left keeps the writing end for as long as it runs: the start
//! here waits for that program to end. A check that something is closed, made at once, is made
//! against all that: the tests ask the thing itself, or wait for it.
//!
//! Linux makes a socket or a pipe close-on-exec as it opens it, and Windows hands
//! nothing over that is not asked: there this does nothing.

use std::process::Command;

/// Makes `command` start its program with no descriptor of this process's above the
/// standard streams. Every program this crate starts is so; a command built by hand asks for
/// it (the app's daemon, its login shell, the tools of its updates, the app it starts again and
/// its page opener; Codex's server and TUI; the rig's programs).
pub fn hold_only_the_streams(command: &mut Command) {
    #[cfg(target_os = "macos")]
    macos::hold_only_the_streams(command);
    #[cfg(not(target_os = "macos"))]
    let _ = command;
}

#[cfg(target_os = "macos")]
mod macos {
    use std::io;
    use std::mem::size_of;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    use libc::{c_int, c_void, proc_fdinfo};

    /// How many descriptors one listing holds: a program with more has the rest found by
    /// asking for each. The listing is on the stack, for the new process may not allocate.
    const LISTED: usize = 4096;

    pub(super) fn hold_only_the_streams(command: &mut Command) {
        // SAFETY: the closure runs in the new process, between fork and exec, where only
        // functions that are safe to call from a signal handler may be called, and it
        // makes only those: `getpid`, `proc_pidinfo` (a system call), `fcntl` and `close`.
        // It allocates nothing and takes no lock; its listing is on the stack.
        #[allow(unsafe_code)]
        unsafe {
            command.pre_exec(close_on_exec_all_above_the_streams);
        }
    }

    fn close_on_exec_all_above_the_streams() -> io::Result<()> {
        let mut listing = [proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0,
        }; LISTED];
        let size = size_of::<proc_fdinfo>();
        // SAFETY: the buffer is `LISTED` records long, and says so.
        #[allow(unsafe_code)]
        let bytes = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                libc::PROC_PIDLISTFDS,
                0,
                listing.as_mut_ptr().cast::<c_void>(),
                (size * LISTED) as c_int,
            )
        };
        let listed = usize::try_from(bytes).map_or(0, |bytes| bytes / size);
        let first = if listed == 0 {
            // The system would not list them: every one is asked for.
            3
        } else {
            let mut highest = 2;
            for entry in &listing[..listed.min(LISTED)] {
                highest = highest.max(entry.proc_fd);
                let_go(entry.proc_fd);
            }
            if listed < LISTED {
                return Ok(());
            }
            // A listing that is full may have left some out, above the last it names.
            highest + 1
        };
        // As far as the table goes. Not the limit on descriptors, nor the system's on any
        // process's: each is lowered as easily below one that is open as above it. When the
        // table's size is not known, the program is not started, rather than started holding
        // what it may.
        let end = table().ok_or_else(io::Error::last_os_error)?;
        scan(first, end);
        Ok(())
    }

    /// How many descriptors this process's table has room for, as the system says when it
    /// is asked how big a listing of them would be: every one open is below it.
    fn table() -> Option<c_int> {
        // SAFETY: no buffer is passed, and none is written: the answer is the size one
        // would need.
        #[allow(unsafe_code)]
        let bytes = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                libc::PROC_PIDLISTFDS,
                0,
                std::ptr::null_mut(),
                0,
            )
        };
        (bytes > 0).then(|| bytes / size_of::<proc_fdinfo>() as c_int)
    }

    /// Every descriptor from `first` to below `end`, asked for one by one.
    fn scan(first: c_int, end: c_int) {
        for fd in first..end {
            let_go(fd);
        }
    }

    /// Closes the descriptor `fd` if it is open, is not one of the standard streams and is
    /// not close-on-exec.
    fn let_go(fd: c_int) {
        if fd <= 2 {
            return;
        }
        // SAFETY: `fcntl` is asked about a number, and answers -1 for one that is not open;
        // what is closed is a copy this process's new self was left, of a descriptor that is
        // none of the program's.
        #[allow(unsafe_code)]
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
                libc::close(fd);
            }
        }
    }
}

#[cfg(test)]
mod tests;
