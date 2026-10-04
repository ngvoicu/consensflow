//! Where a window's Codex server listens, and how its broker knows it is up.
//!
//! On Unix, a socket in a private folder. Windows has no Unix socket to
//! reach (a socket path is a named pipe to Node, and tokio has none there),
//! so there the server listens on loopback, on a port it picks and names when
//! it starts, and takes only this window's token, which no other program sees.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::js;
use sha2::{Digest, Sha256};

/// How long a Unix socket's path may be: macOS's `sun_path` is 104 bytes,
/// the final NUL among them.
#[cfg(unix)]
const SUN_PATH: usize = 104;

/// What the broker's connections to Codex's server go to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Tcp(SocketAddr),
    #[cfg(unix)]
    Unix(PathBuf),
}

/// Codex's server as the broker reaches it: where, and the `authorization`
/// every connection to it carries, if its endpoint asks for one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Upstream {
    pub(crate) target: Target,
    pub(crate) authorization: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum EndpointError {
    #[cfg(unix)]
    #[error("ConsensFlow home makes the Codex socket path too long")]
    TooLong,
    #[cfg(unix)]
    #[error("could not make the Codex socket folder {path}: {cause}")]
    Folder {
        path: PathBuf,
        cause: std::io::Error,
    },
    #[error("could not get random bytes: {0}")]
    Random(getrandom::Error),
}

/// Where one window's Codex server listens.
#[derive(Debug)]
pub(crate) struct Endpoint {
    directory: Option<PathBuf>,
    listen: Vec<OsString>,
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    /// Up once its socket is there.
    #[cfg(unix)]
    Socket(PathBuf),
    /// Up once it says where: `listening on: ws://127.0.0.1:PORT` on its
    /// stderr. Every connection to it carries this `authorization`.
    Loopback { authorization: String },
}

impl Endpoint {
    /// This platform's: a private socket on Unix, loopback on Windows.
    pub(crate) fn open(env: &Env) -> Result<Self, EndpointError> {
        #[cfg(unix)]
        return Self::socket(env);
        #[cfg(not(unix))]
        return {
            let _ = env;
            Self::loopback()
        };
    }

    /// A socket in a private folder under ConsensFlow's home, or the user's
    /// temporary folder when that path would not fit.
    #[cfg(unix)]
    pub(crate) fn socket(env: &Env) -> Result<Self, EndpointError> {
        let directory = create_socket_directory(env)?;
        let socket = directory.join("native.sock");
        let mut listen = OsString::from("unix://");
        listen.push(&socket);
        Ok(Self {
            directory: Some(directory),
            listen: vec!["--listen".into(), listen],
            kind: Kind::Socket(socket),
        })
    }

    /// Loopback, on a port the server picks, for a token made here.
    #[cfg_attr(
        unix,
        allow(
            dead_code,
            reason = "Windows starts Codex on loopback; Unix builds it in tests"
        )
    )]
    pub(crate) fn loopback() -> Result<Self, EndpointError> {
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).map_err(EndpointError::Random)?;
        let token = hex(&random);
        // The server is given the hash of the token's own text, as it is sent.
        let hash = hex(&Sha256::digest(token.as_bytes()));
        Ok(Self {
            directory: None,
            listen: [
                "--listen",
                "ws://127.0.0.1:0",
                "--ws-auth",
                "capability-token",
                "--ws-token-sha256",
                &hash,
            ]
            .map(OsString::from)
            .to_vec(),
            kind: Kind::Loopback {
                authorization: format!("Bearer {token}"),
            },
        })
    }

    /// The private folder of the socket, which goes with the session.
    pub(crate) fn directory(&self) -> Option<&Path> {
        self.directory.as_deref()
    }

    /// What follows `app-server` on the server's command line.
    pub(crate) fn listen(&self) -> &[OsString] {
        &self.listen
    }

    /// Where the server is, once it is up; `printed` is what it has written to
    /// its standard error so far.
    pub(crate) fn upstream(&self, printed: &str) -> Option<Upstream> {
        match &self.kind {
            #[cfg(unix)]
            Kind::Socket(path) => {
                use std::os::unix::fs::FileTypeExt;
                std::fs::metadata(path)
                    .is_ok_and(|found| found.file_type().is_socket())
                    .then(|| Upstream {
                        target: Target::Unix(path.clone()),
                        authorization: None,
                    })
            }
            Kind::Loopback { authorization } => listening_on(printed).map(|address| Upstream {
                target: Target::Tcp(address),
                authorization: Some(authorization.clone()),
            }),
        }
    }
}

/// The address in the first `listening on: ws://127.0.0.1:PORT` of `printed`.
/// The port is complete once something follows its digits: a read may end
/// between two of them, and the server writes more lines after this one.
fn listening_on(printed: &str) -> Option<SocketAddr> {
    const MARK: &str = "listening on:";
    printed.match_indices(MARK).find_map(|(at, _)| {
        let rest = js::trim_start(&printed[at + MARK.len()..]).strip_prefix("ws://127.0.0.1:")?;
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == rest.len() {
            return None;
        }
        let port: u16 = rest[..digits].parse().ok()?;
        Some(SocketAddr::from(([127, 0, 0, 1], port)))
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The user's temporary folder as Node found it: `TMPDIR`, `TMP`, `TEMP`, else `/tmp`.
#[cfg(unix)]
fn temp_dir(env: &Env) -> PathBuf {
    ["TMPDIR", "TMP", "TEMP"]
        .iter()
        .find_map(|name| env.path(name))
        .map_or_else(|| PathBuf::from("/tmp"), Path::to_path_buf)
}

/// Whether a socket at `<root>/codex-XXXXXX/native.sock` fits `sun_path`.
#[cfg(unix)]
fn socket_fits(root: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    root.join("codex-XXXXXX")
        .join("native.sock")
        .as_os_str()
        .as_bytes()
        .len()
        < SUN_PATH
}

/// The socket's private folder: under the ConsensFlow home, or, when that
/// path is too long for a Unix socket (a home deep in a temporary tree), under
/// the user's own temporary folder. The socket is a runtime endpoint, not
/// state: it is removed with the session.
#[cfg(unix)]
pub(crate) fn create_socket_directory(env: &Env) -> Result<PathBuf, EndpointError> {
    use std::fs::{DirBuilder, Permissions};
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let folder = |path: &Path, cause| EndpointError::Folder {
        path: path.to_path_buf(),
        cause,
    };
    let homes = cf_base::home::config_root(env)
        .map(|home| home.join("tmp"))
        .into_iter()
        .chain([temp_dir(env).join("consensflow")]);
    let root = homes
        .into_iter()
        .find(|candidate| socket_fits(candidate))
        .ok_or(EndpointError::TooLong)?;
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&root)
        .map_err(|cause| folder(&root, cause))?;
    // `mkdtemp`: a name no other window has, made private as it is made.
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    loop {
        let mut random = [0_u8; 6];
        getrandom::fill(&mut random).map_err(EndpointError::Random)?;
        let name: String = random
            .iter()
            .map(|byte| char::from(ALPHABET[usize::from(*byte) % ALPHABET.len()]))
            .collect();
        let directory = root.join(format!("codex-{name}"));
        match DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => {
                // The umask may have taken bits from the mode; it never adds one.
                std::fs::set_permissions(&directory, Permissions::from_mode(0o700))
                    .map_err(|cause| folder(&directory, cause))?;
                return Ok(directory);
            }
            Err(cause) if cause.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(cause) => return Err(folder(&directory, cause)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn keeps_native_codex_sockets_private_and_inside_consensflow_home() {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::PermissionsExt;

        // A short root: the temporary folder of a Mac is long enough to
        // push the first path past what a socket takes.
        let root = tempfile::Builder::new()
            .prefix("cf-socket-home-")
            .tempdir_in("/tmp")
            .unwrap();
        let home = root.path().join(".consensflow");
        let env = Env::from_vars([("CONSENSFLOW_HOME", home.as_path())]);
        let directory = create_socket_directory(&env).unwrap();
        assert!(directory.starts_with(home.join("tmp")), "{directory:?}");
        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(directory.join("native.sock").as_os_str().as_bytes().len() < SUN_PATH);

        // A home too deep for a Unix socket (a temporary tree) falls back to the
        // user's own temporary folder, still private; nowhere short and it fails.
        let deep = root.path().join("x".repeat(110));
        let env = Env::from_vars([
            ("CONSENSFLOW_HOME", deep.as_path()),
            ("TMPDIR", root.path()),
        ]);
        let fallback = create_socket_directory(&env).unwrap();
        assert!(
            fallback.starts_with(root.path().join("consensflow")),
            "{fallback:?}"
        );
        assert_eq!(
            std::fs::metadata(&fallback).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(fallback.join("native.sock").as_os_str().as_bytes().len() < SUN_PATH);
        let env = Env::from_vars([
            ("CONSENSFLOW_HOME", deep.as_path()),
            ("TMPDIR", deep.as_path()),
        ]);
        let refused = create_socket_directory(&env).unwrap_err().to_string();
        assert!(refused.contains("socket path") && refused.contains("too long"));
    }

    #[cfg(unix)]
    #[test]
    fn each_window_gets_a_folder_of_its_own_and_the_server_is_up_once_its_socket_is_there() {
        use std::os::unix::net::UnixListener;

        let root = tempfile::Builder::new()
            .prefix("cf-endpoint-")
            .tempdir_in("/tmp")
            .unwrap();
        let env = Env::from_vars([("CONSENSFLOW_HOME", root.path())]);
        let first = Endpoint::socket(&env).unwrap();
        let second = Endpoint::socket(&env).unwrap();
        let directory = first.directory().unwrap();
        assert_ne!(directory, second.directory().unwrap());
        let socket = directory.join("native.sock");
        let mut listen = OsString::from("unix://");
        listen.push(&socket);
        assert_eq!(first.listen(), [OsString::from("--listen"), listen]);

        assert_eq!(first.upstream(""), None, "nothing is there yet");
        std::fs::write(&socket, "").unwrap();
        assert_eq!(first.upstream(""), None, "a file is not a socket");
        std::fs::remove_file(&socket).unwrap();
        let _listener = UnixListener::bind(&socket).unwrap();
        assert_eq!(
            first.upstream("anything it printed"),
            Some(Upstream {
                target: Target::Unix(socket),
                authorization: None
            })
        );
    }

    #[test]
    fn listens_on_loopback_for_the_windows_own_token_and_is_up_once_it_says_where() {
        let server = Endpoint::loopback().unwrap();
        assert_eq!(server.directory(), None, "no Unix socket folder");
        let [flag, address, auth, scheme, hash_flag, hash] = server.listen() else {
            panic!("{:?}", server.listen());
        };
        assert_eq!(
            [flag, address, auth, scheme, hash_flag].map(|arg| arg.to_str().unwrap()),
            [
                "--listen",
                "ws://127.0.0.1:0",
                "--ws-auth",
                "capability-token",
                "--ws-token-sha256"
            ]
        );
        let token = authorization(&server).strip_prefix("Bearer ").unwrap();
        // The hash is of the token's own text, hex as it is, not of the bytes it stands for.
        assert_eq!(token.len(), 64);
        assert_eq!(hash.to_str().unwrap(), hex(&Sha256::digest(token)));
        let raw: Vec<u8> = (0..32)
            .map(|at| u8::from_str_radix(&token[at * 2..at * 2 + 2], 16).unwrap())
            .collect();
        assert_ne!(hash.to_str().unwrap(), hex(&Sha256::digest(raw)));

        assert_eq!(
            server.upstream("codex app-server (WebSockets)\n"),
            None,
            "not before it says where"
        );
        assert_eq!(
            server
                .upstream("codex app-server (WebSockets)\n  listening on: ws://127.0.0.1:53111\n"),
            Some(Upstream {
                target: Target::Tcp(SocketAddr::from(([127, 0, 0, 1], 53111))),
                authorization: Some(format!("Bearer {token}")),
            })
        );
        let other = Endpoint::loopback().unwrap();
        assert_ne!(
            authorization(&other),
            authorization(&server),
            "each window its own"
        );
    }

    /// The `authorization` a loopback endpoint's connections carry.
    fn authorization(endpoint: &Endpoint) -> &str {
        match &endpoint.kind {
            Kind::Loopback { authorization } => authorization,
            #[cfg(unix)]
            Kind::Socket(_) => panic!("a socket endpoint carries no authorization"),
        }
    }

    #[test]
    fn knows_a_line_split_across_reads_once_the_pieces_are_together() {
        let line = "listening on: ws://127.0.0.1:53111\n";
        let mut tail = crate::tail::Tail::default();
        let server = Endpoint::loopback().unwrap();
        for piece in [&line[..9], &line[9..22], &line[22..30], &line[30..]] {
            assert_eq!(server.upstream(tail.text()), None, "before {piece:?}");
            tail.push(piece.as_bytes());
        }
        assert!(server.upstream(tail.text()).is_some());
    }

    #[test]
    fn reads_the_first_line_that_names_a_loopback_port() {
        let address = |printed| listening_on(printed).map(|address| address.port());
        assert_eq!(address("listening on: ws://127.0.0.1:1\n"), Some(1));
        assert_eq!(
            address("listening on:\n\tws://127.0.0.1:65535 "),
            Some(65535)
        );
        assert_eq!(
            address("listening on: http://x\nlistening on: ws://127.0.0.1:9\nlistening on: ws://127.0.0.1:8\n"),
            Some(9)
        );
        // Its digits may not all be there yet: it is read once something follows them.
        assert_eq!(address("listening on: ws://127.0.0.1:5"), None);
        assert_eq!(address("listening on: ws://127.0.0.1:"), None);
        assert_eq!(address("listening on: ws://127.0.0.1:99999\n"), None);
        assert_eq!(address("listening on: ws://localhost:80"), None);
        assert_eq!(address("listening ws://127.0.0.1:80"), None);
    }
}
