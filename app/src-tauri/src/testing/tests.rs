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
    let said = super::command("/bin/sh")
        .args([
            "-c".to_owned(),
            format!("if [ -e /dev/fd/{fd} ]; then echo held; else echo clear; fi"),
        ])
        .output()
        .expect("the shell runs");
    assert_eq!(String::from_utf8_lossy(&said.stdout), "clear\n");
}
