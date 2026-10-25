//! What the tests of the smoke's modules share: a signed bundle as the Rust tests
//! of the installed app's check build one (macOS programs under real names), the
//! environment the system's own tools are run in, a stand-in for the app that is
//! a script, and a client for the feed.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cf_base::env::Env;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use super::bundle::{run, IDENTITY};

/// What a fake bundle is made of.
pub struct Fake {
    pub version: &'static str,
    pub identity: &'static str,
    /// Whether it ships Node, as the releases before the deletion did.
    pub node: bool,
    /// Whether it has the `cf` a window runs.
    pub cf: bool,
    /// The plist's second version, which is the first where none is given.
    pub build_version: Option<&'static str>,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            version: "3.0.0-alpha.82",
            identity: IDENTITY,
            node: true,
            cf: true,
            build_version: None,
        }
    }
}

/// The environment the system's own tools are run in by a test.
pub fn env() -> Env {
    Env::from_process()
}

/// How a stand-in for the app reports, as the page does (`say`), and what it
/// does with its input (`reads`: it answers each line, and ends at `quit` or at
/// the end of its input).
pub const PRELUDE: &str = "say() { printf 'consensflow-selftest {\"event\":\"%s\",\"pid\":%s,\"data\":%s}\\n' \"$1\" \"$$\" \"$2\"; }
reads() {
  while IFS= read -r line; do
    say got \"{\\\"line\\\":\\\"$line\\\"}\"
    [ \"$line\" = quit ] && exit 0
  done
  exit 0
}";

/// Makes the file at `binary` a stand-in for the app: a script that is the prelude
/// and `script`, which is run where the system has a mode to make it one (the
/// tests that start it are the macOS ones, and are ignored elsewhere).
pub fn stand_in_for_the_app(binary: &Path, script: &str) {
    fs::write(binary, format!("#!/bin/sh\n{PRELUDE}\n{script}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(binary, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// What a client that trusts the authority at `ca` is answered to `method path`
/// by the feed whose address is `url`: the status and the body.
pub fn https_get(url: &str, ca: &Path, method: &str, path: &str) -> io::Result<(u16, Vec<u8>)> {
    let all = exchange(url, ca, method, path, None)?;
    let split = all
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a response has a head");
    let head = String::from_utf8_lossy(&all[..split]).into_owned();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    Ok((status, all[split + 4..].to_vec()))
}

/// Whether nothing at `url` answers a client that trusts the authority at `ca`, as
/// the feed of the run would: a port that nothing listens at refuses it, and one
/// that something else holds takes the connection and says nothing a client takes
/// for the feed's, which `patience` ends the wait for. A port is no proof that a
/// server is gone: a process the test's own program started while the server's
/// socket was being made can be left that socket (macOS makes it close-on-exec
/// in a second step), and keeps the port open after the server closed it.
pub fn unanswered(url: &str, ca: &Path, patience: Duration) -> bool {
    exchange(url, ca, "GET", "/feed", Some(patience)).map_or(true, |answer| answer.is_empty())
}

/// Everything the feed whose address is `url` sends back to a client that trusts
/// the authority at `ca`, for `method path`: the whole of it, or, where `patience`
/// is given, as much as it says within that time of each step.
fn exchange(
    url: &str,
    ca: &Path,
    method: &str,
    path: &str,
    patience: Option<Duration>,
) -> io::Result<Vec<u8>> {
    let port: u16 = url
        .trim_start_matches("https://127.0.0.1:")
        .trim_end_matches("/feed")
        .parse()
        .unwrap();
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from_pem_file(ca).unwrap())
        .unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    // The address the app is told, and which the certificate holds.
    let name = ServerName::from(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let connection = ClientConnection::new(Arc::new(config), name).unwrap();
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let socket = match patience {
        None => TcpStream::connect(address)?,
        Some(patience) => {
            let socket = TcpStream::connect_timeout(&address, patience)?;
            socket.set_read_timeout(Some(patience))?;
            socket.set_write_timeout(Some(patience))?;
            socket
        }
    };
    let mut stream = StreamOwned::new(connection, socket);
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nhost: 127.0.0.1\r\nconnection: close\r\n\r\n"
    )?;
    let mut all = Vec::new();
    match stream.read_to_end(&mut all) {
        Ok(_) => {}
        Err(cause) if cause.kind() == io::ErrorKind::UnexpectedEof => {}
        Err(cause) => return Err(cause),
    }
    Ok(all)
}

/// A signed `ConsensFlow.app` in `parent`, made as `fake` says. macOS programs
/// stand in for its own: `/bin/echo` is its executable, its `cf` and its Node.
pub fn fake_bundle(parent: &Path, fake: &Fake) -> PathBuf {
    let app = parent.join("ConsensFlow.app");
    for dir in ["Contents/MacOS", "Contents/Resources/cli/bin"] {
        fs::create_dir_all(app.join(dir)).unwrap();
    }
    let macos = app.join("Contents").join("MacOS");
    let cli = app.join("Contents").join("Resources").join("cli");
    fs::copy("/bin/echo", macos.join("app")).unwrap();
    let build_version = fake.build_version.unwrap_or(fake.version);
    let (identity, version) = (fake.identity, fake.version);
    fs::write(
        app.join("Contents").join("Info.plist"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>\
             <key>CFBundleIdentifier</key><string>{identity}</string>\
             <key>CFBundleExecutable</key><string>app</string>\
             <key>CFBundlePackageType</key><string>APPL</string>\
             <key>CFBundleVersion</key><string>{build_version}</string>\
             <key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>"
        ),
    )
    .unwrap();
    let env = env();
    if fake.cf {
        let cf = cli.join("bin").join("cf");
        fs::copy("/bin/echo", &cf).unwrap();
        run(
            "/usr/bin/codesign",
            &args!["--force", "--sign", "-", &cf],
            &env,
        )
        .unwrap();
    }
    if fake.node {
        fs::copy("/bin/echo", macos.join("node")).unwrap();
        for dir in ["hosts", "src"] {
            fs::create_dir_all(cli.join(dir)).unwrap();
        }
        fs::write(cli.join("bin").join("cf.mjs"), "test Node-era file").unwrap();
        fs::write(
            cli.join("package.json"),
            format!("{{\"name\":\"consensflow\",\"version\":\"{version}\"}}"),
        )
        .unwrap();
    }
    run(
        "/usr/bin/codesign",
        &args!["--force", "--deep", "--sign", "-", &app],
        &env,
    )
    .unwrap();
    app
}

/// The hold on `cache` once it is let go of. A test that takes it back at once can
/// meet a program another test is starting (a `fork` copies the descriptors of the
/// process until the program is `exec`ed, and the lock goes with the last of them),
/// so it is tried for a moment before it is said to be held.
pub fn hold_when_free(cache: &Path) -> super::cache::Hold {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match super::cache::hold(cache) {
            Ok(hold) => return hold,
            Err(held) if std::time::Instant::now() > until => {
                panic!("the cache is still held: {held}")
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
}
