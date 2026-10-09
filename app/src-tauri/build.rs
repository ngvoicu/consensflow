//! Tauri's build step, and on Windows the manifest of the library's test
//! binary (`src/test_manifest.rs` says why it needs one).

// Cargo hands a build script its inputs in the environment, and the rules for
// the product's own code (clippy.toml) do not reach it.
#![allow(clippy::disallowed_methods)]

#[path = "build/res.rs"]
mod res;

use std::error::Error;
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
    tauri_build::build();
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
