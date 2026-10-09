//! The footer, and the payload it finds: where it starts, how long it is, and
//! what its gzip trailer says of the tar inside.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{files, Error};

/// The footer's tag: the last eight bytes of an exe that carries its runtime.
const TAG: &[u8; 8] = b"CFPAYLD1";
/// The footer: the payload's length, then the tag.
const FOOTER_BYTES: usize = 16;
/// The smallest gzip stream there is: its header and its trailer.
const SMALLEST_GZIP: u64 = 18;
/// A gzip stream ends with a trailer of eight bytes: the CRC32 of what was
/// compressed, then its length modulo 2^32, each a little-endian `u32`.
const TRAILER_BYTES: u64 = 8;

/// The footer of an exe whose payload is `length` bytes long.
pub fn footer(length: u64) -> [u8; FOOTER_BYTES] {
    let mut footer = [0; FOOTER_BYTES];
    footer[..8].copy_from_slice(&length.to_le_bytes());
    footer[8..].copy_from_slice(TAG);
    footer
}

/// Where an exe carries its runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Payload {
    /// Where the payload starts: the length of the app's own exe before it.
    pub offset: u64,
    /// The payload's length in bytes: a gzip stream, to the footer.
    pub length: u64,
    /// The CRC32 of the tar inside, from the gzip trailer.
    pub crc: u32,
    /// The length of the tar inside in bytes, modulo 2^32, from the gzip
    /// trailer.
    pub tar_length: u32,
}

impl Payload {
    /// The payload `file` carries, read from its footer; `None` when it does
    /// not end with the tag. A footer that names more than the file holds, or
    /// less than any gzip stream, is an error: the file is damaged.
    pub fn find<R: Read + Seek>(file: &mut R) -> Result<Option<Self>, Error> {
        let footer_bytes = FOOTER_BYTES as u64;
        let size = file.seek(SeekFrom::End(0))?;
        if size < footer_bytes {
            return Ok(None);
        }
        let (mut length, mut tag) = ([0; 8], [0; 8]);
        file.seek(SeekFrom::Start(size - footer_bytes))?;
        file.read_exact(&mut length)?;
        file.read_exact(&mut tag)?;
        if tag != *TAG {
            return Ok(None);
        }
        let length = u64::from_le_bytes(length);
        if length < SMALLEST_GZIP || length > size - footer_bytes {
            return Err(Error::Footer { length, size });
        }
        let offset = size - footer_bytes - length;
        let (mut crc, mut tar_length) = ([0; 4], [0; 4]);
        file.seek(SeekFrom::Start(offset + length - TRAILER_BYTES))?;
        file.read_exact(&mut crc)?;
        file.read_exact(&mut tar_length)?;
        Ok(Some(Self {
            offset,
            length,
            crc: u32::from_le_bytes(crc),
            tar_length: u32::from_le_bytes(tar_length),
        }))
    }

    /// The CRC as the eight hex digits a runtime folder's name ends with.
    pub fn crc_hex(&self) -> String {
        format!("{:08x}", self.crc)
    }

    /// The name of the folder a runtime is kept in: this version's, and this
    /// payload's. A folder for each build.
    pub fn folder(&self, version: &str) -> String {
        format!("{version}-{}", self.crc_hex())
    }
}

/// The payload the file at `path` carries; [`Error::NoFooter`] when it carries
/// none. Every error names the file.
pub fn inspect(path: &Path) -> Result<Payload, Error> {
    let mut file = File::open(path).map_err(files("open", path))?;
    match Payload::find(&mut file) {
        Ok(Some(payload)) => Ok(payload),
        Ok(None) => Err(Error::NoFooter {
            path: path.to_path_buf(),
        }),
        Err(cause) => Err(files("read", path)(cause.into())),
    }
}
