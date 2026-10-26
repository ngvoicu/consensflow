//! Where a transcript ended when ConsensFlow started a window's program on its
//! conversation: what was written before it was written by a process that is gone.
//!
//! A length alone says that only of the file that was that long. A transcript written
//! anew since, shorter or otherwise not holding what it held, has other bytes there,
//! and the length would cut what the process that runs wrote (both reviews of the
//! relaunch boundary, 2026-10-18). So the boundary keeps the bytes before it, as many as
//! a look checks before where it stopped (`jsonl`'s edge), and holds only in a file that
//! still has them there: a prefix checked as the follower checks its own.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use super::jsonl::EDGE_BYTES;

/// How many bytes a transcript held, and the last of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boundary {
    pub at: u64,
    pub edge: Arc<[u8]>,
}

impl Boundary {
    /// Where `file` ends now: its length, and the bytes before it, up to `EDGE_BYTES`.
    /// What is appended after the length is read is not part of it.
    pub(crate) fn of(file: &Path) -> io::Result<Self> {
        let mut handle = File::open(file)?;
        let at = handle.metadata()?.len();
        let edge = bytes_at(&mut handle, at.saturating_sub(EDGE_BYTES), at)?;
        Ok(Self {
            at,
            edge: edge.into(),
        })
    }

    /// Whether `file` holds the boundary's bytes just before its byte: it is the
    /// transcript the boundary was taken of, or one that kept all of it this far. A file
    /// that cannot be read holds none.
    pub(crate) fn held_by(&self, file: &Path) -> bool {
        let Some(from) = self.at.checked_sub(self.edge.len() as u64) else {
            return false;
        };
        File::open(file)
            .and_then(|mut handle| bytes_at(&mut handle, from, self.at))
            .is_ok_and(|bytes| *bytes == *self.edge)
    }
}

/// The bytes of `handle` from `from` to `to`, fewer where the file ends first.
fn bytes_at(handle: &mut File, from: u64, to: u64) -> io::Result<Vec<u8>> {
    handle.seek(SeekFrom::Start(from))?;
    let mut bytes = Vec::new();
    handle.take(to - from).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_boundary_holds_in_its_file_grown_and_in_no_file_written_anew_without_its_bytes() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("t.jsonl");
        let long = format!("{}\n", "a".repeat(3000));
        fs::write(&file, &long).unwrap();
        let boundary = Boundary::of(&file).unwrap();
        assert_eq!(boundary.at, 3001);
        assert_eq!(boundary.edge.len() as u64, EDGE_BYTES);
        assert!(boundary.held_by(&file));
        fs::write(&file, format!("{long}{{}}\n")).unwrap();
        assert!(boundary.held_by(&file), "grown");
        fs::write(&file, "{}\n").unwrap();
        assert!(!boundary.held_by(&file), "shorter");
        fs::write(&file, format!("{}\n", "b".repeat(4000))).unwrap();
        assert!(!boundary.held_by(&file), "as long, other bytes");
        fs::remove_file(&file).unwrap();
        assert!(!boundary.held_by(&file), "gone");
        fs::write(&file, "").unwrap();
        let empty = Boundary::of(&file).unwrap();
        assert_eq!((empty.at, empty.edge.len()), (0, 0));
        assert!(empty.held_by(&file));
    }
}
