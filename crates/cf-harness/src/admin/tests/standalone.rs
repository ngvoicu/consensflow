//! Codex's standalone install as its installer lays it down (`install.ps1` on Windows,
//! `install.sh` elsewhere, Codex 0.167.0), made in a throwaway folder: a releases folder with a
//! stand-in `codex` in it, a `current` link to the release in use, and the visible folder on the
//! PATH, which leads to what `current` leads to. A `codex` there is where the person runs it, and
//! the real place of the file is the release's.

use std::fs;
use std::path::{Path, PathBuf};

/// The release the stand-in is laid down as, in the installer's name for a release
/// (`<version>-<target>`).
pub(in crate::admin) const RELEASE: &str = "0.166.0-x86_64-pc-windows-msvc";

/// Which of the installer's two layouts a stand-in is made as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::admin) enum Layout {
    /// A package: the program is in the release's `bin`, beside `codex-resources`, and the visible
    /// folder leads to `current\bin` (today's installer).
    Package,
    /// The release is the program and its `codex-resources`, and the visible folder leads to
    /// `current` (the installer's earlier layout, which Codex still reads as its own).
    Legacy,
}

/// The program's file name on this system.
pub(in crate::admin) fn program_name() -> &'static str {
    if cfg!(windows) {
        "codex.exe"
    } else {
        "codex"
    }
}

/// The folder Codex keeps its standalone releases and its `current` in, under its home.
pub(in crate::admin) fn standalone_root(codex_home: &Path) -> PathBuf {
    codex_home.join("packages").join("standalone")
}

/// A standalone Codex laid down for `home`'s `.codex`, its visible folder `visible`: the program
/// there, as the person runs it.
pub(in crate::admin) fn lay_down(home: &Path, visible: &Path, layout: Layout) -> PathBuf {
    lay_down_in(&home.join(".codex"), visible, layout)
}

/// A standalone Codex laid down in the Codex home `codex_home` (`CODEX_HOME`, or `.codex` of the
/// user's home), its visible folder `visible`: the program there, as the person runs it.
pub(in crate::admin) fn lay_down_in(codex_home: &Path, visible: &Path, layout: Layout) -> PathBuf {
    let root = standalone_root(codex_home);
    let release = release_in(&root, RELEASE, layout);
    link_dir(&root.join("current"), &release);
    let led_to = match layout {
        Layout::Package => root.join("current").join("bin"),
        Layout::Legacy => root.join("current"),
    };
    fs::create_dir_all(visible.parent().unwrap()).unwrap();
    link_dir(visible, &led_to);
    visible.join(program_name())
}

/// A release called `name` in `root`'s `releases`, with a program in it where `layout` has it;
/// its folder.
pub(in crate::admin) fn release_in(root: &Path, name: &str, layout: Layout) -> PathBuf {
    let release = root.join("releases").join(name);
    let program_folder = match layout {
        Layout::Package => release.join("bin"),
        Layout::Legacy => release.clone(),
    };
    fs::create_dir_all(&program_folder).unwrap();
    fs::create_dir_all(release.join("codex-resources")).unwrap();
    let program = program_folder.join(program_name());
    fs::write(&program, "").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    }
    release
}

/// `link` leading to the folder `target`: a symbolic link, and on Windows a junction, which is
/// what the installer makes there and which needs no administrator.
#[allow(clippy::disallowed_methods)] // The test makes the junction it is about.
pub(in crate::admin) fn link_dir(link: &Path, target: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        // Bare: a Windows test, and Windows hands a program no descriptor it is not asked to.
        let made = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
    }
}
