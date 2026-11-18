use std::fs::{File, TryLockError};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[allow(clippy::disallowed_methods)] // Tests read the run's inherited lock and temporary folder.
fn value(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

#[allow(clippy::expect_used)] // A Windows machine names its system folder.
pub fn lock_path() -> PathBuf {
    if let Some(path) = value("CF_PWSH_START_LOCK") {
        return path.into();
    }
    #[cfg(windows)]
    let temp = value("TEMP")
        .or_else(|| value("TMP"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(value("SystemRoot").expect("SystemRoot")).join("Temp"));
    #[cfg(not(windows))]
    let temp = value("TMPDIR")
        .or_else(|| value("TMP"))
        .or_else(|| value("TEMP"))
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    temp.join("consensflow-pwsh-start-v1.lock")
}

/// Takes the run's shared PowerShell start lock. Closing the handle releases it;
/// a persistent fallback in Temp is opened without following a final link and never removed.
#[allow(clippy::disallowed_methods, clippy::expect_used)]
pub fn one_at_a_time<T>(start: impl FnOnce() -> T) -> T {
    let mut options = File::options();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x100).mode(0o600); // O_NOFOLLOW
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x20000).mode(0o600); // O_NOFOLLOW
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(3); // OPEN_REPARSE_POINT; no delete sharing
    }
    let file = options
        .open(lock_path())
        .expect("open the PowerShell start lock");
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        assert_eq!(
            file.metadata().expect("lock attributes").file_attributes() & 0x400,
            0,
            "the PowerShell lock is a reparse point"
        );
    }
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(why) => panic!("could not hold the PowerShell start lock: {why}"),
        }
    }
    let result = start();
    drop(file);
    result
}
