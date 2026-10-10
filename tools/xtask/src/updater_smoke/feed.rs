//! The feed the apps of the smoke ask, as a release's `latest.json` is, and the
//! archive it names, served over HTTPS on this machine's loopback with a
//! certificate made for the run. The app is told where the feed is by the
//! packaged self-test (`CONSENSFLOW_SELFTEST_UPDATER_URL`, loopback HTTPS only)
//! and which certificate holds its address; the feed's own archive address is
//! checked as a release's is (a GitHub asset of this version), and the archive is
//! fetched from this server.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cf_base::env::Env;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde_json::{json, Value};

use super::bundle::{archive_of, plist_value, run};
use super::say::Say;
use super::signing::sign_file;
use super::{files, Error, Result};

/// How long a client is given to say what it wants, and to take what is sent.
const CLIENT_PATIENCE: Duration = Duration::from_secs(10);

/// How often the server looks for a client, and for the end of the run.
const LOOK: Duration = Duration::from_millis(10);

/// The most of a request the server reads before it takes it for nothing it serves.
const MOST_OF_A_REQUEST: usize = 64 * 1024;

/// The certificates of a run: an authority, which the app is told to trust, and the
/// certificate of the feed's server, which it made.
#[derive(Debug, Clone)]
pub struct Tls {
    /// The authority's certificate, a file the app reads.
    pub ca_cert: PathBuf,
    key: Vec<u8>,
    cert: Vec<u8>,
}

/// A certificate authority and a server certificate for localhost, valid for a day.
pub fn make_tls(directory: &Path, env: &Env) -> Result<Tls> {
    let file = |name: &str| directory.join(name);
    let (ca_key, ca_cert) = (file("root.key"), file("root.pem"));
    let (server_key, server_csr, server_cert) =
        (file("server.key"), file("server.csr"), file("server.pem"));
    let extensions = file("server.ext");
    fs::write(
        &extensions,
        [
            "subjectAltName=DNS:localhost,IP:127.0.0.1",
            "basicConstraints=critical,CA:FALSE",
            "keyUsage=critical,digitalSignature,keyEncipherment",
            "extendedKeyUsage=serverAuth",
            "subjectKeyIdentifier=hash",
            "authorityKeyIdentifier=keyid,issuer",
            "",
        ]
        .join("\n"),
    )
    .map_err(files("write", &extensions))?;
    let openssl = |args: &[OsString]| run("openssl", args, env).map(drop);
    openssl(&args![
        "req",
        "-x509",
        "-days",
        "1",
        "-subj",
        "/CN=ConsensFlow updater smoke root",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        &ca_key,
        "-out",
        &ca_cert
    ])?;
    openssl(&args![
        "req",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        &server_key,
        "-out",
        &server_csr,
        "-subj",
        "/CN=localhost"
    ])?;
    openssl(&args![
        "x509",
        "-req",
        "-in",
        &server_csr,
        "-CA",
        &ca_cert,
        "-CAkey",
        &ca_key,
        "-CAcreateserial",
        "-sha256",
        "-days",
        "1",
        "-extfile",
        &extensions,
        "-out",
        &server_cert
    ])?;
    Ok(Tls {
        key: fs::read(&server_key).map_err(files("read", &server_key))?,
        cert: fs::read(&server_cert).map_err(files("read", &server_cert))?,
        ca_cert,
    })
}

/// The feed's file name for the archive of `version`: a release's, which the app checks.
pub fn archive_name(version: &str) -> String {
    format!("ConsensFlow-{version}_aarch64.app.tar.gz")
}

/// The release the feed offers.
pub fn feed_document(version: &str, signature: &str) -> Value {
    json!({
        "version": version,
        "notes": "Packaged updater acceptance candidate.",
        "pub_date": "2026-09-09T12:00:00Z",
        "platforms": {
            "darwin-aarch64": {
                "url": format!(
                    "https://github.com/ngvoicu/consensflow/releases/download/v{version}/{}",
                    archive_name(version)
                ),
                "signature": signature,
            }
        }
    })
}

/// The update of an app, signed: the version it is, the bytes of its archive and
/// the signature of those bytes.
#[derive(Debug, Clone)]
pub struct SignedUpdate {
    pub version: String,
    pub bytes: Arc<Vec<u8>>,
    pub signature: String,
}

/// The update of `app`, signed: its archive in `directory`, and the signature of
/// those bytes by `private_key` (the signer of the checkout `checkout` is).
pub fn signed_update(
    checkout: &Path,
    private_key: &Path,
    app: &Path,
    directory: &Path,
    env: &Env,
) -> Result<SignedUpdate> {
    let version = plist_value(app, "CFBundleShortVersionString")?;
    let file = archive_of(app, &directory.join(archive_name(&version)), env)?;
    let bytes = fs::read(&file).map_err(files("read", &file))?;
    Ok(SignedUpdate {
        bytes: Arc::new(bytes),
        signature: sign_file(checkout, private_key, &file, env)?,
        version,
    })
}

/// What the server answers with now: each what `offer` last set, and not found before.
#[derive(Default)]
struct Offer {
    feed: Option<Arc<Vec<u8>>>,
    archive: Option<Arc<Vec<u8>>>,
}

/// The server: the feed at `/feed` and the archive at `/archive`.
pub struct Feed {
    /// Where the apps ask: `https://127.0.0.1:<port>/feed`.
    pub url: String,
    offer: Arc<Mutex<Offer>>,
    over: Arc<AtomicBool>,
    serving: Option<JoinHandle<()>>,
}

/// The server's TLS: the run's certificate, for a client that trusts its authority.
fn tls_config(tls: &Tls) -> Result<Arc<ServerConfig>> {
    let words = |what: &str, cause: &dyn std::fmt::Display| {
        Error::new(format!("the feed's {what} is not usable: {cause}"))
    };
    let certs = CertificateDer::pem_slice_iter(&tls.cert)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|cause| words("certificate", &cause))?;
    let key = PrivateKeyDer::from_pem_slice(&tls.key).map_err(|cause| words("key", &cause))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|cause| words("protocols", &cause))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|cause| words("certificate and key", &cause))?;
    Ok(Arc::new(config))
}

/// Serves the feed at `/feed` and the archive at `/archive`: each what `offer`
/// last set, and not found before. Closes with the case.
pub fn serve_updates(tls: &Tls, say: &Say) -> Result<Feed> {
    let config = tls_config(tls)?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .map_err(|cause| Error::new(format!("could not listen on the loopback: {cause}")))?;
    listener
        .set_nonblocking(true)
        .map_err(|cause| Error::new(format!("could not listen on the loopback: {cause}")))?;
    let port = listener
        .local_addr()
        .map_err(|cause| Error::new(format!("could not tell the feed's port: {cause}")))?
        .port();
    let offer = Arc::new(Mutex::new(Offer::default()));
    let over = Arc::new(AtomicBool::new(false));
    let serving = {
        let (offer, over, say) = (Arc::clone(&offer), Arc::clone(&over), say.clone());
        thread::spawn(move || accept(&listener, &config, &offer, &over, &say))
    };
    Ok(Feed {
        url: format!("https://127.0.0.1:{port}/feed"),
        offer,
        over,
        serving: Some(serving),
    })
}

/// Takes each client that comes, to a thread of its own, until the feed is closed;
/// then waits for those that are being served.
fn accept(
    listener: &TcpListener,
    config: &Arc<ServerConfig>,
    offer: &Arc<Mutex<Offer>>,
    over: &AtomicBool,
    say: &Say,
) {
    let mut clients: Vec<JoinHandle<()>> = Vec::new();
    while !over.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((client, _)) => {
                let (config, offer, say) = (Arc::clone(config), Arc::clone(offer), say.clone());
                clients.push(thread::spawn(move || {
                    // A client that went away (the app quitting, or turning a download
                    // back) is no failure of the feed's; one that did not trust its
                    // certificate, or spoke no TLS, is for the run to be told.
                    if let Err(cause) = serve(&config, client, &offer) {
                        if cause.kind() == io::ErrorKind::InvalidData {
                            say.err(format!("updater TLS: {cause}"));
                        }
                    }
                }));
            }
            Err(cause) if cause.kind() == io::ErrorKind::WouldBlock => thread::sleep(LOOK),
            Err(cause) => {
                say.err(format!(
                    "updater TLS: the feed stopped taking clients: {cause}"
                ));
                break;
            }
        }
    }
    for client in clients {
        let _ = client.join();
    }
}

/// Answers the one request a client makes, and closes.
fn serve(config: &Arc<ServerConfig>, client: TcpStream, offer: &Mutex<Offer>) -> io::Result<()> {
    client.set_nonblocking(false)?;
    client.set_read_timeout(Some(CLIENT_PATIENCE))?;
    client.set_write_timeout(Some(CLIENT_PATIENCE))?;
    let connection = ServerConnection::new(Arc::clone(config)).map_err(io::Error::other)?;
    let mut stream = StreamOwned::new(connection, client);
    let head = read_head(&mut stream)?;
    let head = String::from_utf8_lossy(&head);
    let mut first = head.lines().next().unwrap_or_default().split(' ');
    let (method, target) = (
        first.next().unwrap_or_default(),
        first.next().unwrap_or_default(),
    );
    let body = {
        let offered = offer.lock().unwrap_or_else(PoisonError::into_inner);
        match target {
            "/feed" => offered.feed.clone().map(|body| ("application/json", body)),
            "/archive" => offered
                .archive
                .clone()
                .map(|body| ("application/gzip", body)),
            _ => None,
        }
    };
    match body {
        Some((kind, body)) => {
            let heading = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(heading.as_bytes())?;
            if method != "HEAD" {
                stream.write_all(&body)?;
            }
        }
        None => stream.write_all(
            b"HTTP/1.1 404 Not Found\r\ncontent-length: 9\r\nconnection: close\r\n\r\nnot found",
        )?,
    }
    stream.flush()?;
    stream.conn.send_close_notify();
    stream.flush()
}

/// What a client sent up to the end of its request's head, an empty line.
fn read_head(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") && head.len() < MOST_OF_A_REQUEST {
        if stream.read(&mut byte)? == 0 {
            break;
        }
        head.push(byte[0]);
    }
    Ok(head)
}

impl Feed {
    /// Offers `version` with `signature`, whose archive is `bytes`: what the app downloads.
    pub fn offer(&self, version: &str, signature: &str, bytes: Arc<Vec<u8>>) {
        let document = feed_document(version, signature).to_string().into_bytes();
        let mut offered = self.offer.lock().unwrap_or_else(PoisonError::into_inner);
        offered.feed = Some(Arc::new(document));
        offered.archive = Some(bytes);
    }

    /// Stops serving, once the clients being served are done.
    pub fn close(&mut self) {
        self.over.store(true, Ordering::Release);
        if let Some(serving) = self.serving.take() {
            let _ = serving.join();
        }
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests;
