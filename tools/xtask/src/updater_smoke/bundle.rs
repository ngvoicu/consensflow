//! A built app as the updater smoke looks at it: what the installed app's own
//! check accepts of a bundle (`validate_bundle`, app/src-tauri/src/update_install.rs),
//! its code signature, its bytes, and the archive an update is served as. The
//! flip release's bundle still ships Node, `cf.mjs`, `src` and `hosts`, and the
//! release after it does not: this takes either, and says which it is.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use sha2::{Digest, Sha256};

use super::{files, Error, Result};
use crate::process::{self, Invocation};

/// The identity every bundle of ConsensFlow has.
pub const IDENTITY: &str = "dev.ngvoicu.consensflow";

/// What a bundle of the releases before the deletion keeps of Node's, relative
/// to it: the sidecar, and the CLI's sources.
const NODE_FILES: [&[&str]; 4] = [
    &["Contents", "MacOS", "node"],
    &["Contents", "Resources", "cli", "bin", "cf.mjs"],
    &["Contents", "Resources", "cli", "src"],
    &["Contents", "Resources", "cli", "hosts"],
];

/// A path under `base`, written part by part.
pub fn under(base: &Path, parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

/// The window's `cf` in a bundle: the command a window runs, and what the app starts as its daemon.
pub fn cf_of(app: &Path) -> PathBuf {
    under(app, &["Contents", "Resources", "cli", "bin", "cf"])
}

/// Runs `program` with `args` to its end and answers what it printed; a status
/// other than 0 is an error that says what was asked and what the program said.
pub fn run(program: &str, args: &[OsString], env: &Env) -> Result<String> {
    let invocation = Invocation::new(program, Path::new(".")).args(args.iter().cloned());
    let ran = process::capture(&invocation, env)?;
    if ran.code != 0 {
        let said = ran.stderr.trim();
        let said = if said.is_empty() {
            format!("it ended with status {}", ran.code)
        } else {
            said.to_string()
        };
        let asked: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
        return Err(Error::new(format!(
            "{program} {} failed: {said}",
            asked.join(" ")
        )));
    }
    Ok(ran.stdout)
}

/// A field of the bundle's `Info.plist`.
pub fn plist_value(app: &Path, field: &str) -> Result<String> {
    let file = under(app, &["Contents", "Info.plist"]);
    let plist = plist::Value::from_file(&file)
        .map_err(|cause| Error::new(format!("could not read {}: {cause}", file.display())))?;
    plist
        .as_dictionary()
        .and_then(|fields| fields.get(field))
        .and_then(plist::Value::as_string)
        .map(str::to_string)
        .ok_or_else(|| Error::new(format!("{} has no text {field}", file.display())))
}

/// What a bundle is, as the smoke reads it once and keeps it.
#[derive(Debug, Clone)]
pub struct BundleInfo {
    pub app: PathBuf,
    pub label: String,
    /// The app's own executable.
    pub binary: PathBuf,
    /// The command a window runs.
    pub cf: PathBuf,
    pub version: String,
    /// Whether it holds Node's files: it is of a release that still ships them.
    pub node: bool,
    /// The version its CLI's manifest says, where it has one.
    pub cli_version: Option<String>,
}

/// The app at `app` as the installed app's check takes it: this app's identity,
/// one version in both of the plist's fields, the app's executable, and `cf`, the
/// command a window runs. Whatever Node's files it holds must be all of them: a
/// bundle with a runtime and no `cf.mjs` runs neither implementation of the
/// way back. `cli/package.json` is read where it is, and need not be.
pub fn inspect_bundle(app: &Path, label: &str) -> Result<BundleInfo> {
    let plist = under(app, &["Contents", "Info.plist"]);
    ensure!(
        plist.exists(),
        "{label} has no Contents/Info.plist: {}",
        app.display()
    );
    let identifier = plist_value(app, "CFBundleIdentifier")?;
    ensure!(
        identifier == IDENTITY,
        "{label} is {identifier}, not {IDENTITY}"
    );
    let version = plist_value(app, "CFBundleShortVersionString")?;
    ensure!(
        plist_value(app, "CFBundleVersion")? == version,
        "{label}: the plist's two versions differ"
    );
    let executable = plist_value(app, "CFBundleExecutable")?;
    let binary = under(app, &["Contents", "MacOS", &executable]);
    let cf = cf_of(app);
    for (what, path) in [("native executable", &binary), ("window's cf", &cf)] {
        ensure!(path.exists(), "{label} has no {what}: {}", path.display());
    }
    let has: Vec<bool> = NODE_FILES
        .iter()
        .map(|parts| under(app, parts).exists())
        .collect();
    ensure!(
        has.iter().all(|there| *there) || !has.iter().any(|there| *there),
        "{label} holds some of Node's files and not all: {}",
        NODE_FILES
            .iter()
            .zip(&has)
            .map(|(parts, there)| format!(
                "{} {}",
                parts.last().copied().unwrap_or_default(),
                if *there { "there" } else { "missing" }
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let manifest = under(app, &["Contents", "Resources", "cli", "package.json"]);
    let cli_version = if manifest.exists() {
        let text = fs::read_to_string(&manifest).map_err(files("read", &manifest))?;
        let parsed: serde_json::Value = serde_json::from_str(&text).map_err(|cause| {
            Error::new(format!("could not read {}: {cause}", manifest.display()))
        })?;
        let named = parsed.get("version").and_then(serde_json::Value::as_str);
        Some(
            named
                .ok_or_else(|| Error::new(format!("{} has no version", manifest.display())))?
                .to_string(),
        )
    } else {
        None
    };
    Ok(BundleInfo {
        app: app.to_path_buf(),
        label: label.to_string(),
        binary,
        cf,
        version,
        node: has[0],
        cli_version,
    })
}

/// The bundle's code signature verifies, as the installed app verifies an update's.
pub fn verify_seal(app: &Path, env: &Env) -> Result {
    run(
        "/usr/bin/codesign",
        &args!["--verify", "--deep", "--strict", app],
        env,
    )
    .map(drop)
}

/// The bundle is signed ad hoc: no identity of anyone's, which no certificate of ours could be.
pub fn assert_ad_hoc(app: &Path, label: &str, env: &Env) -> Result {
    let shown = Invocation::new("/usr/bin/codesign", Path::new("."))
        .args(["-dv", "--verbose=2"])
        .arg(app);
    let shown = process::capture(&shown, env)?;
    ensure!(
        shown.code == 0,
        "{label}: codesign could not read the signature: {}",
        shown.stderr
    );
    ensure!(
        shown.stderr.contains("Signature=adhoc"),
        "{label} is not signed ad hoc: {}",
        shown.stderr
    );
    Ok(())
}

/// What a bundle holds at a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A file: the SHA-256 of its bytes, and its permission bits.
    File { sha256: String, mode: u32 },
    /// A link, by what it names.
    Link(PathBuf),
}

/// One entry of a digest manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its path under the root of the tree.
    pub name: String,
    pub kind: Kind,
}

/// Every file of a tree by path, with its digest and mode, and every link by what it names.
pub fn digest_manifest(root: &Path) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    visit(root, root, &mut entries)?;
    Ok(entries)
}

fn visit(root: &Path, directory: &Path, entries: &mut Vec<Entry>) -> Result {
    let mut found: Vec<_> = fs::read_dir(directory)
        .map_err(files("list", directory))?
        .collect::<io::Result<_>>()
        .map_err(files("list", directory))?;
    found.sort_by_key(fs::DirEntry::file_name);
    for entry in found {
        let path = entry.path();
        let named = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        let kind = entry.file_type().map_err(files("look at", &path))?;
        if kind.is_dir() {
            visit(root, &path, entries)?;
        } else if kind.is_symlink() {
            let target = fs::read_link(&path).map_err(files("read the link", &path))?;
            entries.push(Entry {
                name: named,
                kind: Kind::Link(target),
            });
        } else if kind.is_file() {
            entries.push(Entry {
                name: named,
                kind: Kind::File {
                    sha256: sha256_of(&path)?,
                    mode: mode_of(&path)?,
                },
            });
        } else {
            return Err(Error::new(format!(
                "unsupported bundle entry in digest manifest: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn sha256_of(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(files("open", path))?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).map_err(files("read", path))?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// A file's permission bits, which a system with none says as 0.
fn mode_of(path: &Path) -> Result<u32> {
    let metadata = fs::metadata(path).map_err(files("look at", path))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(metadata.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Ok(0)
    }
}

/// The app at `from` copied onto `to`, with its modes, links and attributes (what a
/// disk image's copy keeps), over whatever is at `to`: files of the old app that the
/// new one has not stay where they were.
pub fn copy_over(from: &Path, to: &Path, env: &Env) -> Result<PathBuf> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(files("make", parent))?;
    }
    run("/usr/bin/ditto", &args![from, to], env)?;
    Ok(to.to_path_buf())
}

/// A copy of the app at `to`, which was not there before: what is at `to` is taken away first.
pub fn copy_bundle(from: &Path, to: &Path, env: &Env) -> Result<PathBuf> {
    remove_all(to)?;
    copy_over(from, to, env)
}

/// Takes away a file or a tree where it is, and does nothing where it is not.
pub fn remove_all(path: &Path) -> Result {
    let removed = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(cause) => Err(cause),
    };
    removed.map_err(files("remove", path))
}

/// The update archive of the app at `app`: a gzipped tar of `ConsensFlow.app`, as a release makes it.
pub fn archive_of(app: &Path, archive: &Path, env: &Env) -> Result<PathBuf> {
    let name = app.file_name().map(OsStr::to_os_string).unwrap_or_default();
    ensure!(
        name == "ConsensFlow.app",
        "an update archive holds ConsensFlow.app"
    );
    if let Some(parent) = archive.parent() {
        fs::create_dir_all(parent).map_err(files("make", parent))?;
    }
    let folder = app.parent().unwrap_or(Path::new("."));
    // The copy of a file's attributes into the archive as `._` files is left out.
    let env = Env::from_vars(
        env.iter()
            .map(|(name, value)| (name.to_os_string(), value.to_os_string()))
            .chain([("COPYFILE_DISABLE".into(), "1".into())]),
    );
    run(
        "/usr/bin/tar",
        &args!["-czf", archive, "-C", folder, &name],
        &env,
    )?;
    Ok(archive.to_path_buf())
}

/// Copies of the update that the installed app's check refuses, each in a folder
/// of its own where it is `ConsensFlow.app`, and what it refuses them for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No `cli/bin/cf`, the bundle signed again over what is left, so its seal
    /// verifies and only the rule about `cf` can refuse it.
    WithoutCf,
    /// A byte more in `cf` after the signing, so its seal does not verify.
    Tampered,
}

impl Refusal {
    /// Every kind, in the order the smoke runs them.
    pub const ALL: [Self; 2] = [Self::WithoutCf, Self::Tampered];

    /// The name the kind goes by.
    pub fn name(self) -> &'static str {
        match self {
            Self::WithoutCf => "without-cf",
            Self::Tampered => "tampered",
        }
    }

    /// The words the installed app's refusal has to hold.
    pub fn words(self) -> &'static str {
        match self {
            Self::WithoutCf => "must include cf",
            Self::Tampered => "code-signature",
        }
    }

    /// Makes the refused bundle of the copy at `app`.
    fn make(self, app: &Path, env: &Env) -> Result {
        match self {
            Self::WithoutCf => {
                let cf = cf_of(app);
                fs::remove_file(&cf).map_err(files("remove", &cf))?;
                run(
                    "/usr/bin/codesign",
                    &args!["--force", "--deep", "--sign", "-", app],
                    env,
                )
                .map(drop)
            }
            Self::Tampered => {
                use std::io::Write;
                let cf = cf_of(app);
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(&cf)
                    .map_err(files("open", &cf))?;
                file.write_all(b"changed after signing")
                    .map_err(files("write", &cf))
            }
        }
    }
}

/// The copy of `from` that is refused for `kind`, made in `folder`.
pub fn refused_bundle(kind: Refusal, from: &Path, folder: &Path, env: &Env) -> Result<PathBuf> {
    let app = copy_bundle(from, &folder.join(kind.name()).join("ConsensFlow.app"), env)?;
    kind.make(&app, env)?;
    Ok(app)
}

#[cfg(test)]
mod tests;
