//! What the pane host's tests share of starting programs: a command that holds nothing of the
//! test process's but its streams. The tests run side by side and open sockets, pipes and
//! terminals; on macOS each is made close-on-exec in a second step, and a program a test started
//! between the two would hold what another test closed (a terminal that never hangs up).

use std::ffi::OsStr;
use std::process::Command;

/// The command a test starts `program` with, holding only its streams.
#[allow(clippy::disallowed_methods)] // A test's own programs.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    cf_process::hold_only_the_streams(&mut command);
    command
}

/// A program a test starts holds nothing of the test's but its streams: here a descriptor is
/// left open and not close-on-exec as it starts, and it says whether it has it.
#[cfg(target_os = "macos")]
#[test]
fn a_program_a_test_starts_holds_nothing_of_the_tests_but_its_streams() {
    use std::os::fd::AsRawFd;

    let stray = std::fs::File::open("/dev/null").expect("a file");
    let fd = stray.as_raw_fd();
    // SAFETY: `fcntl` is asked to clear the flags of a descriptor this test holds.
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
    let said = command("/bin/sh")
        .args([
            "-c".to_owned(),
            format!("if [ -e /dev/fd/{fd} ]; then echo held; else echo clear; fi"),
        ])
        .output()
        .expect("the shell runs");
    assert_eq!(String::from_utf8_lossy(&said.stdout), "clear\n");
}

/// How long a test waits for something its program does, which only a hang runs into: no case
/// reads this as a time to be quick in.
pub(crate) const LIVENESS: std::time::Duration = std::time::Duration::from_secs(30);

/// A pane's program that reads a human's paste raw and says nothing of it, as Codex does on
/// Windows, and asks the terminal where its cursor is as the paste comes (T-1048): it reads
/// the `paste_len` bytes of the paste and makes the `marker` file, reads the one byte of the
/// terminal's answer, **draws only then** (`drawn`), and reads the human's Enter. It ends with
/// one line, the hex of each: the paste, the answer, the Enter, and stays (`sleep`), so that
/// its pane is there for the test to read it from until the test kills it.
///
/// What the reply tests (`a_terminals_reply_goes_past…`) wait on is these events and not a
/// time: the marker says the paste is in the program, so the human's Enter is held behind it;
/// the draw comes only if the answer got past the held Enter, since the Enter is held until the
/// window draws. An answer that waited behind the Enter would wait for the cap
/// ([`HELD_FOR_MS`]), however slow or fast the machine.
pub(crate) struct SilentReader {
    marker: std::path::PathBuf,
    paste_len: usize,
}

/// The most the human's Enter is held after a paste in these tests: two minutes, past anything
/// a loaded machine takes to run the test's few steps, so that the Enter goes because the
/// window drew the paste and not because the cap came. An answer behind it is found by
/// [`LIVENESS`], well before the cap.
pub(crate) const HELD_FOR_MS: u64 = 120_000;

impl SilentReader {
    pub(crate) fn new(label: &str, paste_len: usize) -> Self {
        // Named by the process and a count, so that no two readers of a run share a file.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let marker = std::env::temp_dir().join(format!(
            "cf-panes-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        Self { marker, paste_len }
    }

    /// The program, as the words `sh -c` takes.
    pub(crate) fn script(&self) -> String {
        format!(
            "stty raw -echo; printf ready; \
             p=$(dd bs=1 count={} 2>/dev/null | od -An -tx1); : > '{}'; \
             a=$(dd bs=1 count=1 2>/dev/null | od -An -tx1); printf drawn; \
             e=$(dd bs=1 count=1 2>/dev/null | od -An -tx1); echo \"$p $a $e\"; sleep 600",
            self.paste_len,
            self.marker.display()
        )
    }

    /// Waits until the program has read the human's paste. Only a hang runs out of time.
    pub(crate) fn wait_pasted(&self) {
        let deadline = std::time::Instant::now() + LIVENESS;
        while !self.marker.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "the program never read the paste"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

impl Drop for SilentReader {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.marker);
    }
}

/// The hex of `bytes` as `od -An -tx1` writes it with the whitespace out.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Waits for `receiver`'s answer for [`LIVENESS`] at most: a wait that runs out is a hang.
pub(crate) fn answered_within_liveness<T>(
    mut receiver: futures_channel::oneshot::Receiver<T>,
    what: &str,
) -> T {
    let deadline = std::time::Instant::now() + LIVENESS;
    loop {
        match receiver.try_recv() {
            Ok(Some(answer)) => return answer,
            Ok(None) => {}
            Err(_) => panic!("{what}: the queue ended before answering"),
        }
        assert!(std::time::Instant::now() < deadline, "{what}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
