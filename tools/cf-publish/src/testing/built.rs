//! What the jobs built, and what GitHub then holds of it: the files of a
//! release in a folder of their own, the way `npm test` made them, in the
//! system's temporary folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::digest::sha256_hex;

/// A folder that is gone when this is dropped.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// A new empty folder, named for `label`.
    pub fn new(label: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "cf-publish-{label}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            if fs::create_dir(&path).is_ok() {
                return Self { path };
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Files by name or path, in the order they were given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Files(Vec<(String, Vec<u8>)>);

impl Files {
    /// These files with `data` at `name`, which replaces what was there.
    pub fn with(mut self, name: &str, data: impl AsRef<[u8]>) -> Self {
        self.set(name, data);
        self
    }

    /// `name` set to `data`.
    pub fn set(&mut self, name: &str, data: impl AsRef<[u8]>) {
        let data = data.as_ref().to_vec();
        match self.0.iter_mut().find(|(have, _)| have == name) {
            Some((_, held)) => *held = data,
            None => self.0.push((name.to_string(), data)),
        }
    }

    /// These files but `name`.
    pub fn without(mut self, name: &str) -> Self {
        self.0.retain(|(have, _)| have != name);
        self
    }

    /// The bytes of `name`.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.0
            .iter()
            .find(|(have, _)| have == name)
            .map(|(_, data)| data.as_slice())
    }

    /// The text of `name`; it is there, and text.
    pub fn text(&self, name: &str) -> String {
        String::from_utf8_lossy(self.get(name).expect("the file is there")).into_owned()
    }

    /// The names, in order.
    pub fn names(&self) -> Vec<&str> {
        self.0.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// Each name with its bytes.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.0
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_slice()))
    }
}

/// The path a release's file is downloaded from.
pub fn download(version: &str, name: &str) -> String {
    format!("/v{version}/{name}")
}

/// The name of the Mac archive of `version`.
pub fn archive_name(version: &str) -> String {
    format!("ConsensFlow_{version}_aarch64.app.tar.gz")
}

/// The name of the Windows installer of `version`.
pub fn installer_name(version: &str) -> String {
    format!("ConsensFlow_{version}_x64-setup.exe")
}

/// The name of the portable Windows exe of `version`.
pub fn portable_name(version: &str) -> String {
    format!("ConsensFlow_{version}_x64-portable.exe")
}

/// A release's latest.json, naming its archive where the release publishes it
/// under `base`.
pub fn latest_json(base: &str, version: &str, notes: &str) -> String {
    format!(
        "{{\"version\":\"{version}\",\"notes\":\"{notes}\",\"pub_date\":\"2026-10-06T12:00:00Z\",\"platforms\":{{\"darwin-aarch64\":{{\"url\":\"{base}/v{version}/ConsensFlow_{version}_aarch64.app.tar.gz\",\"signature\":\"signed\"}}}}}}\n"
    )
}

/// `latest` with the address of the archive it names replaced by `url`.
pub fn with_archive_url(latest: &str, url: &str) -> String {
    let marker = "\"url\":\"";
    let start = latest
        .find(marker)
        .expect("the latest.json names an archive")
        + marker.len();
    let end = start + latest[start..].find('"').expect("the address ends");
    format!("{}{url}{}", &latest[..start], &latest[end..])
}

/// What a built release's folder holds: a path in it mapped to its bytes.
pub fn built_files(base: &str, version: &str) -> Files {
    let mac = format!("ConsensFlow_{version}_aarch64");
    let text = |what: &str| format!("{what} of {version}\n");
    Files::default()
        .with("notes.txt", format!("ConsensFlow {version}, the notes\n"))
        .with(&format!("{mac}.dmg"), text("dmg"))
        .with(&format!("{mac}.app.tar.gz"), text("archive"))
        .with(&format!("{mac}.app.tar.gz.sig"), text("signature"))
        .with("latest.json", latest_json(base, version, "notes"))
        .with(
            &format!("nsis/ConsensFlow_{version}_x64-setup.exe"),
            text("installer"),
        )
        .with(
            &format!("portable/ConsensFlow_{version}_x64-portable.exe"),
            text("portable"),
        )
}

/// A built release's files as the published release holds them, by file name,
/// with the SHA256SUMS the publisher adds (`<sha256>  <name>`, as sha256sum
/// writes them): what [`built_files`] made, less the notes.
pub fn published_assets(files: &Files) -> Files {
    let mut published = Files::default();
    for (path, data) in files.iter() {
        if path != "notes.txt" {
            published.set(path.rsplit('/').next().unwrap_or(path), data);
        }
    }
    let sums: String = published
        .iter()
        .map(|(name, data)| format!("{}  {name}\n", sha256_hex(data)))
        .collect();
    published.with("SHA256SUMS", sums)
}

/// The bytes of a ustar archive of the files `names` (paths in it, each
/// holding its own name), made as the release makes its own: what `tar -t`
/// lists is what the old apps' updater is held to.
pub fn archive_of(names: &[&str]) -> Vec<u8> {
    let dir = TempDir::new("archive");
    let tree = dir.path().join("tree");
    for name in names {
        let target = tree.join(name);
        fs::create_dir_all(target.parent().expect("a file has a folder")).expect("a folder");
        fs::write(target, name).expect("a file");
    }
    let target = dir.path().join("archive.tar.gz");
    // The test starts what it archives with.
    #[allow(clippy::disallowed_methods)]
    let made = Command::new("tar")
        .args(["--format", "ustar", "-czf"])
        .arg(&target)
        .arg("-C")
        .arg(&tree)
        .arg("ConsensFlow.app")
        .env("COPYFILE_DISABLE", "1")
        .output()
        .expect("tar runs");
    assert!(
        made.status.success(),
        "tar: {}",
        String::from_utf8_lossy(&made.stderr)
    );
    fs::read(target).expect("the archive")
}

/// A folder of `files` (a path in it mapped to its bytes).
pub fn folder_of(files: &Files) -> TempDir {
    let dir = TempDir::new("release");
    for (path, data) in files.iter() {
        let target = path
            .split('/')
            .fold(dir.path().to_path_buf(), |at, part| at.join(part));
        fs::create_dir_all(target.parent().expect("a file has a folder")).expect("a folder");
        fs::write(target, data).expect("a file");
    }
    dir
}
