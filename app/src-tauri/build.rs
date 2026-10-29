//! Tauri's build step, the identity the build is made with, which the app keeps
//! in its own file (`build/identity.rs`), and on Windows the manifest of the
//! library's test binary (`src/test_manifest.rs` says why it needs one).

// Cargo hands a build script its inputs in the environment, and the rules for
// the product's own code (clippy.toml) do not reach it.
#![allow(clippy::disallowed_methods)]

#[path = "build/identity.rs"]
mod identity;
#[path = "build/res.rs"]
mod res;

use std::error::Error;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::{env, fs};

/// The manifest, beside `Cargo.toml`.
const MANIFEST: &str = "tests.manifest";
/// The resource file made from it, which `src/test_manifest.rs` links as
/// `tests_manifest`.
const RESOURCE: &str = "tests_manifest.lib";

fn main() -> Result<(), Box<dyn Error>> {
    // The target this builds for, not the machine this script runs on: a Mac
    // runs it to check the Windows build, and has what is needed to make the file.
    let for_windows_msvc = env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows")
        && env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|abi| abi == "msvc");
    if for_windows_msvc {
        write_test_manifest()?;
    }
    hand_on_the_built_identity()?;
    tauri_build::build();
    Ok(())
}

/// Hands the app's code the identity this build is made with, as
/// `CONSENSFLOW_BUILT_IDENTITY` (`build/identity.rs`).
fn hand_on_the_built_identity() -> Result<(), Box<dyn Error>> {
    let read = |file: &str| -> Result<Option<String>, String> {
        match fs::read_to_string(file) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("{file}: {error}")),
        }
    };
    let given = env::var("TAURI_CONFIG").ok();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let (identity, files) = identity::built_identity(&read, &target_os, given.as_deref())?;
    for file in files {
        println!("cargo::rerun-if-changed={file}");
    }
    println!("cargo::rerun-if-env-changed=TAURI_CONFIG");
    println!("cargo::rustc-env=CONSENSFLOW_BUILT_IDENTITY={identity}");
    Ok(())
}

/// Writes the manifest as a resource file in `OUT_DIR`, and tells Cargo to
/// search there for the library that the tests link.
fn write_test_manifest() -> Result<(), Box<dyn Error>> {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo gave no OUT_DIR")?);
    fs::write(out_dir.join(RESOURCE), res::manifest(&fs::read(MANIFEST)?)?)?;
    println!("cargo::rerun-if-changed={MANIFEST}");
    println!("cargo::rustc-link-search=native={}", out_dir.display());
    Ok(())
}
