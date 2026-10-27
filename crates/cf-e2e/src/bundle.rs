//! The `cf` and the pane host a rig or a daemon runs, copied into the home's own folder.
//!
//! A window's `PATH` starts with the folder of the daemon's program (the bundle's `bin`),
//! so that the `cf` of this install is the one an agent finds. The programs under test are
//! built in the checkout's `target/release`, and a `cargo build` there takes `cf` away and
//! puts it back, however little changed: it unlinks the file and links it again, another
//! inode each time and the same modification time, so that a build that happened leaves no
//! trace. For a moment a window that runs `cf` finds none: T-514, a stand-in's `cf` "no such
//! file" once in a crate's run; T-581 took it away in 17 of 20 builds that found it up to
//! date, from a poll every 0.3 ms. Any build in the tree does it, a worker's own beside a
//! run, or a test process that builds the `cf` for itself.
//!
//! So the daemon runs from a copy that no build touches, and its windows find that one. A
//! copy, and not a hard link: the linker may write the file it links in place. Windows has
//! no such window, a running program being one cargo cannot replace (it fails to link
//! instead): there the program is run where it is built.

use std::path::{Path, PathBuf};

use crate::Result;

/// `program` as a rig runs it: a copy of it in `folder`, under its own name, made if it is not
/// there (a rig resumed on a home has the copy it had).
#[cfg(unix)]
pub(crate) fn copy_of(program: &Path, folder: &Path) -> Result<PathBuf> {
    use std::time::{Duration, Instant};

    use crate::{files, Error};

    files::make_dir(folder)?;
    let name = program
        .file_name()
        .ok_or_else(|| Error::Build(format!("{} names no file", program.display())))?;
    let own = folder.join(name);
    if own.is_file() {
        return Ok(own);
    }
    // The program may be missing as it is copied, a build having taken it for a moment.
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        match std::fs::copy(program, &own) {
            Ok(_) => return Ok(own),
            Err(source)
                if source.kind() == std::io::ErrorKind::NotFound && Instant::now() < until =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(source) => {
                return Err(Error::File {
                    action: "copy",
                    path: program.to_path_buf(),
                    source,
                })
            }
        }
    }
}

/// Windows runs the program where it is built.
#[cfg(not(unix))]
pub(crate) fn copy_of(program: &Path, _folder: &Path) -> Result<PathBuf> {
    Ok(program.to_path_buf())
}

/// The folder of the copies of a rig whose home is `root`.
pub(crate) fn folder(root: &Path) -> PathBuf {
    root.join("bundle").join("bin")
}

#[cfg(all(test, unix))]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::process::Run;
    use crate::{cf, checkout};

    /// The probe behind the copy (T-581: 17 of 20 builds that found `cf` up to date took it away
    /// from a poll every 0.3 ms, each leaving another inode and the same modification time).
    /// Opt in; 20 builds that do nothing, and the copy polled beside the build's:
    ///
    /// `cargo test -p cf-e2e --lib -- --ignored a_cargo_build_of_cf --nocapture`
    ///
    /// Only the copy is asserted; how often the build's goes missing is cargo's, and the count
    /// is said. The build is relinked 20 times in the checkout: do not run it beside a run that
    /// starts the `cf` by its path.
    #[test]
    #[ignore = "relinks target/release/cf 20 times: cargo test -p cf-e2e --lib -- --ignored a_cargo_build_of_cf --nocapture"]
    fn a_cargo_build_of_cf_takes_it_away_for_a_moment_and_the_copy_stays() {
        let built = cf::binary().unwrap().to_path_buf();
        let folder = tempfile::tempdir().unwrap();
        let copy = copy_of(&built, folder.path()).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let (built_gone, copy_gone) =
            (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let polling = {
            let (stop, built, copy) = (Arc::clone(&stop), built.clone(), copy.clone());
            let (built_gone, copy_gone) = (Arc::clone(&built_gone), Arc::clone(&copy_gone));
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if !built.exists() {
                        built_gone.fetch_add(1, Ordering::Relaxed);
                    }
                    if !copy.exists() {
                        copy_gone.fetch_add(1, Ordering::Relaxed);
                    }
                    std::thread::sleep(Duration::from_micros(300));
                }
            })
        };
        for _ in 0..20 {
            let ran = Run::new(env!("CARGO"))
                .args([
                    "build",
                    "--offline",
                    "--release",
                    "--locked",
                    "-p",
                    "cf",
                    "--bin",
                    "cf",
                ])
                .cwd(checkout::root())
                .inheriting_env()
                .unlimited()
                .run()
                .unwrap();
            assert_eq!(ran.code, Some(0), "{ran}");
        }
        stop.store(true, Ordering::Relaxed);
        polling.join().unwrap();
        eprintln!(
            "polls that found the build's cf missing: {}; the copy's: {}",
            built_gone.load(Ordering::Relaxed),
            copy_gone.load(Ordering::Relaxed)
        );
        assert_eq!(
            copy_gone.load(Ordering::Relaxed),
            0,
            "the copy went missing"
        );
    }
}
