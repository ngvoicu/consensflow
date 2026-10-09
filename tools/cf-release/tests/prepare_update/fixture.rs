//! What the tests of `prepare-update` are made from: an app as the release has it
//! (the executable, the `cf` a window runs, the `Info.plist`), written as a
//! folder and as the `.app.tar.gz` installed apps unpack, a checkout whose
//! sources say a version, a signature of the shape the signer writes, and notes.
//! An app is a list of paths, so a test changes the one it means to before it is
//! written as an archive, and the folder stays the release's.

use std::ffi::OsString;
use std::fs::{self, File};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cf_base::env::Env;
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::{Builder, EntryType, Header};
use tempfile::TempDir;

pub const VERSION: &str = "3.0.0-alpha.99";
pub const STABLE: &str = "3.0.0";
pub const DATE: &str = "2026-09-09T12:00:00Z";
pub const ROOT: &str = "ConsensFlow.app";
pub const INFO_PLIST: &str = "ConsensFlow.app/Contents/Info.plist";
pub const EXECUTABLE: &str = "ConsensFlow.app/Contents/MacOS/ConsensFlow";
pub const CF: &str = "ConsensFlow.app/Contents/Resources/cli/bin/cf";

/// A path of an app: what it is, its mode and what it holds.
#[derive(Clone)]
pub struct Item {
    pub name: String,
    pub kind: EntryType,
    pub mode: u32,
    pub data: Vec<u8>,
    /// Where a link points.
    pub link: String,
}

impl Item {
    pub fn dir(name: &str) -> Self {
        Self::of(name, EntryType::Directory, 0o755, b"")
    }

    pub fn file(name: &str, mode: u32, data: &[u8]) -> Self {
        Self::of(name, EntryType::Regular, mode, data)
    }

    pub fn link(name: &str, kind: EntryType, target: &str) -> Self {
        Self {
            link: target.to_string(),
            ..Self::of(name, kind, 0o755, b"")
        }
    }

    /// A path of a kind an app has none of: a pipe, a device.
    pub fn special(name: &str, kind: EntryType) -> Self {
        Self::of(name, kind, 0o644, b"")
    }

    fn of(name: &str, kind: EntryType, mode: u32, data: &[u8]) -> Self {
        Self {
            name: name.to_string(),
            kind,
            mode,
            data: data.to_vec(),
            link: String::new(),
        }
    }
}

/// An app, path by path.
#[derive(Clone)]
pub struct App {
    pub items: Vec<Item>,
}

impl App {
    /// The app a release has: a program, and the `cf` that says `cf_version` when
    /// asked for its version, and an `Info.plist` that says `version`. Nothing of
    /// Node's travels in it.
    pub fn new(version: &str, cf_version: &str) -> Self {
        let plist = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
             <key>CFBundleIdentifier</key><string>dev.ngvoicu.consensflow</string>\
             <key>CFBundleExecutable</key><string>ConsensFlow</string>\
             <key>CFBundleShortVersionString</key><string>{version}</string>\
             <key>CFBundleVersion</key><string>{version}</string></dict></plist>\n"
        );
        let script = format!("#!/bin/sh\necho {cf_version}\n");
        Self {
            items: vec![
                Item::dir(ROOT),
                Item::dir("ConsensFlow.app/Contents"),
                Item::file(INFO_PLIST, 0o644, plist.as_bytes()),
                Item::dir("ConsensFlow.app/Contents/MacOS"),
                Item::file(EXECUTABLE, 0o755, b"binary\n"),
                Item::dir("ConsensFlow.app/Contents/Resources"),
                Item::dir("ConsensFlow.app/Contents/Resources/cli"),
                Item::dir("ConsensFlow.app/Contents/Resources/cli/bin"),
                Item::file(CF, 0o755, script.as_bytes()),
            ],
        }
    }

    /// The path called `name`.
    pub fn item(&mut self, name: &str) -> &mut Item {
        self.items
            .iter_mut()
            .find(|item| item.name == name)
            .unwrap_or_else(|| panic!("the app has no {name}"))
    }

    pub fn push(&mut self, item: Item) -> &mut Self {
        self.items.push(item);
        self
    }

    pub fn remove(&mut self, name: &str) -> &mut Self {
        self.items.retain(|item| item.name != name);
        self
    }

    /// Writes the app as a folder `ConsensFlow.app` in `parent`, each path with
    /// the mode it has here (not the process's mask of it).
    pub fn write_folder(&self, parent: &Path) -> PathBuf {
        for item in &self.items {
            let path = parent.join(&item.name);
            match item.kind {
                EntryType::Directory => fs::create_dir_all(&path).unwrap(),
                EntryType::Regular => fs::write(&path, &item.data).unwrap(),
                EntryType::Symlink => symlink(&item.link, &path).unwrap(),
                other => panic!("{other:?} is no path of a folder here"),
            }
        }
        // Modes last: a folder without write permission could take no more paths.
        for item in self
            .items
            .iter()
            .filter(|item| item.kind != EntryType::Symlink)
        {
            let permissions = fs::Permissions::from_mode(item.mode);
            fs::set_permissions(parent.join(&item.name), permissions).unwrap();
        }
        parent.join(ROOT)
    }

    /// Writes the app as the archive at `path`, plain ustar, in the order of the
    /// paths. A name is written as it is given: `..`, a leading `/`, anything.
    pub fn write_archive(&self, path: &Path) {
        let mut builder = Builder::new(GzEncoder::new(
            File::create(path).unwrap(),
            Compression::default(),
        ));
        for item in &self.items {
            let mut header = Header::new_ustar();
            header.set_entry_type(item.kind);
            header.set_mode(item.mode);
            header.set_size(item.data.len() as u64);
            header.set_mtime(0);
            // A folder's name ends in a slash, as `tar` writes it.
            let slash = if item.kind == EntryType::Directory {
                "/"
            } else {
                ""
            };
            let name = format!("{}{slash}", item.name);
            header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
            if !item.link.is_empty() {
                header.set_link_name(&item.link).unwrap();
            }
            header.set_cksum();
            builder.append(&header, item.data.as_slice()).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
    }
}

/// What to change in a release that is otherwise right.
#[derive(Default)]
pub struct Spec<'a> {
    /// The version of the build; `VERSION` if none.
    pub version: Option<&'a str>,
    /// What the bundle's `cf` says its version is, if not the build's.
    pub cf_version: Option<&'a str>,
    /// What the sources say, if not the build's.
    pub repo_version: Option<&'a str>,
    /// The version of the app in the archive, if not the build's.
    pub archive_version: Option<&'a str>,
    /// More paths, in the folder and in the archive both.
    pub extra: Vec<Item>,
}

/// The words a signer writes, with the file name its trusted comment gives.
pub fn signature_for(archive: &str) -> String {
    let lines = [
        "untrusted comment: signature from tauri secret key",
        "RUTKbp37uB4mh3QuqjizE5qwzEQRTiusW3qwiDtoFuhEUaQ+cRCU+VdP/Ee2JVCrCWCRwJw6e64tAmU8FXPeeubR6Y3+foAANgU=",
        &format!("trusted comment: timestamp:1791543247\tfile:{archive}"),
        "ZsYY/Z32Np8PS9zvML7LiCnnfxQRC/vGcV5dA8drvzLItPQf1kDn0m0VcO+Q0j9Lhsx6h9sllAP3jpTF6vfbAQ==",
    ];
    STANDARD.encode(format!("{}\n", lines.join("\n")))
}

/// A release as the workflow has it when it makes the feed's entry: the sources
/// at a checkout, the app it built and the archive made from it, its signature,
/// the notes, and where the entry goes. All in a folder of its own.
pub struct Release {
    pub dir: TempDir,
    pub repo: PathBuf,
    pub bundle: PathBuf,
    pub archive: PathBuf,
    pub signature: PathBuf,
    pub notes: PathBuf,
    pub output: PathBuf,
    /// The app as the archive holds it.
    pub archived: App,
}

impl Release {
    pub fn new() -> Self {
        Self::of(&Spec::default())
    }

    pub fn of(spec: &Spec) -> Self {
        let version = spec.version.unwrap_or(VERSION);
        let archived_version = spec.archive_version.unwrap_or(version);
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        write_sources(&repo, spec.repo_version.unwrap_or(version));

        let staged = dir.path().join("staged");
        fs::create_dir(&staged).unwrap();
        let mut app = App::new(version, spec.cf_version.unwrap_or(version));
        app.items.extend(spec.extra.iter().cloned());
        let bundle = app.write_folder(&staged);

        let name = format!("ConsensFlow-{version}_aarch64.app.tar.gz");
        let mut archived = App::new(archived_version, archived_version);
        archived.items.extend(spec.extra.iter().cloned());
        let release = Self {
            archive: dir.path().join(&name),
            signature: dir.path().join(format!("{name}.sig")),
            notes: dir.path().join("notes.txt"),
            output: dir.path().join("latest.json"),
            archived,
            repo,
            bundle,
            dir,
        };
        release.archived.write_archive(&release.archive);
        fs::write(&release.signature, signature_for(&name)).unwrap();
        fs::write(&release.notes, "Alpha 99 fixes delivery races.\n").unwrap();
        release
    }

    /// Writes the archive again from the app as it is now.
    pub fn archive_again(&self) {
        self.archived.write_archive(&self.archive);
    }

    /// The name the archive has: the asset of the feed's URL.
    pub fn asset(&self) -> String {
        self.archive
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    /// Every flag with the value that makes a release, `channel` and `date` among
    /// them, in the order the script took them.
    pub fn flags(&self) -> Vec<(String, String)> {
        let path = |path: &Path| path.display().to_string();
        [
            ("bundle", path(&self.bundle)),
            ("archive", path(&self.archive)),
            ("signature", path(&self.signature)),
            ("notes", path(&self.notes)),
            ("output", path(&self.output)),
            ("channel", "alpha".to_string()),
            ("date", DATE.to_string()),
            ("repo", path(&self.repo)),
        ]
        .map(|(name, value)| (name.to_string(), value))
        .into()
    }

    /// `prepare-update` with the flags of a release, those in `changes` given
    /// the value they carry instead (and a flag the release has none of added).
    pub fn run(&self, changes: &[(&str, &str)]) -> Ran {
        let mut flags = self.flags();
        for (name, value) in changes {
            match flags.iter_mut().find(|(flag, _)| flag == name) {
                Some((_, current)) => *current = (*value).to_string(),
                None => flags.push(((*name).to_string(), (*value).to_string())),
            }
        }
        run(&words(&flags))
    }

    /// `prepare-update` with every flag of a release but `left_out`.
    pub fn run_without(&self, left_out: &str) -> Ran {
        let mut flags = self.flags();
        flags.retain(|(name, _)| name != left_out);
        run(&words(&flags))
    }

    /// The entry the last run wrote.
    pub fn entry(&self) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(&self.output).unwrap()).unwrap()
    }
}

/// The sources of a checkout that each say `version`.
pub fn write_sources(repo: &Path, version: &str) {
    fs::create_dir_all(repo.join("app").join("src-tauri")).unwrap();
    fs::write(
        repo.join("package.json"),
        format!(r#"{{"version":"{version}"}}"#),
    )
    .unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        format!(
            "[workspace]\nmembers = [\"app/src-tauri\"]\n\n[workspace.package]\nversion = \"{version}\"\n"
        ),
    )
    .unwrap();
    fs::write(
        repo.join("app").join("src-tauri").join("tauri.conf.json"),
        format!(r#"{{"version":"{version}"}}"#),
    )
    .unwrap();
}

/// `prepare-update` followed by `--name value` for each flag.
pub fn words(flags: &[(String, String)]) -> Vec<OsString> {
    let mut words = vec![OsString::from("prepare-update")];
    for (name, value) in flags {
        words.push(format!("--{name}").into());
        words.push(value.into());
    }
    words
}

/// The refusal of the check of the archive and the bundle, in its words.
pub fn unsound(said: &str) -> String {
    format!("archive safety/content check failed: {said}\n")
}

/// The refusal that the archive is not the bundle, and at which path.
pub fn differs(path: &str, how: &str) -> String {
    unsound(&format!(
        "the archive content manifest does not match the supplied bundle: {path} {how}"
    ))
}

/// What a run of the command left.
pub struct Ran {
    pub status: u8,
    pub stdout: String,
    pub stderr: String,
}

impl Ran {
    /// It finished, and said nothing.
    pub fn finished(&self) {
        assert_eq!(self.status, 0, "{}", self.stderr);
        assert_eq!((self.stdout.as_str(), self.stderr.as_str()), ("", ""));
    }

    /// It could not: with status 1, in these words after its own name.
    pub fn refused(&self, said: &str) {
        assert_eq!(
            (self.status, self.stdout.as_str()),
            (1, ""),
            "{}",
            self.stderr
        );
        let line = self.stderr.strip_prefix("cf-release prepare-update: ");
        assert!(
            line.is_some_and(|line| line.starts_with(said)),
            "wanted {said:?}, got {:?}",
            self.stderr
        );
        assert_eq!(self.stderr.lines().count(), 1, "{}", self.stderr);
    }

    /// The words were not ones it takes: status 2, with these words.
    pub fn misused(&self, said: &str) {
        assert_eq!(
            (self.status, self.stdout.as_str()),
            (2, ""),
            "{}",
            self.stderr
        );
        let line = self.stderr.strip_prefix("cf-release prepare-update: ");
        assert!(
            line.is_some_and(|line| line.starts_with(said)),
            "wanted {said:?}, got {:?}",
            self.stderr
        );
    }
}

/// Runs the command line `words` as the binary does, with an empty environment.
pub fn run(words: &[OsString]) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = cf_release::run(&Env::default(), words, &mut out, &mut err);
    Ran {
        status,
        stdout: String::from_utf8(out).unwrap(),
        stderr: String::from_utf8(err).unwrap(),
    }
}
