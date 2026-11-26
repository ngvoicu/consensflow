//! The reader of the log as it runs (T-1194): what it is asked, and that a program's lines
//! come one at a time, its end comes, and a stop ends it.

use std::time::{Duration, Instant};

use super::*;

fn lines_until_the_end(stream: &mut LogStream) -> Vec<String> {
    let mut lines = Vec::new();
    let stop = Instant::now() + Duration::from_secs(20);
    while Instant::now() < stop {
        match stream.recv(Duration::from_millis(200)) {
            Read::Line(line) => lines.push(line),
            Read::Quiet => {}
            Read::End => return lines,
        }
    }
    panic!("the reader never ended: {lines:?}");
}

#[test]
fn the_log_is_asked_for_the_dialogs_and_for_the_requests_in_the_apps_name_alone() {
    let predicate = predicate(1811);
    assert_eq!(
        predicate,
        "subsystem == \"com.apple.TCC\" AND (eventMessage CONTAINS \"AUTHREQ_PROMPTING\" OR \
         (eventMessage CONTAINS \"AUTHREQ_ATTRIBUTION\" AND eventMessage CONTAINS \"pid=1811,\"))"
    );
    let arguments: Vec<String> = log_arguments(1811)
        .into_iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    // The log is named by its whole path, for a shell finds nothing else on the path it is given.
    // A refusal coming through the error stream is tested with the real shell below.
    assert_eq!(arguments[0], "-c");
    assert_eq!(
        &arguments[3..10],
        [
            "/usr/bin/log",
            "stream",
            "--level",
            "info",
            "--style",
            "ndjson",
            "--predicate"
        ]
    );
    assert_eq!(arguments.len(), 11);
}

#[test]
fn a_programs_lines_come_one_at_a_time_and_then_its_end() {
    let mut stream = LogStream::start(
        Path::new("/bin/sh"),
        vec!["-c".into(), "echo first; echo second".into()],
    )
    .expect("it starts");
    assert_eq!(lines_until_the_end(&mut stream), ["first", "second"]);
    // Once ended it stays ended.
    assert_eq!(stream.recv(Duration::from_millis(50)), Read::End);
}

#[test]
fn a_program_that_cannot_start_says_why() {
    let why = LogStream::start(Path::new("/no/such/reader"), vec![]).err();
    assert!(
        why.is_some_and(|why| why.contains("/no/such/reader")),
        "it says why"
    );
}

#[test]
fn a_reader_that_is_asked_to_end_ends_though_it_has_nothing_to_say() {
    let mut args = log_arguments(1);
    args.truncate(3);
    args.extend([
        "/bin/sh".into(),
        "-c".into(),
        "echo started; exec /bin/sleep 15".into(),
    ]);
    let mut stream = LogStream::start(Path::new("/bin/sh"), args).expect("it starts");
    let stop = Instant::now() + Duration::from_secs(20);
    // Its first line, and then nothing for as long as the test waits.
    loop {
        assert!(Instant::now() < stop, "no first line");
        if stream.recv(Duration::from_millis(200)) == Read::Line("started".to_owned()) {
            break;
        }
    }
    assert_eq!(stream.recv(Duration::from_millis(300)), Read::Quiet);
    let asked = Instant::now();
    stream.end();
    loop {
        assert!(Instant::now() < stop, "it did not end");
        if stream.recv(Duration::from_millis(200)) == Read::End {
            break;
        }
    }
    assert!(
        asked.elapsed() < Duration::from_secs(3),
        "{:?}",
        asked.elapsed()
    );
}

// A socket connection is the stand-in's lifetime witness. It never writes to the socket or its
// output; EOF says it ended without ever looking up, or signalling, its process number. The
// stand-in also ends itself if a broken watcher leaves it behind.
mod lifetime {
    use std::io::{ErrorKind, Read as _};
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::process::{Child, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    use super::*;

    const SOCKET_ENV: &str = "CF_TCC_TEST_SOCKET";
    const FAKE_LIFETIME: Duration = Duration::from_secs(15);
    const END_WITHIN: Duration = Duration::from_secs(3);
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Witness {
        dir: PathBuf,
        socket: PathBuf,
        listener: UnixListener,
    }

    impl Witness {
        fn new() -> Self {
            let dir = PathBuf::from(format!(
                "/private/tmp/cf-tcc-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
            let socket = dir.join("witness");
            let listener = UnixListener::bind(&socket).unwrap();
            listener.set_nonblocking(true).unwrap();
            Self {
                dir,
                socket,
                listener,
            }
        }

        fn accept(&self) -> UnixStream {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match self.listener.accept() {
                    Ok((connection, _)) => {
                        connection.set_nonblocking(false).unwrap();
                        return connection;
                    }
                    Err(why)
                        if why.kind() == ErrorKind::WouldBlock && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(why) => panic!("the quiet log did not connect: {why}"),
                }
            }
        }
    }

    impl Drop for Witness {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.socket);
            let _ = std::fs::remove_dir(&self.dir);
        }
    }

    // log_arguments supplies the same shell as production, but asks it to run a test program.
    fn arguments_for(program: &Path, args: &[OsString]) -> Vec<OsString> {
        let mut args_for_log = log_arguments(1);
        args_for_log.truncate(3);
        args_for_log.push(program.into());
        args_for_log.extend_from_slice(args);
        args_for_log
    }

    fn quiet_log_arguments(socket: &Path) -> Vec<OsString> {
        let args = vec![
            "-c".into(),
            "exec \"$@\" >/dev/null".into(),
            "sh".into(),
            "/usr/bin/env".into(),
            format!("{SOCKET_ENV}={}", socket.display()).into(),
            std::env::current_exe().unwrap().into_os_string(),
            "--exact".into(),
            "tcc::real::tests::lifetime::quiet_log_stand_in".into(),
            "--ignored".into(),
        ];
        arguments_for(Path::new("/bin/sh"), &args)
    }

    fn quiet_log(socket: &Path) -> LogStream {
        LogStream::start(Path::new("/bin/sh"), quiet_log_arguments(socket))
            .expect("the watcher starts")
    }

    fn assert_still_running(connection: &mut UnixStream) {
        connection
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let mut byte = [0];
        match connection.read(&mut byte) {
            Err(why) if matches!(why.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            other => panic!("the quiet log ended while its host was alive: {other:?}"),
        }
    }

    fn assert_ended(connection: &mut UnixStream) {
        connection.set_read_timeout(Some(END_WITHIN)).unwrap();
        let mut byte = [0];
        let result = connection.read(&mut byte);
        if matches!(&result, Err(why) if matches!(why.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
        {
            // On a red, wait for the stand-in's own deadline before failing: no fixture is left.
            connection
                .set_read_timeout(Some(FAKE_LIFETIME + Duration::from_secs(5)))
                .unwrap();
            assert_eq!(
                connection.read(&mut byte).unwrap(),
                0,
                "the stand-in must end itself"
            );
            panic!("the quiet log outlived its host or ordinary end by {END_WITHIN:?}");
        }
        assert_eq!(result.unwrap(), 0, "the stand-in never writes");
    }

    // This guard holds a child that it started and has not reaped. It never signals the log or
    // any PID read from a list, and reaps the host even if an assertion fails.
    struct Host(Child);

    impl Drop for Host {
        fn drop(&mut self) {
            if matches!(self.0.try_wait(), Ok(None)) {
                let _ = self.0.kill();
            }
            let _ = self.0.wait();
        }
    }

    #[test]
    #[ignore = "test helper, invoked by the lifetime tests"]
    fn quiet_log_stand_in() {
        let env = Env::from_process();
        let socket = env.os(SOCKET_ENV).expect("test socket");
        let _connection = UnixStream::connect(socket).expect("connect to the test");
        thread::sleep(FAKE_LIFETIME);
    }

    #[test]
    #[ignore = "test helper, invoked by the lifetime test"]
    fn host_stand_in() {
        let env = Env::from_process();
        let socket = env.os(SOCKET_ENV).expect("test socket");
        let mut stream = quiet_log(Path::new(&socket));
        thread::sleep(FAKE_LIFETIME + Duration::from_secs(10));
        stream.end();
        let _ = lines_until_the_end(&mut stream);
    }

    #[test]
    fn a_quiet_log_ends_when_its_host_is_killed() {
        let witness = Witness::new();
        let mut host = Host(
            crate::testing::command(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tcc::real::tests::lifetime::host_stand_in",
                    "--ignored",
                ])
                .env_clear()
                .env(SOCKET_ENV, &witness.socket)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let mut connection = witness.accept();
        assert_still_running(&mut connection);
        assert!(host.0.try_wait().unwrap().is_none(), "the host is alive");
        host.0.kill().unwrap(); // SIGKILL, only to the unreaped child started above.
        host.0.wait().unwrap();
        assert_ended(&mut connection);
    }

    #[test]
    fn a_quiet_log_ends_when_the_reader_is_asked_to_end() {
        let witness = Witness::new();
        let mut stream = quiet_log(&witness.socket);
        let mut connection = witness.accept();
        assert_still_running(&mut connection);
        stream.end();
        assert_ended(&mut connection);
        assert!(lines_until_the_end(&mut stream).is_empty());
    }

    #[test]
    fn a_watcher_dropped_before_end_still_ends_its_log() {
        let witness = Witness::new();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let run = Run {
                program: "/bin/sh".into(),
                args: quiet_log_arguments(&witness.socket),
                verbatim: false,
            };
            let env = Env::from_vars([("PATH", "/usr/bin:/bin")]);
            let child = LogChild::new(spawn(&run, None, &env, Streams::Lines).unwrap());
            let mut connection = witness.accept();
            assert_still_running(&mut connection);
            drop(child);
            assert_ended(&mut connection);
        });
    }

    #[test]
    fn a_refused_log_says_why_and_ends() {
        let args = vec![
            "-c".into(),
            "printf \"Must be admin to run 'stream' command\\n\" >&2".into(),
        ];
        let mut stream = LogStream::start(
            Path::new("/bin/sh"),
            arguments_for(Path::new("/bin/sh"), &args),
        )
        .expect("the watcher starts");
        assert_eq!(
            lines_until_the_end(&mut stream),
            ["Must be admin to run 'stream' command"]
        );
    }
}
