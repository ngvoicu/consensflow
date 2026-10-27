#[cfg(unix)]
use std::ffi::OsString;

use super::*;

/// What a command runs, and with what: its program, words and added variables.
fn described(command: &Command) -> (PathBuf, Vec<String>, Vec<(String, String)>) {
    (
        PathBuf::from(command.get_program()),
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        command
            .get_envs()
            .filter_map(|(name, value)| {
                Some((
                    name.to_string_lossy().into_owned(),
                    value?.to_string_lossy().into_owned(),
                ))
            })
            .collect(),
    )
}

#[test]
fn the_daemon_is_the_bundled_cf_running_ui_and_nothing_of_the_apps_is_added_to_it() {
    let cf = Path::new("/bundle/cli/bin/cf");
    assert_eq!(
        described(&command_for(cf)),
        (
            PathBuf::from(cf),
            vec!["ui".into(), "--json".into(), "--no-open".into()],
            vec![]
        )
    );
}

/// The daemon runs as long as the app does, and the app opens sockets and pipes on its other
/// threads as it starts it. On macOS std makes each close-on-exec in a second step, and a
/// program started between the two holds it for good: the daemon would hold a connection of the
/// app's, and what the app closed would not be closed. Here a descriptor is left in that moment
/// while the daemon starts, and the daemon is asked whether it has it.
#[cfg(target_os = "macos")]
#[test]
fn the_daemon_holds_nothing_of_the_apps_but_its_streams_whatever_was_open_as_it_started() {
    use std::os::fd::AsRawFd;

    let folder = tempfile::tempdir().expect("a folder");
    let stray = std::fs::File::open("/dev/null").expect("a file");
    let fd = stray.as_raw_fd();
    // SAFETY: `fcntl` is asked to clear the flags of a descriptor this test holds.
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
    let cf = login_shell(
        folder.path(),
        &format!("if [ -e /dev/fd/{fd} ]; then echo held; else echo clear; fi"),
    );
    // The script was written a moment ago, and a program started on another thread meanwhile
    // can hold it open for writing: such a start is refused, and tried again.
    let said = (0..50)
        .find_map(|_| match command_for(&cf).output() {
            Ok(output) => Some(output),
            Err(refused) if refused.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(100));
                None
            }
            Err(refused) => panic!("the daemon was not started: {refused}"),
        })
        .expect("the daemon started");
    assert_eq!(String::from_utf8_lossy(&said.stdout), "clear\n");
}

#[test]
fn the_cf_is_the_one_in_the_clis_folder_of_the_bundle_or_the_portable_runtime() {
    let cf = if cfg!(windows) { "cf.exe" } else { "cf" };
    assert_eq!(
        cf_in(Path::new("/bundle")),
        Path::new("/bundle").join("cli").join("bin").join(cf)
    );
}

#[test]
fn the_log_says_which_cf_the_daemon_is() {
    assert_eq!(
        starting(Path::new("/bundle/cli/bin/cf")),
        "starting the daemon: /bundle/cli/bin/cf ui --json --no-open"
    );
}

#[test]
fn a_cf_that_is_there_is_found_and_one_that_is_not_says_the_app_is_missing_it() {
    let bundle = tempfile::tempdir().expect("a bundle");
    let cf = cf_in(bundle.path());
    let error = present(cf.clone()).expect_err("no cf is there yet");
    assert!(
        error.starts_with("the bundled ConsensFlow is missing from this app ("),
        "{error}"
    );
    std::fs::create_dir_all(cf.parent().expect("a folder")).expect("its folder");
    std::fs::write(&cf, "the program").expect("a cf");
    assert_eq!(present(cf.clone()), Ok(cf));
    // A relative name is no place the app was installed in.
    assert!(present(PathBuf::from("cf")).is_err());
}

/// A stand-in for the human's login shell: `body` runs with the command
/// the app gives it (`-lc <command>`) as `$2`.
#[cfg(unix)]
fn login_shell(home: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let shell = home.join("login-shell");
    std::fs::write(&shell, format!("#!/bin/sh\n{body}\n")).expect("write the shell");
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755))
        .expect("make the shell executable");
    shell
}

/// What a login file prints (`nvm use` says which Node it took) is not
/// part of the PATH.
#[cfg(unix)]
#[test]
fn the_login_path_is_read_past_what_login_files_print() {
    let home = tempfile::tempdir().expect("home");
    let shell = login_shell(
        home.path(),
        "echo 'Now using node v22.9.0 (npm v10.8.3)'; exec /bin/sh -c \"$2\"",
    );
    assert_eq!(
        login_path_in(&shell, LOGIN_PATH_TIMEOUT, &LoginLookup::default()),
        std::env::var("PATH").ok()
    );
}

/// The login shell is asked as the app starts, while its other threads open sockets and
/// pipes, and what a login file starts may run on after it: here a descriptor is left open and
/// not close-on-exec while the shell is asked, and the shell says, as its PATH, whether it has it.
#[cfg(target_os = "macos")]
#[test]
fn the_login_shell_holds_nothing_of_the_apps_but_its_streams() {
    use std::os::fd::AsRawFd;

    let home = tempfile::tempdir().expect("home");
    let stray = std::fs::File::open("/dev/null").expect("a file");
    let fd = stray.as_raw_fd();
    // SAFETY: `fcntl` is asked to clear the flags of a descriptor this test holds.
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
    let shell = login_shell(
        home.path(),
        &format!(
            "if [ -e /dev/fd/{fd} ]; then PATH=held; else PATH=clear; fi; export PATH; exec /bin/sh -c \"$2\""
        ),
    );
    assert_eq!(
        login_path_in(&shell, LOGIN_PATH_TIMEOUT, &LoginLookup::default()).as_deref(),
        Some("clear")
    );
}

/// The pid a shell wrote in `file`, once it has. How soon a shell runs is the machine's
/// (a loaded one took seconds to start a script), not what is tested: the bound is for a
/// shell that never does.
#[cfg(unix)]
fn pid_in(file: &Path) -> i32 {
    let started = Instant::now();
    loop {
        if let Some(pid) = std::fs::read_to_string(file)
            .ok()
            .and_then(|said| said.trim().parse().ok())
        {
            return pid;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the shell never ran"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// Ends the process `pid`, which a test left running.
#[cfg(unix)]
fn kill(pid: i32) {
    let _ = crate::testing::command("/bin/kill")
        .args(["-9", &pid.to_string()])
        .status();
}

/// A login shell that fails, or takes too long, leaves the daemon on the
/// PATH the app was started with; the app's start does not wait on it. Only
/// the shell is ended: what its login files started is the user's, an
/// `ssh-agent -D` that other programs use, say (the reviews of 2026-10-16).
///
/// The shell that hangs is not given a time to run in: it is a slow start of a loaded
/// machine that kills it, at the time it is given, before it has started anything
/// (T-581: "the shell never ran" in the gate at a load of 50 to 150, the shell killed
/// at 300 ms before its first command). What it started is looked for when it says it
/// did, and the shell is ended then, as the app's quit ends it.
#[cfg(unix)]
#[test]
fn a_login_shell_that_fails_or_hangs_gives_no_path_and_leaves_what_it_started() {
    let home = tempfile::tempdir().expect("home");
    let lookup = LoginLookup::default();
    let failing = login_shell(home.path(), "/bin/sh -c \"$2\"; exit 3");
    assert_eq!(
        login_path_in(&failing, Duration::from_secs(5), &lookup),
        None
    );

    // A shell that hangs is not waited for beyond its time, though a program it started
    // holds its output as long as it lives (30 s here): the time is up long before.
    let held = home.path().join("held.pid");
    let holding = login_shell(
        home.path(),
        &format!("/bin/sleep 30 & echo $! > '{}'; wait", held.display()),
    );
    let started = Instant::now();
    assert_eq!(
        login_path_in(
            &holding,
            Duration::from_millis(300),
            &LoginLookup::default()
        ),
        None
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the shell held up the start"
    );
    if let Some(held) = std::fs::read_to_string(&held)
        .ok()
        .and_then(|said| said.trim().parse().ok())
    {
        kill(held);
    }

    // What the shell started is left running when the shell is ended. It starts slowly, as a
    // loaded machine's does; the time it is given is long, and it is ended when it has
    // started what it started, as the app's quit ends it.
    let waited_for = home.path().join("waited-for.pid");
    let hanging = login_shell(
        home.path(),
        &format!(
            "/bin/sleep 1; /bin/sleep 30 & echo $! > '{}'; wait",
            waited_for.display()
        ),
    );
    let asking = std::sync::Arc::new(LoginLookup::default());
    let asked = {
        let (asking, hanging) = (std::sync::Arc::clone(&asking), hanging.clone());
        thread::spawn(move || login_path_in(&hanging, Duration::from_secs(120), &asking))
    };
    let agent = pid_in(&waited_for);
    asking.end();
    assert_eq!(asked.join().expect("the lookup ended"), None);
    // Given a moment to be ended, as it would have been with the shell.
    thread::sleep(Duration::from_millis(200));
    let left = cf_panes::pty::process_exists(agent);
    kill(agent);
    assert!(left, "what the login file started was ended with the shell");
}

/// On Windows no login shell is asked, though `SHELL` names one (Git Bash's):
/// an app started from the Start menu has the PATH of the user and the
/// machine, which a terminal there has too, and a POSIX shell's PATH
/// (`/c/…`, joined by `:`) is none a Windows program can use. Windows is the
/// environment's word for it here, as detection's tests have it.
#[cfg(unix)]
#[test]
fn on_windows_no_login_shell_is_asked_and_the_daemon_keeps_the_apps_path() {
    let home = tempfile::tempdir().expect("home");
    let asked = home.path().join("asked");
    let shell = login_shell(
        home.path(),
        &format!("echo > '{}'; exec /bin/sh -c \"$2\"", asked.display()),
    );
    let windows = Env::from_vars([
        (OsString::from("OS"), OsString::from("Windows_NT")),
        (OsString::from("SHELL"), shell.into_os_string()),
        (
            OsString::from("PATH"),
            OsString::from(r"C:\Windows\System32"),
        ),
    ]);
    let (command, said) = the_daemons(&windows, Duration::from_secs(5));
    assert_eq!(
        given(&command),
        [("PATH".to_owned(), r"C:\Windows\System32".to_owned())]
    );
    assert_eq!(said, None);
    assert!(!asked.exists(), "the shell was asked");
}

/// A start after the app began to quit starts no login shell at all: the check
/// of the quit, the start and the holding of the shell are one step, so none
/// is started that the quit has missed.
#[cfg(unix)]
#[test]
fn a_lookup_the_app_has_quit_starts_no_shell() {
    let home = tempfile::tempdir().expect("home");
    let started = home.path().join("started");
    let shell = login_shell(
        home.path(),
        &format!("echo > '{}'; exec /bin/sh -c \"$2\"", started.display()),
    );
    let lookup = LoginLookup::default();
    assert!(login_path_in(&shell, LOGIN_PATH_TIMEOUT, &lookup).is_some());
    std::fs::remove_file(&started).expect("the shell ran once");
    lookup.end();
    assert_eq!(login_path_in(&shell, Duration::from_secs(5), &lookup), None);
    let mut asked = crate::testing::command(&shell);
    asked.stdout(Stdio::piped());
    assert!(lookup.start(&mut asked).is_none(), "a shell was started");
    assert!(lookup.lock().shells.is_empty(), "a shell is held");
    thread::sleep(Duration::from_millis(300));
    assert!(!started.exists(), "a shell ran after the quit");
}

/// The app's quit ends the shell a start asks itself, at once: the app may be
/// gone before the start's thread looks again. Only the shell: what it started
/// is the user's.
#[cfg(unix)]
#[test]
fn ending_a_lookup_ends_the_shell_it_asks_at_once_and_only_it() {
    let home = tempfile::tempdir().expect("home");
    let sleeper = home.path().join("sleeper.pid");
    let mut shell = crate::testing::command("/bin/sh");
    shell
        .args([
            "-c",
            &format!("/bin/sleep 30 & echo $! > '{}'; wait", sleeper.display()),
        ])
        .stdout(Stdio::piped());
    let lookup = LoginLookup::default();
    let started = lookup.start(&mut shell).expect("the shell is started");
    let agent = pid_in(&sleeper);
    lookup.end();
    let until = Instant::now() + Duration::from_secs(3);
    let mut ended = false;
    while Instant::now() < until {
        if lookup
            .status(started.asked)
            .is_ok_and(|status| status.is_some())
        {
            ended = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    lookup.finish(started.asked);
    let left = cf_panes::pty::process_exists(agent);
    kill(agent);
    assert!(ended, "the shell outlived the quit");
    assert!(left, "what the shell started was ended with it");
}

/// A lookup given up on has ended its shell before it lets the lock go: a quit
/// that comes then finds no shell to end, and the app may be gone before the
/// lookup's thread goes on (the third review of 2026-10-16). The shell counts
/// on in a file while it runs.
#[cfg(unix)]
#[test]
fn a_lookup_given_up_on_has_ended_its_shell_before_a_quit_can_miss_it() {
    let home = tempfile::tempdir().expect("home");
    let beat = home.path().join("beat");
    let mut shell = crate::testing::command("/bin/sh");
    shell
        .args([
            "-c",
            &format!(
                "i=0; while kill -0 $PPID 2>/dev/null; do i=$((i+1)); echo $i > '{}'; /bin/sleep 0.05; done",
                beat.display()
            ),
        ])
        .stdout(Stdio::piped());
    let lookup = LoginLookup::default();
    let asked = lookup
        .start(&mut shell)
        .expect("the shell is started")
        .asked;
    let started = Instant::now();
    while !beat.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the shell never ran"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let mut stopped = false;
    lookup.finish_with(asked, || {
        // The quit comes here, and the app could end with it.
        lookup.end();
        let before = std::fs::read_to_string(&beat).unwrap_or_default();
        thread::sleep(Duration::from_millis(400));
        stopped = std::fs::read_to_string(&beat).unwrap_or_default() == before;
    });
    assert!(
        stopped,
        "the shell ran on past the quit, with nothing left to end it"
    );
}

/// The PATH the Finder gives an app.
#[cfg(unix)]
const FINDER_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

/// The environment of an app the Finder started, whose login shell is
/// `shell` and whose system's folders of programs are `folders`.
#[cfg(unix)]
fn finder(shell: Option<&Path>, folders: &[PathBuf]) -> Env {
    let folders = std::env::join_paths(folders).expect("folders a PATH can name");
    let mut vars = vec![
        (OsString::from("PATH"), OsString::from(FINDER_PATH)),
        (OsString::from(cf_base::env::TOOL_FOLDERS), folders),
    ];
    vars.extend(shell.map(|shell| (OsString::from("SHELL"), shell.into())));
    Env::from_vars(vars)
}

/// The daemon's command, as the app makes it in `app`'s environment, and what
/// the app's log says of it.
#[cfg(unix)]
fn the_daemons(app: &Env, timeout: Duration) -> (Command, Option<String>) {
    let mut command = command_for(Path::new("/bundle/cli/bin/cf"));
    let said = on_login_environment(&mut command, app, timeout, &LoginLookup::default());
    (command, said)
}

/// The variables the daemon's `command` is given.
#[cfg(unix)]
fn given(command: &Command) -> Vec<(String, String)> {
    described(command).2
}

/// The value the daemon's `command` is given of the variable `name`.
#[cfg(unix)]
fn given_of(command: &Command, name: &str) -> Option<String> {
    given(command)
        .into_iter()
        .find(|(held, _)| held == name)
        .map(|(_, value)| value)
}

/// A loaded machine's login shell, slower than the five seconds it was
/// given (2026-10-15), is waited for: the daemon is given its PATH as it is
/// (the daemon puts the system's folders after it), and the log has nothing
/// to say.
#[cfg(unix)]
#[test]
fn a_login_shell_slower_than_five_seconds_still_gives_its_path() {
    let home = tempfile::tempdir().expect("home");
    let shell = login_shell(home.path(), "/bin/sleep 6; exec /bin/sh -c \"$2\"");
    let homebrew = home.path().join("homebrew-bin");
    std::fs::create_dir(&homebrew).expect("the system's folder");
    let login = std::env::var("PATH").expect("the PATH the shell has");
    let started = Instant::now();
    let (command, said) = the_daemons(
        &finder(Some(shell.as_path()), std::slice::from_ref(&homebrew)),
        LOGIN_PATH_TIMEOUT,
    );
    assert!(started.elapsed() >= Duration::from_secs(6), "it answered");
    assert_eq!(given_of(&command, "PATH"), Some(login));
    assert_eq!(said, None);
}

/// A login shell that fails, or never answers, leaves the daemon on the app's
/// PATH as it is, and the app's log says so, and what the daemon runs on: that
/// PATH with the system's folders that are there and not on it after it. With
/// no login shell to ask, the same PATH, unsaid.
#[cfg(unix)]
#[test]
fn a_login_shell_that_fails_or_hangs_leaves_the_apps_path_and_the_log_says_what_the_daemon_runs_on()
{
    let home = tempfile::tempdir().expect("home");
    let homebrew = home.path().join("homebrew-bin");
    std::fs::create_dir(&homebrew).expect("the system's folder");
    // One that is not there, and one the Finder's PATH has already.
    let folders = [
        homebrew.clone(),
        home.path().join("homebrew-sbin"),
        PathBuf::from("/bin"),
    ];
    let runs_on = format!("{FINDER_PATH}:{}", homebrew.display());
    let failing = login_shell(home.path(), "/bin/sh -c \"$2\"; exit 3");
    let other = tempfile::tempdir().expect("another home");
    let hanging = login_shell(other.path(), "exec /bin/sleep 5");
    let finders = [("PATH".to_owned(), FINDER_PATH.to_owned())];
    for (shell, timeout) in [
        (&failing, Duration::from_secs(5)),
        (&hanging, Duration::from_secs(1)),
    ] {
        let (command, said) = the_daemons(&finder(Some(shell.as_path()), &folders), timeout);
        assert_eq!(given(&command), finders, "{}", shell.display());
        assert_eq!(
            said,
            Some(format!(
                "the login shell {} gave no PATH within {} s, and the daemon runs on {runs_on}",
                shell.display(),
                timeout.as_secs()
            ))
        );
    }
    let (command, said) = the_daemons(&finder(None, &folders), Duration::from_secs(1));
    assert_eq!((given(&command), said), (finders.to_vec(), None));
}

#[test]
fn the_log_says_how_long_the_login_shell_had_and_the_path_the_daemon_runs_on() {
    assert_eq!(
        unanswered(
            Path::new("/bin/zsh"),
            Duration::from_secs(30),
            OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin")
        ),
        "the login shell /bin/zsh gave no PATH within 30 s, and the daemon runs on /usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin"
    );
}

/// Something a login file starts may keep the shell's output open after
/// the shell has gone; what the shell printed is read all the same.
#[cfg(unix)]
#[test]
fn a_login_path_is_read_while_a_started_process_holds_the_output() {
    let home = tempfile::tempdir().expect("home");
    // The process outlives the read by far, so a loaded machine's slow start is
    // not taken for a wait on it.
    let shell = login_shell(home.path(), "/bin/sleep 20 & exec /bin/sh -c \"$2\"");
    let started = Instant::now();
    assert_eq!(
        login_path_in(&shell, Duration::from_secs(15), &LoginLookup::default()),
        std::env::var("PATH").ok()
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the read waited for the process"
    );
}

/// A file at `path` this user can start.
#[cfg(unix)]
fn startable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::create_dir_all(path.parent().expect("a folder")).expect("its folder");
    std::fs::write(path, "#!/bin/sh\n").expect("the program");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("startable");
}

/// The daemon the app starts on a Finder's PATH, its login shell failing, and
/// detection in it: the human's own Codex in the home is the one a window
/// opens on, as it was before the system's folders were anywhere, and not an
/// older one of Homebrew's (the reviews of 2026-10-16); and what the daemon
/// runs has Homebrew's folder after its PATH.
#[cfg(unix)]
#[test]
fn the_daemon_the_app_starts_finds_a_harness_in_the_home_before_the_systems_folders() {
    let machine = tempfile::tempdir().expect("a machine");
    let home = machine.path().join("home");
    let homebrew = machine.path().join("homebrew-bin");
    let own = home.join(".codex").join("bin").join("codex");
    startable(&own);
    startable(&homebrew.join("codex"));
    let failing = login_shell(machine.path(), "exit 3");
    let app = Env::from_vars(
        finder(Some(failing.as_path()), std::slice::from_ref(&homebrew))
            .iter()
            .map(|(name, value)| (name.to_os_string(), value.to_os_string()))
            .chain([(OsString::from("HOME"), home.into_os_string())]),
    );
    let (command, _) = the_daemons(&app, Duration::from_secs(5));
    // The daemon's environment: the app's, which it inherits, with what the
    // command sets over it, as its start makes it.
    let set: Vec<(OsString, OsString)> = command
        .get_envs()
        .filter_map(|(name, value)| Some((name.to_os_string(), value?.to_os_string())))
        .collect();
    let daemon = Env::from_vars(
        app.iter()
            .filter(|(name, _)| !set.iter().any(|(set, _)| set == name))
            .map(|(name, value)| (name.to_os_string(), value.to_os_string()))
            .chain(set.iter().cloned()),
    )
    .with_tool_folders_on_path();
    assert_eq!(
        cf_harness::detect::harness_path(cf_proto::agents::Harness::Codex, &daemon),
        Some(own)
    );
    assert_eq!(
        daemon.os("PATH"),
        Some(OsString::from(format!("{FINDER_PATH}:{}", homebrew.display())).as_os_str())
    );
}

/// A login shell that ends without its PATH while something it started goes on
/// writing to its output is given up on by the deadline: the read waited a
/// second for each piece, and so for ever. What it started is the user's (the
/// test ends it itself).
#[cfg(unix)]
#[test]
fn a_shell_that_ends_without_its_path_while_what_it_started_writes_on_is_given_up_by_the_deadline()
{
    let home = tempfile::tempdir().expect("home");
    let writer = home.path().join("writer.pid");
    let shell = login_shell(
        home.path(),
        &format!(
            "/bin/sh -c 'echo $$ > {}; while {}; do echo noise; /bin/sleep 0.2; done' & exit 0",
            writer.display(),
            // Its parent is the shell that exits at once: its owner is the test.
            cf_base::fixture::owner_there(std::process::id())
        ),
    );
    let (sent, ended) = mpsc::channel();
    thread::spawn(move || {
        let _ = sent.send(login_path_in(
            &shell,
            Duration::from_secs(2),
            &LoginLookup::default(),
        ));
    });
    let given = ended.recv_timeout(Duration::from_secs(15));
    kill(pid_in(&writer));
    assert_eq!(given.expect("the read was given up by its deadline"), None);
}

#[test]
fn a_verbatim_windows_path_is_spelled_plainly() {
    assert_eq!(
        plain_path(PathBuf::from(r"\\?\C:\Users\me\app\cli\bin\cf.exe")),
        PathBuf::from(r"C:\Users\me\app\cli\bin\cf.exe")
    );
    assert_eq!(
        plain_path(PathBuf::from(r"\\?\UNC\server\share\cf.exe")),
        PathBuf::from(r"\\server\share\cf.exe")
    );
    assert_eq!(
        plain_path(PathBuf::from("/Applications/ConsensFlow.app/cf")),
        PathBuf::from("/Applications/ConsensFlow.app/cf")
    );
}

/// A stand-in for a shell whose interactive rc file runs `rc` before the command it
/// is given, as zsh runs `.zshrc` for `-i`; started without `-i` it runs the command
/// alone, as a login shell's `-lc` does. Its rc exports [`RC_READ`] first.
#[cfg(unix)]
fn shell_with_rc(home: &Path, rc: &str) -> PathBuf {
    login_shell(
        home,
        &format!("case \"$1\" in *i*) export {RC_READ}=1; {rc} ;; esac\nexec /bin/sh -c \"$2\""),
    )
}

/// What every [`shell_with_rc`] exports: the daemon is given it when the interactive
/// shell's environment was read.
#[cfg(unix)]
const RC_READ: &str = "PARITY_RC_READ";

/// The daemon's command, as the app makes it in `app`'s environment, whose login shell is
/// a [`shell_with_rc`] that answers, and what the app's log says of it. The shell has the
/// app's own limit, which one that answers never waits out: how soon a loaded machine
/// starts a script is not what is tested. And the environment was read, or this fails
/// saying so: a lookup that read nothing gives the daemon none of the rc, and what a test
/// says is never taken would hold of nothing (CI, 2026-10-27: a shell the rc's loader
/// variable ended at its start gave nothing, and only the test's last assertion said so).
#[cfg(unix)]
fn the_daemons_on_the_rc(app: &Env) -> (Command, Option<String>) {
    let (command, said) = the_daemons(app, LOGIN_PATH_TIMEOUT);
    assert_eq!(
        given_of(&command, RC_READ).as_deref(),
        Some("1"),
        "the interactive shell's environment was not read"
    );
    (command, said)
}

/// What a terminal's shell gives a harness, a member window has (T-540): a
/// variable only the interactive rc exports reaches the daemon, a value of any
/// bytes as it is, and the app's log says nothing of it.
#[cfg(unix)]
#[test]
fn a_variable_only_the_interactive_shell_sets_reaches_the_daemon_as_it_is_and_is_not_logged() {
    let home = tempfile::tempdir().expect("home");
    let shell = shell_with_rc(
        home.path(),
        "export PARITY_ONLY_INTERACTIVE='a value = with = signs\nand a second line'; \
         export LANG=en_GB.UTF-8; export PATH=\"/rc/bin:$PATH\"",
    );
    let (command, said) = the_daemons_on_the_rc(&finder(Some(shell.as_path()), &[]));
    assert_eq!(
        given_of(&command, "PARITY_ONLY_INTERACTIVE").as_deref(),
        Some("a value = with = signs\nand a second line")
    );
    assert_eq!(given_of(&command, "LANG").as_deref(), Some("en_GB.UTF-8"));
    let path = given_of(&command, "PATH").expect("a PATH");
    assert!(path.starts_with("/rc/bin:"), "{path}");
    assert_eq!(said, None, "the log says nothing of a shell that answered");
}

/// What says where the shell was, how deep, its last command and its terminal is no
/// pane's, and is not taken.
#[cfg(unix)]
#[test]
fn the_shells_place_depth_last_command_and_terminal_are_not_taken() {
    let home = tempfile::tempdir().expect("home");
    let shell = shell_with_rc(
        home.path(),
        "cd /; export OLDPWD=/tmp SHLVL=7 TERM=dumb TERM_PROGRAM=iTerm.app TERMINFO=/x \
         PARITY_KEPT=yes",
    );
    let (command, _) = the_daemons_on_the_rc(&finder(Some(shell.as_path()), &[]));
    let names: Vec<String> = given(&command).into_iter().map(|(name, _)| name).collect();
    for dropped in [
        "PWD",
        "OLDPWD",
        "SHLVL",
        "_",
        "TERM",
        "TERM_PROGRAM",
        "TERMINFO",
    ] {
        assert!(
            !names.iter().any(|name| name == dropped),
            "{dropped} was taken"
        );
    }
    assert_eq!(given_of(&command, "PARITY_KEPT").as_deref(), Some("yes"));
}

/// An interactive rc that hangs (one that `exec`s a terminal multiplexer, say) costs
/// the start no more than its one limit, both questions together, and the daemon still
/// gets the login shell's PATH: it is never worse off than before the environment was
/// asked.
#[cfg(unix)]
#[test]
fn an_interactive_rc_that_hangs_falls_back_to_the_login_path_within_its_time() {
    let home = tempfile::tempdir().expect("home");
    let shell = shell_with_rc(home.path(), "exec /bin/sleep 30");
    let started = Instant::now();
    let (command, said) = the_daemons(&finder(Some(shell.as_path()), &[]), Duration::from_secs(2));
    assert!(
        started.elapsed() < Duration::from_millis(3_500),
        "the start waited {:?}, past its one limit",
        started.elapsed()
    );
    assert_eq!(given_of(&command, "PATH"), std::env::var("PATH").ok());
    assert_eq!(given_of(&command, "PARITY_ONLY_INTERACTIVE"), None);
    assert_eq!(said, None);
}

/// An entry of `env -0` is a variable by its first `=`; one with no name is none.
#[test]
fn an_entry_is_named_by_its_first_equals_sign_and_one_without_a_name_is_none() {
    assert_eq!(
        variable(b"A=b=c"),
        Some((OsString::from("A"), OsString::from("b=c")))
    );
    assert_eq!(
        variable(b"EMPTY="),
        Some((OsString::from("EMPTY"), OsString::new()))
    );
    assert_eq!(variable(b"=x"), None);
    assert_eq!(variable(b"no-equals"), None);
}

/// The PowerShell a Windows terminal opens: PowerShell 7 where a folder of the PATH has
/// it, else Windows PowerShell, under the system's root.
#[test]
fn the_powershell_asked_is_pwsh_where_the_path_has_it_else_windows_powershell() {
    let folder = tempfile::tempdir().expect("a folder");
    let other = tempfile::tempdir().expect("another");
    let env = |path: &[&Path]| {
        Env::from_vars([
            (
                std::ffi::OsString::from("PATH"),
                std::env::join_paths(path).expect("a PATH"),
            ),
            (
                std::ffi::OsString::from("SystemRoot"),
                std::ffi::OsString::from("/Windows-root"),
            ),
        ])
    };
    assert_eq!(
        powershell_of(&env(&[other.path(), folder.path()])),
        Path::new("/Windows-root").join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
    );
    let pwsh = folder.path().join("pwsh.exe");
    std::fs::write(&pwsh, "").expect("a pwsh");
    assert_eq!(powershell_of(&env(&[other.path(), folder.path()])), pwsh);
}

/// Windows names its PATH `Path`: an environment that has it is taken whole.
#[test]
fn a_printed_environment_is_taken_by_its_path_whatever_its_case() {
    let printed = b"Path=C:\\Windows\0SystemRoot=C:\\Windows\0PWD=/x\0";
    let vars = taken(printed).expect("taken");
    let names: Vec<String> = vars
        .iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["Path", "SystemRoot"]);
    assert_eq!(taken(b"SystemRoot=C:\\Windows\0"), None, "no PATH: none");
}

/// Of what a shell printed, ConsensFlow's own variables, the loader's and a Claude session's
/// identity are not taken; a harness's home is.
#[test]
fn of_a_printed_environment_consensflows_own_the_loaders_and_a_claude_sessions_are_not_taken() {
    let printed = b"PATH=/rc/bin\0CONSENSFLOW_HOME=/rc/home\0CF_ANYTHING=1\0\
        DYLD_INSERT_LIBRARIES=/x.dylib\0DYLD_LIBRARY_PATH=/x\0LD_PRELOAD=/x.so\0CLAUDECODE=1\0\
        CLAUDE_CODE_SESSION_ID=s1\0CLAUDE_CONFIG_DIR=/rc/claude\0";
    let names: Vec<String> = taken(printed)
        .expect("taken")
        .iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["PATH", "CLAUDE_CONFIG_DIR"]);
}

/// The real Windows PowerShell answers what this process has, as a terminal's gives it:
/// its Path and SystemRoot come back as they are, within the time.
#[cfg(windows)]
#[test]
fn windows_powershell_says_its_environment_in_utf8_between_the_markers() {
    let env = Env::from_process();
    let started = Instant::now();
    let vars = profile_environment_in(
        &powershell_of(&env),
        Duration::from_secs(60),
        &LoginLookup::default(),
    )
    .expect("PowerShell gave its environment");
    assert!(started.elapsed() < Duration::from_secs(60));
    let value = |name: &str| {
        vars.iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    };
    assert_eq!(value("SystemRoot"), std::env::var_os("SystemRoot"));
    assert!(value("PATH").is_some_and(|path| !path.is_empty()));
}

/// What the app sets for itself wins over the rc, and the rc over what the app was started
/// with: ConsensFlow's own variables and the loader's are never taken, nor a Claude
/// session's identity, while a harness's home is, as in a terminal. The dynamic loader's
/// of macOS (`DYLD_*`) are held of what a shell printed, below: a shell can show them on
/// no Mac. With SIP, dyld takes them out of what its own programs are given (`/bin/sh`,
/// `/usr/bin/env`), and this test held nothing of them; without it, a shell an rc gave a
/// library that is not there is ended as it starts (`/x.dylib`, on GitHub's runner, taken
/// to be such a Mac: CI, 2026-10-27).
#[cfg(unix)]
#[test]
fn consensflows_own_the_loaders_and_a_claude_sessions_variables_are_never_taken_and_a_harness_home_is(
) {
    let home = tempfile::tempdir().expect("home");
    let shell = shell_with_rc(
        home.path(),
        "export CONSENSFLOW_HOME=/rc/home CF_ANYTHING=1 LD_PRELOAD=/x.so CLAUDECODE=1 \
         CLAUDE_CODE_SESSION_ID=s1 CLAUDE_CONFIG_DIR=/rc/claude CODEX_HOME=/rc/codex",
    );
    let (command, _) = the_daemons_on_the_rc(&finder(Some(shell.as_path()), &[]));
    for never in [
        "CONSENSFLOW_HOME",
        "CF_ANYTHING",
        "LD_PRELOAD",
        "CLAUDECODE",
        "CLAUDE_CODE_SESSION_ID",
    ] {
        assert_eq!(given_of(&command, never), None, "{never} was taken");
    }
    assert_eq!(
        given_of(&command, "CLAUDE_CONFIG_DIR").as_deref(),
        Some("/rc/claude")
    );
    assert_eq!(
        given_of(&command, "CODEX_HOME").as_deref(),
        Some("/rc/codex")
    );
}

/// A run that starts the app with a harness home of its own keeps it, whatever the rc
/// says: the daemon is not given the rc's, nor told its name, so it and every window keep
/// the app's. A home the app has none of is the rc's.
#[cfg(unix)]
#[test]
fn a_harness_home_the_app_was_started_with_is_kept_over_the_rcs() {
    let home = tempfile::tempdir().expect("home");
    let shell = shell_with_rc(
        home.path(),
        "export CLAUDE_CONFIG_DIR=/rc/claude CODEX_HOME=/rc/codex",
    );
    let mut vars: Vec<(OsString, OsString)> = finder(Some(shell.as_path()), &[])
        .iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    vars.push(("CLAUDE_CONFIG_DIR".into(), "/test/claude".into()));
    let (command, _) = the_daemons_on_the_rc(&Env::from_vars(vars));
    assert_eq!(
        given_of(&command, "CLAUDE_CONFIG_DIR"),
        None,
        "the app's is kept"
    );
    assert_eq!(
        given_of(&command, "CODEX_HOME").as_deref(),
        Some("/rc/codex"),
        "a home the app has none of is the rc's"
    );
    let names = given_of(&command, TERMINAL_ENV).unwrap_or_default();
    assert!(!names.contains("CLAUDE_CONFIG_DIR"), "{names}");
    assert!(names.contains("CODEX_HOME"), "{names}");
}

/// The daemon is told which names it was given of the terminal's environment, by name
/// only and without the PATH, which it gives every pane itself: so that each window's
/// pane gets them too.
#[cfg(unix)]
#[test]
fn the_daemon_is_told_the_names_it_was_given_and_never_their_values() {
    let home = tempfile::tempdir().expect("home");
    let shell = shell_with_rc(
        home.path(),
        "export PARITY_SECRET=do-not-print LANG=en_GB.UTF-8 PATH=\"/rc/bin:$PATH\"",
    );
    let (command, _) = the_daemons_on_the_rc(&finder(Some(shell.as_path()), &[]));
    let said = given_of(&command, TERMINAL_ENV).expect("the names");
    let names: Vec<String> = serde_json::from_str(&said).expect("a list of names");
    assert!(
        names.iter().any(|name| name == "PARITY_SECRET"),
        "{names:?}"
    );
    assert!(names.iter().any(|name| name == "LANG"), "{names:?}");
    assert!(
        !names.iter().any(|name| name.eq_ignore_ascii_case("PATH")),
        "{names:?}"
    );
    assert!(!said.contains("do-not-print"), "a value crossed: {said}");
    // A shell that gave nothing names nothing.
    let failing = login_shell(home.path(), "/bin/sh -c \"$2\"; exit 3");
    let (command, _) = the_daemons(
        &finder(Some(failing.as_path()), &[]),
        Duration::from_secs(5),
    );
    assert_eq!(given_of(&command, TERMINAL_ENV), None);
}

/// On Windows the PowerShell asked has no console window of its own: started from a
/// windowed app, one would flash up at every start for as long as the profile takes.
#[cfg(windows)]
#[test]
fn the_powershell_asked_has_no_console_window() {
    let marker = marker("window");
    let script = format!(
        "Add-Type -Namespace Cf -Name Console -MemberDefinition '[DllImport(\"kernel32.dll\")] public static extern System.IntPtr GetConsoleWindow();'; \
         [Console]::Out.Write('{marker}' + [Cf.Console]::GetConsoleWindow() + '{marker}')"
    );
    let env = Env::from_process();
    let said = asked(
        &powershell_of(&env),
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ],
        &marker,
        Duration::from_secs(60),
        &LoginLookup::default(),
    )
    .expect("PowerShell answered");
    assert_eq!(
        String::from_utf8_lossy(&said),
        "0",
        "it had a console window"
    );
}

/// A shell whose login files hang too is given one limit in all, not one for each
/// question: the start waits no longer than `LOGIN_PATH_TIMEOUT` says.
#[cfg(unix)]
#[test]
fn a_shell_that_hangs_on_every_question_costs_the_start_one_limit() {
    let home = tempfile::tempdir().expect("home");
    let shell = login_shell(home.path(), "exec /bin/sleep 30");
    let started = Instant::now();
    let (command, said) = the_daemons(&finder(Some(shell.as_path()), &[]), Duration::from_secs(2));
    assert!(
        started.elapsed() < Duration::from_millis(3_500),
        "the start waited {:?}",
        started.elapsed()
    );
    assert_eq!(given_of(&command, "PATH").as_deref(), Some(FINDER_PATH));
    assert!(said.is_some(), "the log says the shell gave none");
}

/// Login files that are slow, which both questions run, leave the interactive one its
/// whole limit: the two are asked at once, not one after the other, and the start still
/// waits one limit at most. Here the login files of each question wait until the other's
/// have begun: asked at once, both go on at once; asked one after the other, the first
/// waits out its whole limit. No clock decides it but that limit (a loaded machine
/// took more than the 1.5 s a sleep left it).
#[cfg(unix)]
#[test]
fn slow_login_files_leave_the_interactive_question_its_whole_limit() {
    let home = tempfile::tempdir().expect("home");
    let shell = login_shell(
        home.path(),
        "case \"$1\" in *i*) asked=interactive other=login ;; *) asked=login other=interactive ;; esac\n\
         : > \"${0%/*}/$asked\"\n\
         while [ ! -e \"${0%/*}/$other\" ] && kill -0 $PPID 2>/dev/null; do /bin/sleep 0.05; done\n\
         case \"$1\" in *i*) export PARITY_ONLY_INTERACTIVE=1 ;; esac\nexec /bin/sh -c \"$2\"",
    );
    let limit = Duration::from_secs(20);
    let started = Instant::now();
    let (command, said) = the_daemons(&finder(Some(shell.as_path()), &[]), limit);
    assert!(
        started.elapsed() < limit,
        "the start waited {:?}: one question waited out its limit for the other",
        started.elapsed()
    );
    assert_eq!(
        given_of(&command, "PARITY_ONLY_INTERACTIVE").as_deref(),
        Some("1"),
        "the interactive question ran out of time behind the login one"
    );
    assert_eq!(said, None);
}

/// The app's quit ends every shell the lookup asks, when it asks two at once.
#[cfg(unix)]
#[test]
fn ending_a_lookup_ends_both_shells_it_asks_at_once() {
    let lookup = LoginLookup::default();
    let mut first = Command::new("/bin/sleep");
    first.arg("30").stdout(Stdio::piped());
    let mut second = Command::new("/bin/sleep");
    second.arg("30").stdout(Stdio::piped());
    let one = lookup.start(&mut first).expect("the first is started");
    let two = lookup.start(&mut second).expect("the second is started");
    assert_ne!(one.asked, two.asked);
    lookup.end();
    let until = Instant::now() + Duration::from_secs(3);
    let ended = |asked| lookup.status(asked).is_ok_and(|status| status.is_some());
    while Instant::now() < until && !(ended(one.asked) && ended(two.asked)) {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        ended(one.asked) && ended(two.asked),
        "a shell outlived the quit"
    );
    lookup.finish(one.asked);
    lookup.finish(two.asked);
    assert!(lookup.lock().shells.is_empty(), "a shell is still held");
}
