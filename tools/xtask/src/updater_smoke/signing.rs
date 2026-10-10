//! The updater key of one smoke run: a pair made for the run (`tauri signer
//! generate`), whose public half the apps are built with and whose private
//! half signs the archives the run serves. It never leaves the run's folder,
//! and nothing here reads, uses or copies the product's own key (the one
//! its maintainer keeps in the home folder) or any Apple key or certificate:
//! what a build or a signer inherits of those from the caller's shell is taken
//! out first, and the signer is told the one file it may read.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;

use super::{files, Error, Result};
use crate::process::{self, Invocation};

/// The variables that carry an updater key, an Apple certificate or an identity to sign with.
const SIGNING_VARIABLES: [&str; 3] = ["TAURI_", "APPLE_", "CSC_"];

/// The environment a build or a signer runs in: the caller's, without any
/// signing variable, and offline (cargo reads no network, so a crate not
/// already fetched fails the build, as `--offline` has it).
pub fn clean_env(base: &Env) -> Env {
    let kept = base
        .iter()
        .filter(|(name, _)| {
            let name = name.to_string_lossy();
            !SIGNING_VARIABLES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        })
        .map(|(name, value)| (name.to_os_string(), value.to_os_string()));
    Env::from_vars(kept.chain([("CARGO_NET_OFFLINE".into(), "true".into())]))
}

/// The Tauri CLI a checkout has, the program `npm` links in its `.bin`: a `.cmd`
/// file on Windows.
pub fn tauri_bin(checkout: &Path) -> PathBuf {
    let name = if cfg!(windows) { "tauri.cmd" } else { "tauri" };
    ["app", "node_modules", ".bin", name]
        .iter()
        .fold(checkout.to_path_buf(), |path, part| path.join(part))
}

/// Runs the Tauri CLI of `checkout` in it with `args`, in a clean environment,
/// and answers what it printed.
fn tauri(checkout: &Path, args: &[OsString], env: &Env) -> Result<String> {
    let invocation = Invocation::new(tauri_bin(checkout), checkout).args(args.iter().cloned());
    let ran = process::capture(&invocation, &clean_env(env))?;
    if ran.code != 0 {
        let said = ran.stderr.trim();
        let said = if said.is_empty() {
            format!("it ended with status {}", ran.code)
        } else {
            said.to_string()
        };
        let named: Vec<_> = args
            .iter()
            .take(2)
            .map(|arg| arg.to_string_lossy())
            .collect();
        return Err(Error::new(format!(
            "tauri {} failed: {said}",
            named.join(" ")
        )));
    }
    Ok(ran.stdout)
}

/// A key pair: the private key's file, and the public key, in the form
/// `plugins.updater.pubkey` takes, which the file beside it holds.
#[derive(Debug, Clone)]
pub struct Key {
    pub private_key: PathBuf,
    pub public_key_file: PathBuf,
    pub public_key: String,
}

/// `path` with `suffix` after its name.
pub fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// A new key pair in `directory`, with no password: the private key and the
/// public key (`<key>.pub`).
pub fn generate_key(checkout: &Path, directory: &Path, env: &Env) -> Result<Key> {
    fs::create_dir_all(directory).map_err(files("make", directory))?;
    let private_key = directory.join("updater.key");
    let args = args![
        "signer",
        "generate",
        "--ci",
        "--password",
        "",
        "--write-keys",
        &private_key
    ];
    tauri(checkout, &args, env)?;
    let public_key_file = beside(&private_key, ".pub");
    let public_key =
        fs::read_to_string(&public_key_file).map_err(files("read", &public_key_file))?;
    Ok(Key {
        private_key,
        public_key_file,
        public_key: public_key.trim().to_string(),
    })
}

/// The signature of `file` by `private_key`, as the feed carries it (the `.sig`
/// file's one line).
pub fn sign_file(checkout: &Path, private_key: &Path, file: &Path, env: &Env) -> Result<String> {
    let args = args![
        "signer",
        "sign",
        "--password",
        "",
        "--private-key-path",
        private_key,
        file
    ];
    tauri(checkout, &args, env)?;
    let signature = beside(file, ".sig");
    let text = fs::read_to_string(&signature).map_err(files("read", &signature))?;
    Ok(text.trim().to_string())
}

#[cfg(test)]
mod tests;
