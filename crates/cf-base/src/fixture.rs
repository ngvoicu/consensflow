//! Programs a test starts, to wait on, to end or to leave alone, that end when the process
//! that started them does (T-686, T-693). A test that is ended on its way (a harness whose
//! output is closed exits at once with its tests in flight, a run is killed) never sends the
//! signal such a program waits for, and a shell that ignored `TERM` and looped went on for
//! eight hours, two of them, and twenty-six that ignored `TERM` and `HUP` for four days. So a
//! program a test starts that does not end of itself looks at its owner, as these do; the
//! rule test `fixtures_end_with_their_owner` (cf-e2e) refuses the string of a test that
//! loops without.
//!
//! They are shell, for `sh -c`: the tests that start them run on a Mac or a Linux. Windows has
//! no shell to run them in, and its programs of a test end of themselves (`ping -n`).

/// The shell test that the process that started the script is there. `$PPID` is the parent
/// the script began with, and a subshell of it keeps that number, so it holds in a loop in
/// the background too. A script whose owner is not its parent (its parent is a shell that
/// ends at once, or a program the owner starts that is itself ended on purpose) names the
/// owner's number: [`owner_there`].
pub const OWNER_THERE: &str = "kill -0 $PPID 2>/dev/null";

/// The shell test that the process `owner` is there, for a script that does not run under
/// its owner.
#[must_use]
pub fn owner_there(owner: u32) -> String {
    format!("kill -0 {owner} 2>/dev/null")
}

/// A script that ignores `signals` (names, as `trap` takes them: `TERM HUP`), runs `first`
/// (none when empty) and then goes on, asleep, for as long as its owner is there. A program
/// that takes no notice of being asked to end, which the close of a window or the end of a
/// run must force, and that does not go on for ever when the test that was to force it is
/// gone.
#[must_use]
pub fn deaf(signals: &str, first: &str) -> String {
    let first = if first.is_empty() {
        String::new()
    } else {
        format!("{first}; ")
    };
    format!("trap '' {signals}; {first}while {OWNER_THERE}; do /bin/sleep 0.2; done")
}

#[cfg(all(test, unix))]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::*;

    /// Whether the process `pid` runs: one that has ended and waits to be reaped does not.
    #[allow(clippy::disallowed_methods)] // The test looks at what it starts.
    fn runs(pid: u32) -> bool {
        let there = Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        let zombie = Command::new("/bin/ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .is_ok_and(|ran| String::from_utf8_lossy(&ran.stdout).trim().starts_with('Z'));
        there && !zombie
    }

    /// Ends the process it holds, if it is still there, when it goes: a test that fails with
    /// the script running must not leave it.
    struct Left(u32);

    impl Drop for Left {
        #[allow(clippy::disallowed_methods)] // The test ends what it started.
        fn drop(&mut self) {
            if runs(self.0) {
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", &self.0.to_string()])
                    .status();
            }
        }
    }

    #[test]
    fn a_script_is_text_that_looks_at_its_owner() {
        assert_eq!(
            deaf("TERM HUP", "printf ready"),
            "trap '' TERM HUP; printf ready; while kill -0 $PPID 2>/dev/null; do /bin/sleep 0.2; done"
        );
        assert_eq!(
            deaf("TERM", ""),
            "trap '' TERM; while kill -0 $PPID 2>/dev/null; do /bin/sleep 0.2; done"
        );
        assert_eq!(owner_there(42), "kill -0 42 2>/dev/null");
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it probes.
    fn a_deaf_script_goes_on_when_asked_to_end_and_ends_when_its_owner_does() {
        // The owner starts the script, says its number and waits. The script says it is up
        // once it ignores the request: one sent before has the shell's own answer.
        let folder = tempfile::tempdir().unwrap();
        let up = folder.path().join("up");
        let mut owner = Command::new("/bin/sh")
            .args(["-c", "/bin/sh -c \"$SCRIPT\" & echo $!; wait"])
            .env(
                "SCRIPT",
                deaf("TERM", &format!("echo up > '{}'", up.display())),
            )
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut said = String::new();
        BufReader::new(owner.stdout.take().unwrap())
            .read_line(&mut said)
            .unwrap();
        let script: u32 = said.trim().parse().unwrap();
        let _left = Left(script);
        let until = Instant::now() + Duration::from_secs(20);
        while !up.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(up.exists(), "the script never came up");

        let asked = Command::new("/bin/kill")
            .args(["-TERM", &script.to_string()])
            .status()
            .unwrap();
        assert!(asked.success());
        std::thread::sleep(Duration::from_millis(600));
        assert!(runs(script), "it ignores the request to end");

        owner.kill().unwrap();
        owner.wait().unwrap();
        let until = Instant::now() + Duration::from_secs(20);
        while runs(script) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!runs(script), "it went on with its owner gone");
    }
}
