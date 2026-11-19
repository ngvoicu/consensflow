//! A login-file revision from its path and attributes, never its contents.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub(crate) async fn revision_async(path: PathBuf) -> Option<u64> {
    tokio::task::spawn_blocking(move || revision(&path))
        .await
        .ok()?
}

fn revision(path: &Path) -> Option<u64> {
    let mut hash = Sha256::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hash.update(path.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for word in path.as_os_str().encode_wide() {
            hash.update(word.to_le_bytes());
        }
    }
    match cf_base::file::stat(path) {
        Ok(stat) => {
            let (volume, index) = stat.identity.parts();
            hash.update([1]);
            hash.update(volume.to_le_bytes());
            hash.update(index.to_le_bytes());
            hash.update(stat.size.to_le_bytes());
            let (before_epoch, modified) = match stat.modified.duration_since(std::time::UNIX_EPOCH)
            {
                Ok(modified) => (false, modified),
                Err(before) => (true, before.duration()),
            };
            hash.update([u8::from(before_epoch)]);
            hash.update(modified.as_secs().to_le_bytes());
            hash.update(modified.subsec_nanos().to_le_bytes());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => hash.update([0]),
        Err(_) => return None,
    }
    let bytes = hash.finalize();
    Some(u64::from_le_bytes(bytes[..8].try_into().ok()?))
}
