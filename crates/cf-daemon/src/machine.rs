//! What the daemon finds out about the machine it runs on: where the bundle it
//! ships with is, found from its own binary's place.

use std::path::Path;

use cf_harness::seams::Bundle;

/// The bundle beside the binary at `exe`: its `bin` is the binary's folder,
/// first on every window's PATH, and its native `cf` is `bin/cf`
/// (`bin/cf.exe`), which a window's role text and hooks name. On Windows that
/// path is written with forward slashes, which Git Bash keeps where it drops
/// backslashes and PowerShell reads alike (`src/core/pane-cf.js`).
pub fn bundle_of(exe: &Path) -> Bundle {
    let bin = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let cf = bin.join(if cfg!(windows) { "cf.exe" } else { "cf" });
    let written = cf.to_string_lossy().into_owned();
    let pane_cf = if cfg!(windows) {
        written.replace('\\', "/")
    } else {
        written
    };
    Bundle { bin, cf, pane_cf }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_bundle_is_the_folder_of_the_binary_and_its_cf_is_named_there() {
        let bundle = bundle_of(Path::new("/Applications/ConsensFlow/bin/cf"));
        assert_eq!(bundle.bin, Path::new("/Applications/ConsensFlow/bin"));
        assert_eq!(bundle.cf, Path::new("/Applications/ConsensFlow/bin/cf"));
        assert_eq!(bundle.pane_cf, "/Applications/ConsensFlow/bin/cf");
    }

    #[cfg(unix)]
    #[test]
    fn a_binary_by_another_name_still_names_the_cf_beside_it() {
        let bundle = bundle_of(Path::new("/target/debug/deps/cf_daemon-1a2b"));
        assert_eq!(bundle.bin, Path::new("/target/debug/deps"));
        assert_eq!(bundle.cf, Path::new("/target/debug/deps/cf"));
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_the_cf_a_window_names_has_forward_slashes() {
        let bundle = bundle_of(Path::new(r"C:\Program Files\ConsensFlow\bin\cf.exe"));
        assert_eq!(
            bundle.cf,
            Path::new(r"C:\Program Files\ConsensFlow\bin\cf.exe")
        );
        assert_eq!(bundle.pane_cf, "C:/Program Files/ConsensFlow/bin/cf.exe");
    }
}
