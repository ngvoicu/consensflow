//! What a program started here is left of this one's descriptors: its standard
//! streams, and nothing else.
//!
//! macOS cannot open a socket or a pipe close-on-exec: std opens it, and makes it so
//! in a second call. A program started on another thread between the two is left the
//! descriptor, open, and keeps it for as long as it runs. A listener this process closed
//! goes on taking connections, and nothing serves them; its port cannot be bound again;
//! the end of a pipe that is closed here is not the end of it, and whoever reads the
//! other side waits for it. Seen in the tests as a closed feed that still answered, a
//! free port that was taken, and a host that would not end with its input; nothing in
//! the product is spared, for the daemon starts programs while its other threads open
//! sockets and pipes.
//!
//! So a program, in the moment between its start and its exec, closes every descriptor it
//! holds above the standard streams that is not close-on-exec: the copy it was left, which
//! whatever this process does to its own after is no matter to. (The close-on-exec ones
//! are left to the exec, as std's own pipe that tells the start's failure must be.) That
//! is done in the new process, which is why it is started by fork and exec (std does, for
//! a program that has something to run first): macOS offers `POSIX_SPAWN_CLOEXEC_DEFAULT`
//! to the same end, through no call std makes.
//!
//! What this does not do is the moment before: a program that is being started when a
//! descriptor is closed here holds its copy until it gets to run. A program started by fork
//! and exec holds every descriptor of this process, close-on-exec or not, from the fork to
//! the exec, a few milliseconds on a loaded Mac, and one started by `posix_spawn` holds
//! them for the one call, a fraction of that. Measured on this crate's own tests, a pipe
//! closed here and asked about a moment after was still held by a program in flight in 36
//! of 150 runs of the suite when every program was started so, and in 2 of 150 when none
//! was. That is the price, so it is paid by the programs that run beside this one for as
//! long as it does ([`crate::spawn`]: the pane host, a harness's server) and by the ones let
//! go ([`crate::hand_over`]), whose hold would last as long as they do, and not by the
//! ones run to their end ([`crate::capture`]), whose hold lasts as long as they run. A check
//! that something is closed, made at once, is made against that: the tests ask the thing
//! itself, or wait for it.
//!
//! Linux makes a socket or a pipe close-on-exec as it opens it, and Windows hands
//! nothing over that is not asked: there this does nothing.

use std::process::Command;

/// Makes `command` start its program with no descriptor of this process's above the
/// standard streams. The programs started to run beside this one and the ones let go are
/// so; a command built by hand that starts a program for long (the app's daemon) asks for it.
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

    /// How far a program that cannot be listed is looked in for descriptors, at the most.
    const SCANNED: c_int = 10_240;

    pub(super) fn hold_only_the_streams(command: &mut Command) {
        // SAFETY: the closure runs in the new process, between fork and exec, where only
        // functions that are safe to call from a signal handler may be called, and it
        // makes only those: `getpid`, `proc_pidinfo` (a system call), `getdtablesize`
        // and `fcntl`. It allocates nothing and takes no lock; its listing is on the
        // stack.
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
        // SAFETY: no pointer is passed.
        #[allow(unsafe_code)]
        let table = unsafe { libc::getdtablesize() };
        let listed = usize::try_from(bytes).map_or(0, |bytes| bytes / size);
        if listed == 0 {
            // The system would not list them.
            scan(3, table.min(SCANNED));
            return Ok(());
        }
        let mut highest = 2;
        for entry in &listing[..listed.min(LISTED)] {
            highest = highest.max(entry.proc_fd);
            let_go(entry.proc_fd);
        }
        if listed >= LISTED {
            // A listing that is full may have left some out, above the last it names.
            scan(highest + 1, table);
        }
        Ok(())
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
