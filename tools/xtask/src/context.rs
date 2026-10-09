//! What every command is told about where it works: the checkout, and the
//! environment xtask was started with.
//!
//! The checkout is where this xtask was built, found from where its own
//! manifest is (`<checkout>/tools/xtask`), never from the folder it was run
//! in: `cargo xtask` runs the same from the root, from `app/` or from any
//! folder below.

use std::path::{Path, PathBuf};

use cf_base::env::Env;

/// Where a command works and with what environment.
#[derive(Debug, Clone)]
pub struct Context {
    /// The checkout's root, where the root `Cargo.toml` is.
    pub root: PathBuf,
    /// The environment this process was started with, read once in `main`.
    pub env: Env,
}

/// Why there is no checkout to work on.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("this xtask was built from {}, which is no folder of a checkout (tools/xtask)", manifest_dir.display())]
    NotInACheckout { manifest_dir: PathBuf },
    #[error("the checkout this xtask was built in is gone: {} has no Cargo.toml; build xtask again where the checkout is", root.display())]
    Gone { root: PathBuf },
}

impl Context {
    /// The checkout this xtask was built in.
    pub fn new(env: &Env) -> Result<Self, Error> {
        Self::at(Path::new(env!("CARGO_MANIFEST_DIR")), env)
    }

    /// The checkout whose xtask has its manifest in `manifest_dir`.
    pub fn at(manifest_dir: &Path, env: &Env) -> Result<Self, Error> {
        let root = manifest_dir
            .ancestors()
            .nth(2)
            .ok_or_else(|| Error::NotInACheckout {
                manifest_dir: manifest_dir.to_path_buf(),
            })?;
        if !root.join("Cargo.toml").is_file() {
            return Err(Error::Gone {
                root: root.to_path_buf(),
            });
        }
        Ok(Self {
            root: root.to_path_buf(),
            env: env.clone(),
        })
    }

    /// `relative`, written with `/` between folders as the repository does,
    /// under the checkout's root and with the platform's separator. The root
    /// itself for an empty one.
    pub fn path(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .filter(|part| !part.is_empty())
            .fold(self.root.clone(), |path, part| path.join(part))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checkout_is_found_from_the_manifest_whatever_folder_the_run_is_in() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let context = Context::new(&Env::default()).unwrap();
        assert_eq!(context.root, manifest.parent().unwrap().parent().unwrap());
        assert!(context
            .root
            .join("tools")
            .join("xtask")
            .join("Cargo.toml")
            .is_file());
        assert!(context
            .root
            .join("app")
            .join("src-tauri")
            .join("tauri.conf.json")
            .is_file());
    }

    #[test]
    fn a_manifest_that_is_in_no_checkout_or_whose_checkout_is_gone_is_said_so() {
        let said = Context::at(Path::new("/"), &Env::default())
            .unwrap_err()
            .to_string();
        assert_eq!(
            said,
            "this xtask was built from /, which is no folder of a checkout (tools/xtask)"
        );

        let moved = tempfile::tempdir().unwrap();
        let manifest = moved.path().join("tools").join("xtask");
        let said = Context::at(&manifest, &Env::default())
            .unwrap_err()
            .to_string();
        assert_eq!(
            said,
            format!(
                "the checkout this xtask was built in is gone: {} has no Cargo.toml; \
                 build xtask again where the checkout is",
                moved.path().display()
            )
        );
    }

    #[test]
    fn the_environment_is_the_one_it_is_given() {
        let env = Env::from_vars([("CF_XTASK_TEST", "yes")]);
        let context = Context::new(&env).unwrap();
        assert_eq!(context.env.text("CF_XTASK_TEST"), Some("yes"));
    }

    #[test]
    fn a_path_is_written_under_the_root_with_the_platforms_separator() {
        let context = Context {
            root: PathBuf::from("checkout"),
            env: Env::default(),
        };
        let expected: PathBuf = ["checkout", "app", "scripts", "build-cf.mjs"]
            .iter()
            .collect();
        assert_eq!(context.path("app/scripts/build-cf.mjs"), expected);
        assert_eq!(context.path(""), PathBuf::from("checkout"));
        assert_eq!(context.path("app/"), PathBuf::from("checkout").join("app"));
    }
}
