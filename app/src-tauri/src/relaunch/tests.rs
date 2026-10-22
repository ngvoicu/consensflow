use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use super::*;

/// A bundle whose Info.plist names `renamed` its executable, which is `body` as a shell script.
fn bundle_naming(folder: &Path, body: &str) -> PathBuf {
    let app = folder.join("ConsensFlow.app");
    let macos = app.join("Contents/MacOS");
    std::fs::create_dir_all(&macos).expect("the bundle");
    std::fs::write(
        app.join("Contents/Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>renamed</string></dict></plist>"#,
    )
    .expect("its Info.plist");
    let executable = macos.join("renamed");
    std::fs::write(&executable, format!("#!/bin/sh\n{body}\n")).expect("its executable");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
        .expect("an executable");
    macos.join("app")
}

/// The app an update installed is started as this one ends, while the app's other threads open
/// sockets and pipes: here a descriptor is left open and not close-on-exec as it starts, and it
/// says whether it has it, and what it was started with. It is the executable the bundle names
/// now, not the one that ran.
#[test]
fn the_app_started_again_is_the_bundles_executable_and_holds_nothing_of_this_ones() {
    let folder = tempfile::tempdir().expect("a folder");
    let said = folder.path().join("said");
    let stray = std::fs::File::open("/dev/null").expect("a file");
    let fd = stray.as_raw_fd();
    // SAFETY: `fcntl` is asked to clear the flags of a descriptor this test holds.
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
    let exe = bundle_naming(
        folder.path(),
        &format!(
            "if [ -e /dev/fd/{fd} ]; then held=held; else held=clear; fi; echo \"$held $*\" > '{}'",
            said.display()
        ),
    );
    // The script was written a moment ago, and a program started on another thread meanwhile
    // can hold it open for writing: such a start is refused, and tried again.
    let mut started = (0..50)
        .find_map(
            |_| match replacement(&exe, [OsString::from("--updated")]).spawn() {
                Ok(started) => Some(started),
                Err(busy) if busy.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(Duration::from_millis(100));
                    None
                }
                Err(refused) => panic!("the app was not started again: {refused}"),
            },
        )
        .expect("the app started again");
    assert!(started.wait().expect("its end").success());
    assert_eq!(
        std::fs::read_to_string(&said).expect("what it said"),
        "clear --updated\n"
    );
}

#[test]
fn an_app_outside_a_bundle_is_started_again_as_it_ran() {
    let command = replacement(Path::new("/opt/consensflow/app"), [OsString::from("--x")]);
    assert_eq!(command.get_program(), "/opt/consensflow/app");
    assert_eq!(command.get_args().collect::<Vec<_>>(), ["--x"]);
}
