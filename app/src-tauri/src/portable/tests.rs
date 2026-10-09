use super::*;
use std::io::Read;
use std::sync::Arc;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

/// A runtime as the packer packs it: the native `cf`, whose bytes are `cf`,
/// and the terminals' console host, in a gzip-compressed tar.
fn payload(cf: &[u8]) -> Vec<u8> {
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let runtime: &[(&str, &[u8])] = &[
        ("cli/bin/cf.exe", cf),
        ("conpty.dll", b"the console host"),
        ("OpenConsole.exe", b"its process"),
        ("OpenConsole-LICENSE.txt", b"its license"),
    ];
    for (path, body) in runtime {
        let mut header = tar::Header::new_ustar();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        tar.append_data(&mut header, path, *body)
            .expect("add a file");
    }
    tar.into_inner()
        .expect("end the tar")
        .finish()
        .expect("end the gzip stream")
}

/// An exe as the packer writes it: the app, its payload, the footer.
fn packed(dir: &Path, payload: &[u8]) -> PathBuf {
    let exe = dir.join("ConsensFlow_9.9.9_x64-portable.exe");
    let mut bytes = b"MZ the app".to_vec();
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(&cf_portable::footer(payload.len() as u64));
    fs::write(&exe, bytes).expect("write the exe");
    exe
}

fn crc_of(payload: &[u8]) -> u32 {
    let mut tar = Vec::new();
    GzDecoder::new(payload)
        .read_to_end(&mut tar)
        .expect("gunzip the payload");
    let mut crc = flate2::Crc::new();
    crc.update(&tar);
    crc.sum()
}

fn entries(root: &Path) -> Vec<String> {
    let mut names = fs::read_dir(root)
        .expect("read the runtime root")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("a name")
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// An old runtime under `root`, as an earlier start unpacked it, with the
/// programs named in `present` in it as plain files.
fn old_runtime(root: &Path, name: &str, present: &[&[&str]]) -> PathBuf {
    let folder = root.join(name);
    fs::create_dir_all(folder.join("cli")).expect("an old runtime");
    for parts in present {
        let program = parts
            .iter()
            .fold(folder.clone(), |path, part| path.join(part));
        fs::create_dir_all(program.parent().expect("a folder")).expect("its folder");
        fs::write(program, b"old program").expect("its program");
    }
    folder
}

/// The installed app, and the Mac's, carry nothing: they find their
/// runtime beside them, and nothing is unpacked.
#[test]
fn an_exe_without_the_footer_carries_no_runtime() {
    let dir = tempfile::tempdir().expect("dir");
    let local = dir.path().join("local");
    for bytes in [
        &b"MZ"[..],
        &b"MZ an installed app, longer than a footer"[..],
    ] {
        let exe = dir.path().join("ConsensFlow.exe");
        fs::write(&exe, bytes).expect("write");
        assert_eq!(unpacked_runtime(&exe, &local, "9.9.9"), Ok(None));
    }
    assert!(!local.exists());
}

/// A footer naming more payload than the file holds, or less than any
/// gzip stream, is a damaged exe, not one that carries nothing.
#[test]
fn a_footer_that_cannot_be_right_is_an_error() {
    let dir = tempfile::tempdir().expect("dir");
    let exe = dir.path().join("ConsensFlow.exe");
    for length in [5_u64, 1_000] {
        let mut bytes = b"MZ the app and some".to_vec();
        bytes.extend_from_slice(&cf_portable::footer(length));
        fs::write(&exe, bytes).expect("write");
        let error =
            unpacked_runtime(&exe, &dir.path().join("local"), "9.9.9").expect_err("a damaged exe");
        assert!(
            error.starts_with("the bundled runtime could not be unpacked: its footer names"),
            "{error}"
        );
    }
}

/// The first start unpacks the `cf` and the console host, marker last; a
/// later start reuses the folder without reading the payload at all.
#[test]
fn the_first_start_unpacks_the_runtime_and_later_ones_reuse_it() {
    let dir = tempfile::tempdir().expect("dir");
    let local = dir.path().join("local");
    let root = local.join(RUNTIME_PARENT);
    let mut payload = payload(b"the native cf");
    let exe = packed(dir.path(), &payload);

    let folder = unpacked_runtime(&exe, &local, "9.9.9")
        .expect("unpacked")
        .expect("a runtime");
    assert_eq!(folder, root.join(format!("9.9.9-{:08x}", crc_of(&payload))));
    assert_eq!(
        fs::read(folder.join("cli/bin/cf.exe")).expect("cf"),
        b"the native cf"
    );
    for host in ["conpty.dll", "OpenConsole.exe", "OpenConsole-LICENSE.txt"] {
        assert!(folder.join(host).is_file(), "{host}");
    }
    // No Node is in the payload, and none is looked for.
    assert!(!folder.join("node.exe").exists());
    assert!(folder.join(".unpacked").is_file());
    assert_eq!(
        entries(&root),
        [folder.file_name().unwrap().to_str().unwrap()]
    );

    // Its compressed bytes damaged, its trailer kept: only a start that
    // unpacked again would notice.
    payload[12] ^= 0xff;
    let exe = packed(dir.path(), &payload);
    assert_eq!(unpacked_runtime(&exe, &local, "9.9.9"), Ok(Some(folder)));
}

/// gzip's CRC checks the payload: one whose tar does not match the CRC in
/// its trailer unpacks nothing and leaves nothing behind.
#[test]
fn a_payload_that_does_not_match_its_crc_unpacks_nothing() {
    let dir = tempfile::tempdir().expect("dir");
    let local = dir.path().join("local");
    let mut payload = payload(b"the native cf");
    let trailer = payload.len() - 8;
    payload[trailer] ^= 0xff;
    let exe = packed(dir.path(), &payload);

    let error = unpacked_runtime(&exe, &local, "9.9.9").expect_err("a damaged payload");
    assert!(
        error.starts_with("the bundled runtime could not be unpacked:"),
        "{error}"
    );
    assert_eq!(entries(&local.join(RUNTIME_PARENT)), Vec::<String>::new());
}

/// Copies of the app started at once each unpack, and the first to
/// finish names the folder; the others use it.
#[test]
fn copies_started_at_once_share_one_runtime() {
    let dir = tempfile::tempdir().expect("dir");
    let local = Arc::new(dir.path().join("local"));
    let exe = Arc::new(packed(dir.path(), &payload(&vec![7; 512 * 1024])));
    let copies = (0..4)
        .map(|_| {
            let (exe, local) = (Arc::clone(&exe), Arc::clone(&local));
            thread::spawn(move || unpacked_runtime(&exe, &local, "9.9.9"))
        })
        .collect::<Vec<_>>();
    let folders = copies
        .into_iter()
        .map(|copy| {
            copy.join()
                .expect("a copy")
                .expect("unpacked")
                .expect("a runtime")
        })
        .collect::<Vec<_>>();

    assert!(folders.iter().all(|folder| *folder == folders[0]));
    assert!(folders[0].join(".unpacked").is_file());
    assert_eq!(
        fs::read(folders[0].join("cli/bin/cf.exe"))
            .expect("cf")
            .len(),
        512 * 1024
    );
    assert_eq!(
        entries(&local.join(RUNTIME_PARENT)),
        [folders[0].file_name().unwrap().to_str().unwrap()]
    );
}

/// An unpacked runtime that cannot take its name for a moment (on
/// Windows, while an antivirus scans it) takes it once it can.
#[cfg(unix)]
#[test]
fn a_runtime_denied_its_name_for_a_moment_is_placed_once_it_can_be() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

    let dir = tempfile::tempdir().expect("dir");
    let root = dir.path().join("runtime");
    fs::create_dir_all(root.join(".unpacking-done")).expect("an unpacked runtime");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).expect("deny");
    let allowed = {
        let root = root.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).expect("allow");
        })
    };
    let started = Instant::now();
    let placed = place(&root.join(".unpacking-done"), &root.join("9.9.9-00000001"));
    allowed.join().expect("allowed");

    placed.expect("placed");
    assert!(started.elapsed() >= Duration::from_millis(200));
    assert_eq!(entries(&root), ["9.9.9-00000001"]);
}

/// A folder under the runtime's name without its marker, which no start
/// of the app leaves, is unpacked again.
#[test]
fn a_runtime_folder_without_its_marker_is_unpacked_again() {
    let dir = tempfile::tempdir().expect("dir");
    let local = dir.path().join("local");
    let root = local.join(RUNTIME_PARENT);
    let payload = payload(b"the native cf");
    let exe = packed(dir.path(), &payload);
    let damaged = root.join(format!("9.9.9-{:08x}", crc_of(&payload)));
    fs::create_dir_all(damaged.join("cli")).expect("a damaged runtime");
    fs::write(damaged.join("stray"), b"left").expect("a stray file");

    let folder = unpacked_runtime(&exe, &local, "9.9.9")
        .expect("unpacked")
        .expect("a runtime");
    assert_eq!(folder, damaged);
    assert!(folder.join(".unpacked").is_file());
    assert!(folder.join("cli/bin/cf.exe").is_file());
    assert!(!folder.join("stray").exists());
    assert_eq!(
        entries(&root),
        [folder.file_name().unwrap().to_str().unwrap()]
    );
}

/// The runtimes of this app have a folder of their own. The apps before the
/// flip release unpacked into `runtime` and emptied it of every runtime whose
/// `node.exe` was not running: nothing of this app is there for them to find,
/// and what they left there is left as it is, running or not.
#[test]
fn the_runtime_goes_under_a_parent_the_older_apps_collector_never_reads() {
    let dir = tempfile::tempdir().expect("dir");
    let local = dir.path().join("local");
    let older = local.join("runtime");
    let before = [
        old_runtime(&older, "9.9.8-00000001", &[&["node.exe"]]),
        old_runtime(
            &older,
            "9.9.8-00000002",
            &[&["node.exe"], &["cli", "bin", "cf.exe"]],
        ),
    ];
    let exe = packed(dir.path(), &payload(b"the native cf"));

    let folder = unpacked_runtime(&exe, &local, "9.9.9")
        .expect("unpacked")
        .expect("a runtime");

    assert_eq!(folder.parent(), Some(local.join(RUNTIME_PARENT).as_path()));
    assert_ne!(RUNTIME_PARENT, "runtime");
    assert_eq!(entries(&older), ["9.9.8-00000001", "9.9.8-00000002"]);
    for old in before {
        assert!(old.join("node.exe").is_file(), "{}", old.display());
    }
}

/// What the build's packer writes is what the app unpacks: the exe is made by
/// `cf_portable::pack` from a release folder, not laid out here.
#[test]
fn the_app_unpacks_the_exe_the_packer_wrote() {
    let dir = tempfile::tempdir().expect("dir");
    let local = dir.path().join("local");
    let release = dir.path().join("release");
    for (path, body) in [
        ("ConsensFlow.exe", "MZ the app"),
        ("cli/bin/cf.exe", "the native cf"),
        ("conpty.dll", "the console host"),
        ("OpenConsole.exe", "its process"),
        ("OpenConsole-LICENSE.txt", "its license"),
    ] {
        let file = release.join(path);
        fs::create_dir_all(file.parent().expect("a folder")).expect("its folder");
        fs::write(file, body).expect("its file");
    }
    let exe = dir.path().join("ConsensFlow_9.9.9_x64-portable.exe");
    let packed =
        cf_portable::pack(&release.join("ConsensFlow.exe"), &release, &exe).expect("packed");

    let folder = unpacked_runtime(&exe, &local, "9.9.9")
        .expect("unpacked")
        .expect("a runtime");

    assert_eq!(
        folder,
        local
            .join(RUNTIME_PARENT)
            .join(packed.payload.folder("9.9.9"))
    );
    assert_eq!(
        fs::read(folder.join("cli/bin/cf.exe")).expect("cf"),
        b"the native cf"
    );
    for host in ["conpty.dll", "OpenConsole.exe", "OpenConsole-LICENSE.txt"] {
        assert!(folder.join(host).is_file(), "{host}");
    }
    assert!(folder.join(".unpacked").is_file());
}

/// What the cleanup sees of "running" is a program that cannot be opened for
/// writing; here a read-only file stands in for it, for either program of a
/// runtime. It refuses every kind of open, an open for appending too, so it
/// cannot show which open the check makes: the tests that follow do, with a
/// file that may only be appended to (macOS) and with a program that runs
/// (Windows).
#[test]
fn a_runtime_with_a_program_that_cannot_be_written_stays_whole_and_the_others_go() {
    let dir = tempfile::tempdir().expect("dir");
    let local = dir.path().join("local");
    let root = local.join(RUNTIME_PARENT);
    let both = [&["node.exe"][..], &["cli", "bin", "cf.exe"][..]];
    for name in ["9.9.8-00000001", "9.9.8-00000002", "9.9.8-00000003"] {
        old_runtime(&root, name, &both);
    }
    old_runtime(&root, ".unpacking-stopped", &both);
    // The first is held by its node.exe alone, the second by its cf.exe alone.
    let held = [
        root.join("9.9.8-00000001").join("node.exe"),
        root.join("9.9.8-00000002")
            .join("cli")
            .join("bin")
            .join("cf.exe"),
    ];
    for program in &held {
        let mut permissions = fs::metadata(program).expect("a program").permissions();
        permissions.set_readonly(true);
        fs::set_permissions(program, permissions).expect("read-only program");
    }

    let exe = packed(dir.path(), &payload(b"the native cf"));
    let folder = unpacked_runtime(&exe, &local, "9.9.9")
        .expect("unpacked")
        .expect("a runtime");
    let kept = entries(&root);
    for program in &held {
        let mut permissions = fs::metadata(program).expect("a program").permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(program, permissions).expect("writable again");
    }

    assert_eq!(
        kept,
        [
            "9.9.8-00000001",
            "9.9.8-00000002",
            folder.file_name().unwrap().to_str().unwrap()
        ]
    );
    assert!(
        root.join("9.9.8-00000002").join("cli").join("bin").is_dir(),
        "left whole"
    );
    assert!(root.join("9.9.8-00000001").join("cli").is_dir());
}

/// A file that may only be appended to (macOS's `UF_APPEND`), which refuses an
/// open for writing and grants one for appending, as Windows does a program
/// that runs. It is let go when this is dropped: a file so held cannot be
/// removed, and a failed assertion must not leave one behind.
#[cfg(target_os = "macos")]
struct AppendOnly(PathBuf);

#[cfg(target_os = "macos")]
impl AppendOnly {
    const UF_APPEND: libc::c_uint = 0x0000_0004;

    fn set(path: &Path) -> Self {
        Self::flags(path, Self::UF_APPEND).expect("a file system that keeps user flags");
        Self(path.to_path_buf())
    }

    fn flags(path: &Path, flags: libc::c_uint) -> io::Result<()> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let path = CString::new(path.as_os_str().as_bytes()).expect("a path with no NUL");
        // SAFETY: `path` is a NUL-terminated string that outlives the call.
        if unsafe { libc::chflags(path.as_ptr(), flags) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for AppendOnly {
    fn drop(&mut self) {
        let _ = Self::flags(&self.0, 0);
    }
}

/// The check asks of a program that it refuses an open for writing, not that
/// it refuses every open: Windows grants an open for appending to a program
/// that runs, and `append` asks for no more. Here a file that may only be
/// appended to is that program: the check sees it run, and an open for
/// appending (the check's first form) would not.
#[cfg(target_os = "macos")]
#[test]
fn a_program_that_refuses_a_write_open_and_grants_an_append_open_is_seen_to_run() {
    let dir = tempfile::tempdir().expect("dir");
    let folder = old_runtime(dir.path(), "9.9.8-00000001", &[&["node.exe"]]);
    let node = folder.join("node.exe");
    assert!(!runs(&folder), "a plain file is no program that runs");

    let held = AppendOnly::set(&node);
    assert!(
        OpenOptions::new().append(true).open(&node).is_ok(),
        "it grants an open for appending"
    );
    assert!(
        OpenOptions::new().write(true).open(&node).is_err(),
        "and refuses one for writing"
    );
    assert!(runs(&folder), "so it is seen to run");

    drop(held);
    assert!(!runs(&folder), "and is not once it is let go");
}

/// A program that runs until it is ended: Windows' own command interpreter,
/// copied to `at` under the name of the program it stands in for, which waits
/// for a command on a standard input that nobody writes to.
#[cfg(windows)]
struct Running(std::process::Child);

#[cfg(windows)]
impl Running {
    fn start(at: &Path) -> Self {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let shell = std::env::var_os("ComSpec").expect("ComSpec names the command interpreter");
        fs::create_dir_all(at.parent().expect("a folder")).expect("its folder");
        fs::copy(shell, at).expect("copy the command interpreter");
        let mut child = Command::new(at)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .expect("start the copy");
        assert!(
            child.try_wait().expect("ask the copy").is_none(),
            "{} ended on its own",
            at.display()
        );
        Self(child)
    }

    fn end(mut self) {
        self.0.kill().expect("end the copy");
        self.0.wait().expect("wait for the copy");
    }
}

/// A failed assertion must not leave the program running, or its folder held.
#[cfg(windows)]
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The check on one real process, and what it rests on. A program that runs
/// refuses an open for writing and grants one for appending, and `append` is
/// all that std's `OpenOptions` asks for with it: a check made of that open
/// (the first form of this one) sees no program run, and the cleanup takes a
/// runtime from under its daemon. A read-only file refuses every open and
/// cannot tell the two apart, so a process that runs is used here. `runs`
/// holds while it does, and lets go once it has ended.
#[cfg(windows)]
#[test]
fn a_program_that_runs_refuses_a_write_open_and_grants_an_append_open() {
    let dir = tempfile::tempdir().expect("dir");
    let folder = dir.path().join("runtime");
    let node = folder.join("node.exe");
    let running = Running::start(&node);

    let writable = OpenOptions::new().write(true).open(&node).is_ok();
    let appendable = OpenOptions::new().append(true).open(&node).is_ok();
    let seen = runs(&folder);
    running.end();

    assert!(!writable, "a program that runs refuses an open for writing");
    assert!(
        appendable,
        "and grants an open for appending, which is why `runs` does not make it"
    );
    assert!(seen, "a runtime whose node.exe runs is seen to run");

    // Windows lets go of an ended program's file a moment after it ends.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while runs(&folder) && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(250));
    }
    assert!(!runs(&folder), "and is not seen to run once it has ended");
}

/// Proven with real processes, started from the runtime's own files: what
/// keeps a runtime is that its `node.exe` or its `cf.exe` runs, not what the
/// file is. A third runtime, the same files idle, goes at the same start, and
/// the other two go once their programs have ended.
#[cfg(windows)]
#[test]
fn a_runtime_whose_node_or_cf_runs_stays_whole_and_goes_once_it_has_ended() {
    let dir = tempfile::tempdir().expect("dir");
    let local = dir.path().join("local");
    let root = local.join(RUNTIME_PARENT);
    let both = [&["node.exe"][..], &["cli", "bin", "cf.exe"][..]];
    let (node_runs, cf_runs, idle) = (
        root.join("9.9.8-00000001"),
        root.join("9.9.8-00000002"),
        root.join("9.9.8-00000003"),
    );
    for folder in [&node_runs, &cf_runs, &idle] {
        old_runtime(&root, folder.file_name().unwrap().to_str().unwrap(), &both);
    }
    // The program of each is replaced by a real running process; the other
    // program of the runtime stays a plain file, which does not run.
    fs::remove_file(node_runs.join("node.exe")).expect("make room");
    fs::remove_file(cf_runs.join("cli").join("bin").join("cf.exe")).expect("make room");
    let running = [
        Running::start(&node_runs.join("node.exe")),
        Running::start(&cf_runs.join("cli").join("bin").join("cf.exe")),
    ];
    let exe = packed(dir.path(), &payload(b"the native cf"));

    let folder = unpacked_runtime(&exe, &local, "9.9.9")
        .expect("unpacked")
        .expect("a runtime");

    let name = folder.file_name().unwrap().to_str().unwrap();
    assert_eq!(
        entries(&root),
        ["9.9.8-00000001", "9.9.8-00000002", name],
        "the idle runtime goes, the two that run stay"
    );
    assert!(node_runs.join("cli").join("bin").join("cf.exe").is_file());
    assert!(cf_runs.join("node.exe").is_file());

    for program in running {
        program.end();
    }
    // Windows lets go of an ended program's file a moment after it ends.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        unpacked_runtime(&exe, &local, "9.9.9").expect("unpacked again");
        if entries(&root) == [name] || std::time::Instant::now() > deadline {
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    assert_eq!(entries(&root), [name], "both go once their programs ended");
}
