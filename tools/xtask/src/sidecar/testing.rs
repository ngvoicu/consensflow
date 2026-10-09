//! What the tests of the sidecar commands share: a checkout in a temporary
//! folder, a stand-in for the system the steps run on, a package like the
//! console host's, and a way to list what a step left.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::{Platform, System};
use crate::context::Context;
use crate::process::{self, Invocation};

/// Where the console host's package keeps `conpty.dll` and `OpenConsole.exe` for
/// x64, written out here and not read from the code that takes them from it.
pub(super) const DLL_INSIDE: &str = "runtimes/win-x64/native/conpty.dll";
pub(super) const EXE_INSIDE: &str = "build/native/runtimes/x64/OpenConsole.exe";

/// A checkout in a temporary folder, which is gone with the first of the two.
pub(super) fn checkout() -> (tempfile::TempDir, Context) {
    let dir = tempfile::tempdir().unwrap();
    let context = Context {
        root: dir.path().to_path_buf(),
        env: Env::default(),
    };
    (dir, context)
}

/// Writes `text` to `path`, the folders above it made.
pub(super) fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// What is read from `path`, as text.
pub(super) fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

/// A zip of these files, deflated as the packages on nuget.org are.
pub(super) fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// A package like the console host's: its two files for x64, the same two for
/// another processor, and a file the app has no use for.
pub(super) fn console_host_package() -> Vec<u8> {
    zip_of(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("runtimes/win-arm64/native/conpty.dll", b"the dll for arm64"),
        (DLL_INSIDE, b"the console host's dll"),
        (EXE_INSIDE, b"the console host's exe"),
    ])
}

/// The SHA-256 of `bytes` in hex, written out here and not by the code it checks.
pub(super) fn sha256_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Every file under `dir`, as `/`-separated paths from it, in order: the files
/// a step left, and none it did not.
pub(super) fn files_under(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, from: &Path, found: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, from, found);
            } else {
                let relative = path.strip_prefix(from).unwrap();
                let parts: Vec<_> = relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                found.push(parts.join("/"));
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, dir, &mut found);
    found.sort();
    found
}

/// The system the steps run on, as a test says it is: which one, the time, what
/// the programs answer, and which files cannot be deleted.
pub(super) struct Fake {
    pub platform: Platform,
    /// The time of day, which never moves.
    pub now: i64,
    /// Every program a step started, in order.
    pub ran: Vec<Invocation>,
    /// Programs that are not on the PATH.
    pub missing: Vec<OsString>,
    /// The status `cargo` ends with, and what it leaves as the built `cf` when
    /// that is 0: nothing, if `built` is none.
    pub cargo_status: i32,
    pub built: Option<Vec<u8>>,
    /// The status `codesign` ends with. Signing a file adds to it, as a
    /// signature does, so that a test can tell a signed copy from the one it came from.
    pub codesign_status: i32,
    /// The status `curl` ends with, and what it writes to the file it is told
    /// to when that is 0.
    pub curl_status: i32,
    pub package: Vec<u8>,
    /// Files that cannot be deleted, as Windows cannot delete one that runs.
    pub undeletable: Vec<PathBuf>,
}

impl Fake {
    /// A system of the kind `platform` names, whose programs all succeed: the
    /// build leaves a `cf` that says it is new, and nothing is fetched.
    pub fn on(platform: Platform) -> Self {
        Self {
            platform,
            now: 1_760_000_000_000,
            ran: Vec::new(),
            missing: Vec::new(),
            cargo_status: 0,
            built: Some(b"the new cf".to_vec()),
            codesign_status: 0,
            curl_status: 0,
            package: Vec::new(),
            undeletable: Vec::new(),
        }
    }

    /// The command lines of the programs started, as a person reads them.
    pub fn lines(&self) -> Vec<String> {
        self.ran.iter().map(Invocation::display).collect()
    }

    fn cargo(&self, invocation: &Invocation) {
        // Builds only in a checkout a test made: a folder it is run from would be written to.
        assert!(
            invocation.cwd.is_absolute(),
            "the stand-in builds in {}, which is no checkout of a test's",
            invocation.cwd.display()
        );
        if self.cargo_status != 0 {
            return;
        }
        if let Some(bytes) = &self.built {
            let release = ["app", "src-tauri", "target", "release"]
                .iter()
                .fold(invocation.cwd.clone(), |path, part| path.join(part));
            fs::create_dir_all(&release).unwrap();
            fs::write(release.join(self.platform.cf()), bytes).unwrap();
        }
    }

    fn codesign(&self, invocation: &Invocation) {
        if self.codesign_status != 0 {
            return;
        }
        let signed = invocation
            .args
            .last()
            .expect("codesign was not told which file");
        let mut bytes = fs::read(signed).unwrap();
        bytes.extend_from_slice(b" signed");
        fs::write(signed, bytes).unwrap();
    }

    fn curl(&self, invocation: &Invocation) {
        if self.curl_status != 0 {
            return;
        }
        let told = invocation
            .args
            .iter()
            .position(|word| word == "-o")
            .expect("curl was not told where to write");
        fs::write(&invocation.args[told + 1], &self.package).unwrap();
    }
}

impl System for Fake {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn run(&mut self, invocation: &Invocation) -> Result<i32, process::Failure> {
        self.ran.push(invocation.clone());
        let program = invocation.program.to_string_lossy().into_owned();
        if self.missing.contains(&invocation.program) {
            return Err(process::Failure::NotFound { program });
        }
        match program.as_str() {
            "cargo" => {
                self.cargo(invocation);
                Ok(self.cargo_status)
            }
            "codesign" => {
                self.codesign(invocation);
                Ok(self.codesign_status)
            }
            "curl" => {
                self.curl(invocation);
                Ok(self.curl_status)
            }
            other => panic!("a step started {other}, which the stand-in has no answer for"),
        }
    }

    fn now_ms(&mut self) -> i64 {
        self.now
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        // A file that is not there is not refused: there is nothing to run from it.
        if path.exists() && self.undeletable.iter().any(|stuck| stuck == path) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        fs::remove_file(path)
    }
}
