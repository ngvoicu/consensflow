//! A file's identity as the system holds it (the device and the file's number on Unix, the
//! volume and the file's index on Windows): what the write door checks a file it made by when it
//! writes it again, so that a file put at that path since is not taken for the run's (K3,
//! T-2002 finding 5), and what tells whether two spellings of a name are one file.
#![allow(unsafe_code)] // Windows' `GetFileInformationByHandle`, which std does not give.

use std::fs;
use std::io;
use std::path::Path;

/// The identity of `file`, an open file, as text: the device and file number on Unix, the
/// volume serial and the file's whole 128-bit id on Windows (its 64-bit index only where the
/// system gives no 128-bit one, which ReFS needs and NTFS has).
pub fn of_file(file: &fs::File) -> io::Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let found = file.metadata()?;
        Ok(format!("{}:{}", found.dev(), found.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle as _;
        use windows_sys::Win32::Storage::FileSystem::{
            FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
            BY_HANDLE_FILE_INFORMATION, FILE_ID_INFO,
        };
        let handle = file.as_raw_handle();
        // The file's whole id with its volume's serial, where the system gives it (T-2319 finding
        // 4): ReFS does not keep the 64-bit index unique.
        let mut whole: FILE_ID_INFO = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is the open file's own, and the structure is ours to fill.
        let given = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                std::ptr::from_mut(&mut whole).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        };
        if given != 0 {
            let id = whole.FileId.Identifier;
            // An id of nothing or of all ones is no id: the file system could not give one.
            if id.iter().all(|byte| *byte == 0) || id.iter().all(|byte| *byte == 0xff) {
                return Err(io::Error::other("the file system gave no id for the file"));
            }
            let text: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
            return Ok(format!("{}:{text}", whole.VolumeSerialNumber));
        }
        let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is the open file's own, and the structure is ours to fill.
        let read = unsafe { GetFileInformationByHandle(handle, &mut information) };
        if read == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(format!(
            "{}:{}:{}",
            information.dwVolumeSerialNumber, information.nFileIndexHigh, information.nFileIndexLow
        ))
    }
}

/// The identity of what `path` leads to: a file or a folder. On Unix it is read without opening
/// the file: a close of any descriptor of a file drops every `fcntl` lock the process holds on it,
/// and a database's (SQLite's, on its WAL and shared memory) with them. The stand-in Devin's store
/// then took its shared memory for unused, and the next read of its mapping ended the window with
/// SIGBUS (T-2007 round 2). Windows' locks are the handle's own, which a handle opened and closed
/// here does not touch.
pub fn of_path(path: &Path) -> io::Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let found = fs::metadata(path)?;
        Ok(format!("{}:{}", found.dev(), found.ino()))
    }
    #[cfg(windows)]
    {
        of_path_opened(path)
    }
}

/// [`of_path`] by a handle opened for it, where the system gives a file's identity only so.
#[cfg(windows)]
fn of_path_opened(path: &Path) -> io::Result<String> {
    let mut open = fs::OpenOptions::new();
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        // Only the file's attributes are asked for (`FILE_READ_ATTRIBUTES`), so that a folder
        // above a place that no one may read still tells its identity; a folder is opened only
        // with the flag that asks for it (`FILE_FLAG_BACKUP_SEMANTICS`).
        open.access_mode(0x0080).custom_flags(0x0200_0000);
    }
    of_file(&open.open(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_itself_and_a_file_put_at_its_path_since_is_another() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("report.txt");
        fs::write(&file, "first").unwrap();
        let first = of_path(&file).unwrap();
        assert_eq!(of_path(&file).unwrap(), first);
        fs::remove_file(&file).unwrap();
        // A file made at the same path while the first is still open is another file.
        let held = fs::File::create(folder.path().join("held")).unwrap();
        fs::write(&file, "second").unwrap();
        assert_ne!(of_path(&file).unwrap(), first);
        drop(held);
    }

    /// Windows' id is the file's whole one: a volume serial and 32 hex digits, where the system
    /// gives it (NTFS and ReFS both do), and no folder and file of one volume share it.
    #[cfg(windows)]
    #[test]
    fn a_files_identity_on_windows_is_its_whole_128_bit_id() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("report.txt");
        fs::write(&file, "x").unwrap();
        let (one, other) = (of_path(&file).unwrap(), of_path(folder.path()).unwrap());
        let (serial, id) = one.split_once(':').unwrap();
        assert!(serial.parse::<u64>().is_ok(), "{one}");
        assert_eq!(id.len(), 32, "{one}");
        assert!(id.chars().all(|digit| digit.is_ascii_hexdigit()), "{one}");
        assert_ne!(one, other);
    }
}
