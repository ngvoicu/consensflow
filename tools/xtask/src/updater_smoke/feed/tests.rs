use super::*;
use crate::updater_smoke::say::Line;
use crate::updater_smoke::testing::{env, https_get as ask, unanswered};

#[test]
fn offers_a_release_the_way_a_releases_feed_does_a_versioned_github_asset_signed() {
    let document = feed_document("3.0.0-alpha.82", "SIGNATURE");
    let platform = &document["platforms"]["darwin-aarch64"];
    assert_eq!(document["version"], "3.0.0-alpha.82");
    assert_eq!(platform["signature"], "SIGNATURE");
    assert_eq!(
        platform["url"],
        format!(
            "https://github.com/ngvoicu/consensflow/releases/download/v3.0.0-alpha.82/{}",
            archive_name("3.0.0-alpha.82")
        )
    );
    let name = archive_name("3.0.0-alpha.82");
    assert!(name.ends_with(".app.tar.gz"));
    assert!(name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)));
    assert!(name.contains("3.0.0-alpha.82"));
    assert!(!document["pub_date"].is_null() && !document["notes"].is_null());
    // The keys are in the order a release's feed has them, as the app's own are written.
    let keys: Vec<_> = document.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["version", "notes", "pub_date", "platforms"]);
}

#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "the certificates are made by the system's openssl"
)]
fn serves_the_feed_and_the_archive_when_offered_and_not_found_when_not() {
    let folder = tempfile::tempdir().unwrap();
    let tls = make_tls(folder.path(), &env()).unwrap();
    let (say, lines) = Say::channel();
    let mut served = serve_updates(&tls, &say).unwrap();
    let get = |path: &str| ask(&served.url, &tls.ca_cert, "GET", path).unwrap();

    assert_eq!(get("/feed").0, 404, "nothing offered yet");
    assert_eq!(get("/archive").0, 404);
    served.offer(
        "3.0.0-alpha.82",
        "SIGNATURE",
        Arc::new(b"archive bytes".to_vec()),
    );
    let (status, feed) = get("/feed");
    assert_eq!(status, 200);
    let feed: Value = serde_json::from_slice(&feed).unwrap();
    assert_eq!(feed, feed_document("3.0.0-alpha.82", "SIGNATURE"));
    assert_eq!(get("/archive"), (200, b"archive bytes".to_vec()));
    assert_eq!(get("/elsewhere").0, 404);
    assert_eq!(get("/feed?x=1").0, 404, "the address is the whole of it");
    // A head is answered as a get is, with no body.
    assert_eq!(
        ask(&served.url, &tls.ca_cert, "HEAD", "/archive").unwrap(),
        (200, Vec::new())
    );
    // What is offered next replaces it.
    served.offer("3.0.0-alpha.83", "OTHER", Arc::new(b"other bytes".to_vec()));
    let feed: Value = serde_json::from_slice(&get("/feed").1).unwrap();
    assert_eq!(feed, feed_document("3.0.0-alpha.83", "OTHER"));
    assert_eq!(get("/archive").1, b"other bytes");

    served.close();
    drop(say);
    assert_eq!(lines.iter().collect::<Vec<_>>(), [], "no client failed");
    // Closed, it answers no one. The port is no proof of it: another process can hold it.
    assert!(
        unanswered(&served.url, &tls.ca_cert, Duration::from_secs(2)),
        "a closed feed answered"
    );
}

#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "the certificates are made by the system's openssl"
)]
fn a_feed_that_is_up_answers_and_a_port_nothing_holds_does_not() {
    let folder = tempfile::tempdir().unwrap();
    let tls = make_tls(folder.path(), &env()).unwrap();
    let (say, _lines) = Say::channel();
    let mut served = serve_updates(&tls, &say).unwrap();
    // The patience is only the longest a silence is waited for: an answer ends it.
    assert!(!unanswered(
        &served.url,
        &tls.ca_cert,
        Duration::from_secs(60)
    ));
    served.close();
    // The port of a feed that is closed is refused at once, or answered by no one.
    assert!(unanswered(
        &served.url,
        &tls.ca_cert,
        Duration::from_millis(300)
    ));
}

/// A process that holds the socket a server closed: the port stays open, the system takes
/// the connections for it, and no one serves them. It happens with no one meaning it: the
/// socket of a server is made close-on-exec in a second step on macOS, and a program that
/// starts a process in between, as these tests do from many threads, leaves it the socket.
#[cfg(unix)]
struct Holder(std::process::Child);

#[cfg(unix)]
impl Holder {
    /// A listener on a port of the loopback, closed here and held by a process that
    /// was started while its socket was inherited.
    fn of_a_closed_listener() -> (Self, std::net::SocketAddr) {
        use std::net::Ipv4Addr;
        use std::os::fd::AsRawFd;
        use std::process::Stdio;

        use nix::fcntl::{fcntl, FcntlArg, FdFlag};

        use crate::process::{spawn, Invocation};

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        fcntl(listener.as_raw_fd(), FcntlArg::F_SETFD(FdFlag::empty())).unwrap();
        let sleeping = Invocation::new("/bin/sleep", Path::new(".")).arg("600");
        let process = spawn(&sleeping, &Env::default(), Stdio::null(), false).unwrap();
        drop(listener);
        (Self(process), address)
    }
}

#[cfg(unix)]
impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[cfg(unix)]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "the certificates are made by the system's openssl"
)]
fn a_port_a_process_holds_after_the_server_closed_it_takes_a_connect_and_answers_no_one() {
    let folder = tempfile::tempdir().unwrap();
    let tls = make_tls(folder.path(), &env()).unwrap();
    let (_holder, address) = Holder::of_a_closed_listener();
    let started = std::time::Instant::now();
    // The listener was closed here, and a connect to its port goes through all the same,
    // which is why a failed connect cannot be asked of a closed server.
    assert!(TcpStream::connect(address).is_ok());
    let held = format!("https://127.0.0.1:{}/feed", address.port());
    assert!(unanswered(&held, &tls.ca_cert, Duration::from_millis(300)));
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "the silence is waited for as long as it is given"
    );
}

#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "the certificates are made by the system's openssl"
)]
fn a_client_that_does_not_trust_the_feeds_authority_is_refused_and_the_run_is_told() {
    let folder = tempfile::tempdir().unwrap();
    // The folders are made as the case's machine makes its own.
    fs::create_dir_all(folder.path().join("ours")).unwrap();
    fs::create_dir_all(folder.path().join("theirs")).unwrap();
    let ours = make_tls(&folder.path().join("ours"), &env()).unwrap();
    let theirs = make_tls(&folder.path().join("theirs"), &env()).unwrap();
    let (say, lines) = Say::channel();
    let mut served = serve_updates(&ours, &say).unwrap();
    let refused = ask(&served.url, &theirs.ca_cert, "GET", "/feed");
    assert!(refused.is_err(), "{refused:?}");
    served.close();
    drop(say);
    let told: Vec<_> = lines.iter().collect();
    assert!(
        told.iter()
            .any(|line| matches!(line, Line::Err(text) if text.starts_with("updater TLS: "))),
        "{told:?}"
    );
}

#[test]
fn a_folder_that_is_not_there_is_no_place_for_a_certificate() {
    let folder = tempfile::tempdir().unwrap();
    let said = make_tls(&folder.path().join("nowhere"), &env())
        .unwrap_err()
        .to_string();
    assert!(said.starts_with("could not write "), "{said}");
}

#[test]
fn a_certificate_or_key_that_is_none_is_said_so() {
    let (say, _lines) = Say::channel();
    let said = serve_updates(
        &Tls {
            ca_cert: PathBuf::from("root.pem"),
            key: b"not a key".to_vec(),
            cert: b"not a certificate".to_vec(),
        },
        &say,
    )
    .err()
    .unwrap()
    .to_string();
    assert!(said.starts_with("the feed's "), "{said}");
}

#[test]
fn a_request_is_read_to_the_end_of_its_head_and_no_further() {
    let mut sent: &[u8] = b"GET /feed HTTP/1.1\r\nhost: x\r\n\r\nbody that is not read";
    assert_eq!(
        read_head(&mut sent).unwrap(),
        b"GET /feed HTTP/1.1\r\nhost: x\r\n\r\n"
    );
    assert_eq!(sent, b"body that is not read");
    // A client that stops short, or says more than anyone sends, is let go of.
    assert_eq!(read_head(&mut &b"GET /fe"[..]).unwrap(), b"GET /fe");
    let endless = vec![b'a'; MOST_OF_A_REQUEST * 2];
    assert_eq!(
        read_head(&mut endless.as_slice()).unwrap().len(),
        MOST_OF_A_REQUEST
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn the_update_of_an_app_is_its_archive_and_the_signature_of_its_bytes() {
    use crate::updater_smoke::bundle::run;
    use crate::updater_smoke::signing::{generate_key, tauri_bin};
    use crate::updater_smoke::testing::{fake_bundle, Fake};

    let root = crate::context::Context::new(&Env::default()).unwrap().root;
    if !tauri_bin(&root).exists() {
        eprintln!("skipped: the Tauri CLI is not installed");
        return;
    }
    let env = env();
    let folder = tempfile::tempdir().unwrap();
    let key = generate_key(&root, &folder.path().join("keys"), &env).unwrap();
    let app = fake_bundle(&folder.path().join("update"), &Fake::default());
    let update = signed_update(
        &root,
        &key.private_key,
        &app,
        &folder.path().join("served"),
        &env,
    )
    .unwrap();
    assert_eq!(update.version, "3.0.0-alpha.82");
    let archive = folder
        .path()
        .join("served")
        .join(archive_name("3.0.0-alpha.82"));
    assert_eq!(*update.bytes, fs::read(&archive).unwrap());
    assert_eq!(
        update.signature,
        fs::read_to_string(format!("{}.sig", archive.display()))
            .unwrap()
            .trim()
    );
    let listed = run("/usr/bin/tar", &args!["-tzf", &archive], &env).unwrap();
    assert!(listed
        .lines()
        .all(|name| name.starts_with("ConsensFlow.app")));
}
