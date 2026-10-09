//! Unpacking the payload's tar: its files and folders, and nothing that could
//! land outside the folder they are unpacked into.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path};

use flate2::read::GzDecoder;

use crate::error::{files, Error};
use crate::format::Payload;

impl Payload {
    /// The payload's tar, from `file`, into `into`: its files and folders,
    /// nothing else. An entry that is a link or any other special file, one
    /// whose path is absolute or has a `..` in it, and a payload whose gzip
    /// stream is cut short or does not match its CRC are all refused. Reading
    /// the stream to its end is what checks the CRC and the length in its
    /// trailer, which unpacking alone would not: tar stops reading at the
    /// archive's end marker.
    ///
    /// `into` is made if it is not there. On an error it holds what was
    /// unpacked before it: a caller that cannot use half a runtime unpacks into
    /// a folder it throws away.
    pub fn extract<R: Read + Seek>(&self, file: &mut R, into: &Path) -> Result<(), Error> {
        fs::create_dir_all(into).map_err(files("make", into))?;
        file.seek(SeekFrom::Start(self.offset))?;
        let mut archive = tar::Archive::new(GzDecoder::new(file.by_ref().take(self.length)));
        for entry in archive.entries()? {
            let mut entry = entry?;
            let kind = entry.header().entry_type();
            if kind.is_pax_global_extensions() {
                continue;
            }
            let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
            if !(kind.is_file() || kind.is_dir()) {
                return Err(Error::NotPlain { path: name });
            }
            if !stays_inside(&entry.path()?) || !entry.unpack_in(into)? {
                return Err(Error::Outside { path: name });
            }
        }
        io::copy(&mut archive.into_inner(), &mut io::sink())?;
        Ok(())
    }
}

/// Whether a path names something under the folder it is unpacked into: no
/// root, no drive, no `..`. (The tar crate would unpack an absolute path under
/// the folder with its root taken off; a payload that holds one is damaged.)
fn stays_inside(path: &Path) -> bool {
    path.components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
}
