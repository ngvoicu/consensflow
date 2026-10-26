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
        login_path_in(&shell, Duration::from_secs(5), &LoginLookup::default()),
        std::env::var("PATH").ok()
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
    let _ = Command::new("/bin/kill")
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
    assert!(login_path_in(&shell, Duration::from_secs(5), &lookup).is_some());
    std::fs::remove_file(&started).expect("the shell ran once");
    lookup.end();
    assert_eq!(login_path_in(&shell, Duration::from_secs(5), &lookup), None);
    let mut asked = Command::new(&shell);
    asked.stdout(Stdio::piped());
    assert!(lookup.start(&mut asked).is_none(), "a shell was started");
    assert!(lookup.status().is_err(), "a shell is held");
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
    let mut shell = Command::new("/bin/sh");
    shell
        .args([
            "-c",
            &format!("/bin/sleep 30 & echo $! > '{}'; wait", sleeper.display()),
        ])
        .stdout(Stdio::piped());
    let lookup = LoginLookup::default();
    let _output = lookup.start(&mut shell).expect("the shell is started");
    let agent = pid_in(&sleeper);
    lookup.end();
    let until = Instant::now() + Duration::from_secs(3);
    let mut ended = false;
    while Instant::now() < until {
        if lookup.status().is_ok_and(|status| status.is_some()) {
            ended = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    lookup.finish();
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
    let mut shell = Command::new("/bin/sh");
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
    let _output = lookup.start(&mut shell).expect("the shell is started");
    let started = Instant::now();
    while !beat.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the shell never ran"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let mut stopped = false;
    lookup.finish_with(|| {
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
    let said = on_login_path(&mut command, app, timeout, &LoginLookup::default());
    (command, said)
}

/// The PATH the daemon's `command` is given, and nothing else.
#[cfg(unix)]
fn given(command: &Command) -> Vec<(String, String)> {
    described(command).2
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
    assert_eq!(given(&command), [("PATH".to_owned(), login)]);
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
