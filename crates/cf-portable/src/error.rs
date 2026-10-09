//! Why a portable exe could not be packed, read or unpacked.

use std::io;
use std::path::PathBuf;

/// Why a portable exe could not be packed, read or unpacked.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The footer names more payload than the file holds, or less than any gzip
    /// stream: the exe is damaged, not one that carries nothing.
    #[error("its footer names {length} bytes of payload in a file of {size}")]
    Footer { length: u64, size: u64 },
    /// The file does not end with the tag: it carries no runtime.
    #[error("{} does not end with the portable footer", path.display())]
    NoFooter { path: PathBuf },
    /// A link, a device or anything else that is not a plain file or folder:
    /// all a payload holds, so the reader takes no other and the packer writes
    /// no other. `path` is the entry's, as the payload names it.
    #[error("{path} is not a plain file or folder, which is all a payload holds")]
    NotPlain { path: String },
    /// An entry of the payload whose path is absolute or climbs out of the
    /// folder it unpacks into with `..`.
    #[error("the payload holds {path}, which is outside the folder it unpacks into")]
    Outside { path: String },
    /// A piece of the build is not where it is packed from. `piece` is its
    /// path from `folder`, written as the system writes one.
    #[error(
        "{} is missing from {}; build first with npm --prefix app run build",
        piece.display(),
        folder.display()
    )]
    Missing { piece: PathBuf, folder: PathBuf },
    /// A named file or folder could not be opened, made, read or written.
    #[error("could not {what} {}: {cause}", path.display())]
    Files {
        what: &'static str,
        path: PathBuf,
        cause: io::Error,
    },
    /// Reading the exe, or the gzip stream or the tar in it, failed: a payload
    /// that does not match its CRC ends here.
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl From<Error> for io::Error {
    /// For a caller that speaks `io::Result`, as the app's unpacking does: an
    /// I/O failure stays what it was, and anything else is data that is not as
    /// a payload should be, with the same words.
    fn from(error: Error) -> Self {
        match error {
            Error::Io(cause) => cause,
            other => io::Error::new(io::ErrorKind::InvalidData, other),
        }
    }
}

/// A [`Error::Files`] for `path`, to map an `io::Error` with: `.map_err(files("open", path))`.
pub(crate) fn files<'a>(
    what: &'static str,
    path: &'a std::path::Path,
) -> impl FnOnce(io::Error) -> Error + 'a {
    move |cause| Error::Files {
        what,
        path: path.to_path_buf(),
        cause,
    }
}
