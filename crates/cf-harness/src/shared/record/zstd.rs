//! A bounded streaming reader of a compressed rollout. A seek backwards
//! restarts decompression; no decompressed file or pipe is created.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

type Decoder = zstd::stream::read::Decoder<'static, BufReader<File>>;

/// Codex's compressed representation, not a backup or a temporary file.
pub(crate) fn is_rollout(file: &Path) -> bool {
    file.file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".jsonl.zst"))
}

/// The uncompressed sibling Codex may publish before it removes the compressed one.
pub(crate) fn plain_sibling(file: &Path) -> Option<std::path::PathBuf> {
    if !is_rollout(file) {
        return None;
    }
    Some(file.with_extension(""))
}

/// At most a 128 MiB decoder window, larger than Codex's level-3 writer needs.
pub(crate) fn decoder(file: File) -> io::Result<Decoder> {
    if file.metadata()?.len() == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "empty zstd stream",
        ));
    }
    let mut decoder = zstd::stream::read::Decoder::new(file)?;
    decoder.window_log_max(27)?;
    Ok(decoder)
}

/// A virtual seekable view of a compressed file. Its size is found by one
/// streaming pass, which also detects a truncated frame before any result is
/// returned. Backwards seeks decode from the start again. It is for the JSONL
/// follower, whose offsets and torn-line handling refer to decoded bytes.
pub(crate) struct Decoded {
    decoder: Option<Decoder>,
    at: u64,
    len: u64,
}

impl Decoded {
    pub(crate) fn new(file: File) -> io::Result<Self> {
        let mut decoder = decoder(file)?;
        let len = io::copy(&mut decoder, &mut io::sink())?;
        let mut decoded = Self {
            decoder: Some(decoder),
            at: len,
            len,
        };
        decoded.rewind()?;
        Ok(decoded)
    }

    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    fn rewind(&mut self) -> io::Result<()> {
        let mut file = self
            .decoder
            .take()
            .ok_or_else(|| io::Error::other("zstd decoder is unavailable"))?
            .finish()
            .into_inner();
        file.seek(SeekFrom::Start(0))?;
        self.decoder = Some(decoder(file)?);
        self.at = 0;
        Ok(())
    }
}

impl Read for Decoded {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self
            .decoder
            .as_mut()
            .ok_or_else(|| io::Error::other("zstd decoder is unavailable"))?
            .read(buf)?;
        self.at += read as u64;
        Ok(read)
    }
}

impl Seek for Decoded {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let SeekFrom::Start(target) = from else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "zstd seek is not from start",
            ));
        };
        if target < self.at {
            self.rewind()?;
        }
        let distance = target - self.at;
        if io::copy(&mut self.take(distance), &mut io::sink())? != distance {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "zstd seek past end",
            ));
        }
        Ok(self.at)
    }
}
