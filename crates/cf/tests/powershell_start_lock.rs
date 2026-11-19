#[path = "../../../tests/support/powershell_start_lock.rs"]
mod powershell_start_lock;

use std::sync::mpsc;
use std::time::Duration;

#[test]
fn two_shell_starts_in_the_test_run_do_not_overlap() {
    let (first_in, first_ready) = mpsc::channel();
    let (release_first, first_release) = mpsc::channel();
    let (second_attempt, second_attempted) = mpsc::channel();
    let (second_in, second_ready) = mpsc::channel();
    let first = std::thread::spawn(move || {
        powershell_start_lock::one_at_a_time(|| {
            first_in.send(()).unwrap();
            first_release.recv().unwrap();
        });
    });
    first_ready.recv().unwrap();
    let second = std::thread::spawn(move || {
        second_attempt.send(()).unwrap();
        powershell_start_lock::one_at_a_time(|| second_in.send(()).unwrap());
    });
    second_attempted.recv().unwrap();
    let overlapped = second_ready
        .recv_timeout(Duration::from_millis(250))
        .is_ok();
    release_first.send(()).unwrap();
    first.join().unwrap();
    second.join().unwrap();
    assert!(
        !overlapped,
        "the second shell started before the first finished starting"
    );
}

#[test]
#[allow(clippy::disallowed_methods)] // Starts only this test's Node stand-in.
fn node_and_rust_take_the_same_run_lock() {
    use std::process::{Child, Command, Stdio};
    use std::time::Instant;

    struct Probe(Child);
    impl Drop for Probe {
        fn drop(&mut self) {
            if self.0.try_wait().is_ok_and(|status| status.is_none()) {
                let _ = self.0.kill();
            }
            let _ = self.0.wait();
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let ready = dir.path().join("ready");
    let entered = dir.path().join("entered");
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/support/powershell-lock-probe.mjs");
    let mut probe = None;
    powershell_start_lock::one_at_a_time(|| {
        let child = Command::new("node")
            .arg(script)
            .arg(&ready)
            .arg(&entered)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        probe = Some(Probe(child));
        let until = Instant::now() + Duration::from_secs(10);
        while !ready.is_file() {
            assert!(Instant::now() < until, "the Node stand-in did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until {
            assert!(!entered.exists(), "Node entered while Rust held the lock");
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let mut probe = probe.unwrap();
    assert!(probe.0.wait().unwrap().success());
    assert!(entered.is_file(), "Node never acquired the released lock");
}

#[test]
#[allow(clippy::disallowed_methods)] // Only this test's two stand-in processes are started.
fn fallback_lock_is_shared_between_node_and_rust_without_changing_existing_file() {
    use std::fs;
    use std::process::{Child, Command, Stdio};
    use std::time::Instant;

    if std::env::var_os("CF_TEST_FALLBACK_PROBE").is_some() {
        let entered = std::env::var_os("CF_TEST_ENTERED").unwrap();
        let release = std::env::var_os("CF_TEST_RELEASE").unwrap();
        powershell_start_lock::one_at_a_time(|| {
            fs::write(entered, "").unwrap();
            let until = Instant::now() + Duration::from_secs(15);
            while !std::path::Path::new(&release).exists() {
                assert!(
                    Instant::now() < until,
                    "the parent did not release the probe"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        return;
    }

    struct Probe(Child);
    impl Drop for Probe {
        fn drop(&mut self) {
            if self.0.try_wait().is_ok_and(|status| status.is_none()) {
                let _ = self.0.kill();
            }
            let _ = self.0.wait();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let lock = dir.path().join("consensflow-pwsh-start-v1.lock");
    fs::write(&lock, "keep").unwrap();
    let rust_entered = dir.path().join("rust-entered");
    let release = dir.path().join("release");
    let node_ready = dir.path().join("node-ready");
    let node_entered = dir.path().join("node-entered");
    let mut rust = Probe(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "fallback_lock_is_shared_between_node_and_rust_without_changing_existing_file",
            ])
            .env("CF_TEST_FALLBACK_PROBE", "1")
            .env("CF_TEST_ENTERED", &rust_entered)
            .env("CF_TEST_RELEASE", &release)
            .env_remove("CF_PWSH_START_LOCK")
            .env("TEMP", dir.path())
            .env("TMP", dir.path())
            .env("TMPDIR", dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let wait_for = |file: &std::path::Path| {
        let until = Instant::now() + Duration::from_secs(15);
        while !file.exists() {
            assert!(Instant::now() < until, "{} was never made", file.display());
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    wait_for(&rust_entered);
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/support/powershell-lock-probe.mjs");
    let mut node = Probe(
        Command::new("node")
            .arg(script)
            .arg(&node_ready)
            .arg(&node_entered)
            .env_remove("CF_PWSH_START_LOCK")
            .env("TEMP", dir.path())
            .env("TMP", dir.path())
            .env("TMPDIR", dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(&node_ready);
    let until = Instant::now() + Duration::from_millis(500);
    while Instant::now() < until {
        assert!(
            !node_entered.exists(),
            "Node entered while Rust held the fallback lock"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(release, "").unwrap();
    assert!(rust.0.wait().unwrap().success());
    assert!(node.0.wait().unwrap().success());
    assert!(node_entered.is_file());
    assert_eq!(fs::read_to_string(lock).unwrap(), "keep");
}

#[cfg(unix)]
#[test]
#[allow(clippy::disallowed_methods)] // Starts this test binary as its own stand-in.
fn a_link_at_the_rust_lock_path_is_refused() {
    use std::os::unix::fs::symlink;
    use std::process::{Command, Stdio};

    if std::env::var_os("CF_TEST_LOCK_LINK").is_some() {
        powershell_start_lock::one_at_a_time(|| {});
        return;
    }
    let folder = tempfile::tempdir().unwrap();
    let outside = folder.path().join("another-file");
    let link = folder.path().join("lock");
    std::fs::write(&outside, "keep").unwrap();
    symlink(&outside, &link).unwrap();
    let ran = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "a_link_at_the_rust_lock_path_is_refused"])
        .env("CF_TEST_LOCK_LINK", "1")
        .env("CF_PWSH_START_LOCK", link)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!ran.success(), "a symlink must not be opened as the lock");
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "keep");
}
