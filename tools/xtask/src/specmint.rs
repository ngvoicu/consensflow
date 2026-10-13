//! `cargo xtask specmint-sync <source-root>`: the four Specmint skills a chief
//! may plan with, vendored in the repository (`skill/specmint/<variant>`, which
//! the harness crate embeds in the binary) refreshed from checkouts of their
//! repositories, and the manifest that pins them written again. The command is
//! the whole of a refresh, so one can be made again: nothing of the vendored
//! files is edited by hand.
//!
//! The source root holds one checkout of each repository, named
//! `specmint-core`, `specmint-core-html`, `specmint-tdd` and `specmint-tdd-html`.
//! One with changes that are not committed is refused (`git status --porcelain`
//! says nothing of it), since what is vendored is what its commit holds, which
//! the manifest names. From each, the files that ship are copied: `SKILL.md` and
//! `LICENSE`, and every file under `references` and `assets` where the
//! repository has them (the HTML variants have `assets`). Everything is read
//! before anything is written, so a checkout that is refused leaves the
//! vendored copy as it was; a file a repository no longer ships is gone from the
//! copy. The manifest (`skill/specmint/manifest.json`) names each variant's
//! commit and the SHA-256 of each file shipped, in the order the page offers the
//! variants and by file name, in the shape a test of the harness reads it.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};

pub const COMMANDS: &[Command] = &[Command {
    words: &["specmint-sync"],
    about: "Refresh the vendored Specmint skills (skill/specmint) from checkouts of their four repositories and write their manifest again",
    usage: "<source-root> (the folder that holds the checkouts specmint-core, specmint-core-html, specmint-tdd and specmint-tdd-html; one with uncommitted changes is refused)",
    run: Run::Native(run),
}];

/// The four variants, as the ledger, the page and the harness name them, in the
/// order the page offers them (a test holds this to the manifest, which a test
/// of the harness holds to `cf_proto::specs::Variant`).
const VARIANTS: [&str; 4] = ["core", "core-html", "tdd", "tdd-html"];

/// Where the vendored skills are, from the checkout's root.
const VENDORED: &str = "skill/specmint";

/// The files of a checkout that ship, besides the ones of the folders below.
const FILES: [&str; 2] = ["SKILL.md", "LICENSE"];

/// The folders of a checkout whose every file ships, where it has them.
const FOLDERS: [&str; 2] = ["references", "assets"];

/// Why a refresh could not be made.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{} is no checkout of the {variant} skill: it is not there", path.display())]
    NoCheckout { variant: String, path: PathBuf },
    #[error("{} is no git repository: {why}", path.display())]
    NotARepository { path: PathBuf, why: String },
    #[error("{} has changes that are not committed, so it is no commit to vendor; commit or put them away:\n{changes}", path.display())]
    Dirty { path: PathBuf, changes: String },
    #[error("{} has no {file}: it is no checkout of the skill", path.display())]
    Missing { path: PathBuf, file: String },
    #[error("{0}")]
    Process(#[from] process::Failure),
    #[error("{} could not be read or written: {cause}", path.display())]
    File { path: PathBuf, cause: io::Error },
}

/// What one checkout gave: the commit it is at, and its files that ship by
/// their names under it, in slashes.
struct Taken {
    variant: &'static str,
    commit: String,
    files: BTreeMap<String, Vec<u8>>,
}

fn run(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    let [source] = args else {
        return Err(Failure::Usage(
            "specmint-sync takes the folder that holds the four checkouts, and nothing else".into(),
        ));
    };
    let taken = take(Path::new(source), &context.env)?;
    write(&context.path(VENDORED), &taken)?;
    for each in &taken {
        writeln!(
            console.err,
            "specmint-sync: {}: {} ({} files)",
            each.variant,
            each.commit,
            each.files.len()
        )?;
    }
    Ok(0)
}

/// Every variant's commit and files, from the checkouts in `source`: none if
/// any is refused.
fn take(source: &Path, env: &Env) -> Result<Vec<Taken>, Error> {
    VARIANTS
        .iter()
        .map(|&variant| {
            let checkout = source.join(format!("specmint-{variant}"));
            if !checkout.is_dir() {
                return Err(Error::NoCheckout {
                    variant: variant.to_owned(),
                    path: checkout,
                });
            }
            let commit = committed(&checkout, env)?;
            let files = shipped(&checkout)?;
            Ok(Taken {
                variant,
                commit,
                files,
            })
        })
        .collect()
}

/// The commit `checkout` is at, where nothing of it is left uncommitted.
fn committed(checkout: &Path, env: &Env) -> Result<String, Error> {
    let git = |words: &[&str]| {
        process::capture(
            &Invocation::new("git", checkout).args(words.iter().copied()),
            env,
        )
    };
    let status = git(&["status", "--porcelain"])?;
    if status.code != 0 {
        return Err(Error::NotARepository {
            path: checkout.to_path_buf(),
            why: status.stderr.trim().to_owned(),
        });
    }
    if !status.stdout.trim().is_empty() {
        return Err(Error::Dirty {
            path: checkout.to_path_buf(),
            changes: status.stdout.trim_end().to_owned(),
        });
    }
    let head = git(&["rev-parse", "HEAD"])?;
    if head.code != 0 {
        return Err(Error::NotARepository {
            path: checkout.to_path_buf(),
            why: head.stderr.trim().to_owned(),
        });
    }
    Ok(head.stdout.trim().to_owned())
}

/// The files of `checkout` that ship, by their names under it.
fn shipped(checkout: &Path) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let mut files = BTreeMap::new();
    for file in FILES {
        let path = checkout.join(file);
        if !path.is_file() {
            return Err(Error::Missing {
                path: checkout.to_path_buf(),
                file: file.to_owned(),
            });
        }
        files.insert(file.to_owned(), read(&path)?);
    }
    for folder in FOLDERS {
        collect(checkout, &checkout.join(folder), &mut files)?;
    }
    Ok(files)
}

/// Every file under `folder` (none where it is not there), by its name under `base`.
fn collect(base: &Path, folder: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> Result<(), Error> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry
            .map_err(|cause| Error::File {
                path: folder.to_path_buf(),
                cause,
            })?
            .path();
        if path.is_dir() {
            collect(base, &path, files)?;
        } else if path.is_file() {
            let name = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            files.insert(name, read(&path)?);
        }
    }
    Ok(())
}

fn read(path: &Path) -> Result<Vec<u8>, Error> {
    fs::read(path).map_err(|cause| Error::File {
        path: path.to_path_buf(),
        cause,
    })
}

/// Puts what was taken in `vendored`: each variant's folder made again from its
/// files, and the manifest.
fn write(vendored: &Path, taken: &[Taken]) -> Result<(), Error> {
    let put = |path: &Path, bytes: &[u8]| {
        if let Some(above) = path.parent() {
            fs::create_dir_all(above).map_err(|cause| Error::File {
                path: above.to_path_buf(),
                cause,
            })?;
        }
        fs::write(path, bytes).map_err(|cause| Error::File {
            path: path.to_path_buf(),
            cause,
        })
    };
    for each in taken {
        let folder = vendored.join(each.variant);
        if folder.exists() {
            fs::remove_dir_all(&folder).map_err(|cause| Error::File {
                path: folder.clone(),
                cause,
            })?;
        }
        for (name, bytes) in &each.files {
            put(&folder.join(name), bytes)?;
        }
    }
    put(&vendored.join("manifest.json"), manifest(taken).as_bytes())
}

/// The manifest: each variant's commit and the SHA-256 of each file shipped.
fn manifest(taken: &[Taken]) -> String {
    let mut variants = Map::new();
    for each in taken {
        let files: Map<String, Value> = each
            .files
            .iter()
            .map(|(name, bytes)| (name.clone(), json!(sha256(bytes))))
            .collect();
        variants.insert(
            each.variant.to_owned(),
            json!({ "commit": each.commit, "files": files }),
        );
    }
    let mut text = serde_json::to_string_pretty(&Value::Object(variants)).unwrap_or_default();
    text.push('\n');
    text
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests;
