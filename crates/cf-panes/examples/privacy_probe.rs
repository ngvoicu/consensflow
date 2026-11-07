//! The end-to-end proof of the watch of macOS privacy dialogs (T-1194), run as the main program of
//! a stand-in app: a bundle with an id of its own and no grants, started through LaunchServices, so
//! that it is its own responsible process as ConsensFlow's app is. It opens a real window (a pty)
//! running a command, as the pane host does, starts the real watch on a loopback bridge, and writes
//! every event the host sends the daemon, as the daemon's end reads it, to a file, one line each,
//! with the milliseconds since the probe began. It runs the command in the window and nothing else:
//! the dialog the command raises is the system's, and is left for a person to answer.
//!
//!     privacy_probe '<command run in the window>' <events file> <seconds to run>
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::unwrap_used,
    clippy::expect_used
)]

#[cfg(target_os = "macos")]
fn main() {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use cf_bridge::BridgeBuilder;
    use cf_panes::privacy::watch_privacy_dialogs;
    use cf_panes::pty::PaneTable;
    use cf_proto::bridge::Role;
    use portable_pty::PtySize;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [command, events, seconds] = &args[..] else {
        eprintln!("usage: privacy_probe '<command>' <events file> <seconds>");
        std::process::exit(2);
    };
    let began = Instant::now();
    let (host, mut daemon) = UnixStream::pair().expect("a socket pair");
    daemon
        .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"probe\"}\n")
        .expect("the handle line");
    let connected = BridgeBuilder::new(Role::Host, 1024 * 1024)
        .connect(host.try_clone().expect("a clone"), host)
        .expect("the bridge");
    let reader = BufReader::new(daemon.try_clone().expect("a clone"));
    let file = events.clone();
    std::thread::spawn(move || {
        let mut out = std::fs::File::create(file).expect("the events file");
        for line in reader.lines().map_while(Result::ok) {
            let _ = writeln!(out, "{} {line}", began.elapsed().as_millis());
            let _ = out.flush();
        }
    });
    let panes = Arc::new(PaneTable::new());
    let bridge = connected.bridge.clone();
    let _watch = watch_privacy_dialogs(Arc::clone(&panes), move || Some(bridge.clone()))
        .expect("the watch starts");
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let argv = vec!["/bin/sh".to_owned(), "-c".to_owned(), command.clone()];
    let opened = panes
        .open(Path::new("/tmp"), &argv, &HashMap::new(), size)
        .expect("a window");
    println!(
        "pid {} opened window {} generation {}",
        std::process::id(),
        opened.key.id,
        opened.key.generation
    );
    std::thread::sleep(Duration::from_secs(seconds.parse().expect("seconds")));
    let _ = panes.kill(&opened.key);
}

#[cfg(not(target_os = "macos"))]
fn main() {}
