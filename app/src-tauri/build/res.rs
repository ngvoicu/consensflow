//! A Windows resource file (`.res`) with one manifest in it, written by hand.
//!
//! The library's test binary on Windows needs its manifest embedded, and the
//! linker takes embedded resources from a `.res`. A resource compiler would
//! make one from `1 24 "tests.manifest"`, but it is a tool of Windows, and the
//! Mac runs this build script for `npm run clippy:windows -- app`. The format
//! is small (<https://learn.microsoft.com/en-us/windows/win32/menurc/resource-file-formats>):
//! a file is entries, each a header and the resource's data, and each begins on
//! a multiple of four bytes. The first is the null entry, with no data, which
//! every `.res` begins with. All of it is little-endian. A header is:
//!
//! ```text
//! offset  bytes  field
//!      0      4  DataSize         the data's length, not counting padding
//!      4      4  HeaderSize       32: this header, all of it
//!      8      4  TYPE             0xFFFF, then the type's number
//!     12      4  NAME             0xFFFF, then the resource's number
//!     16      4  DataVersion      0
//!     20      2  MemoryFlags      movable and pure; Windows ignores them
//!     22      2  LanguageId       0x0409, English (United States)
//!     24      4  Version          0
//!     28      4  Characteristics  0
//! ```
//!
//! This file is compiled twice: into the build script, which writes the
//! resource, and into the library's tests (`src/test_manifest.rs`), because
//! Cargo does not run a build script's own tests.

use std::io;

/// `RT_MANIFEST`: the type of a manifest.
pub const RT_MANIFEST: u16 = 24;
/// `CREATEPROCESS_MANIFEST_RESOURCE_ID`: the number the loader looks under for
/// the manifest of an executable.
pub const EXECUTABLE_MANIFEST: u16 = 1;
/// English (United States).
const LANGUAGE: u16 = 0x0409;
/// `MOVEABLE | PURE`. Windows ignores the flags.
const MEMORY_FLAGS: u16 = 0x0030;
/// A header's size: both numbers in it are ordinals, so it has no strings.
const HEADER_SIZE: u32 = 32;
/// Each entry begins on a multiple of this many bytes.
const ALIGNMENT: usize = 4;

/// The entry every `.res` begins with: no data, type 0 and number 0, and every
/// other field 0 but the header's size.
const NULL_ENTRY: [u8; 32] = [
    0, 0, 0, 0, // DataSize
    32, 0, 0, 0, // HeaderSize
    0xFF, 0xFF, 0, 0, // TYPE: ordinal 0
    0xFF, 0xFF, 0, 0, // NAME: ordinal 0
    0, 0, 0, 0, // DataVersion
    0, 0, // MemoryFlags
    0, 0, // LanguageId
    0, 0, 0, 0, // Version
    0, 0, 0, 0, // Characteristics
];

/// The `.res` file for `manifest`, as resource 1 of type `RT_MANIFEST`.
///
/// Fails for a manifest of 4 GiB or more, which a resource's size cannot hold.
pub fn manifest(manifest: &[u8]) -> io::Result<Vec<u8>> {
    let header = [
        data_size(manifest.len())?.to_le_bytes().as_slice(),
        &HEADER_SIZE.to_le_bytes(),
        &ordinal(RT_MANIFEST),
        &ordinal(EXECUTABLE_MANIFEST),
        &0u32.to_le_bytes(), // DataVersion
        &MEMORY_FLAGS.to_le_bytes(),
        &LANGUAGE.to_le_bytes(),
        &0u32.to_le_bytes(), // Version
        &0u32.to_le_bytes(), // Characteristics
    ]
    .concat();
    let mut file = [NULL_ENTRY.as_slice(), &header, manifest].concat();
    file.resize(file.len().next_multiple_of(ALIGNMENT), 0);
    Ok(file)
}

/// A type or a name that is a number: 0xFFFF, then the number.
fn ordinal(number: u16) -> [u8; 4] {
    let [low, high] = number.to_le_bytes();
    [0xFF, 0xFF, low, high]
}

/// A resource's length as its header holds it.
fn data_size(length: usize) -> io::Result<u32> {
    u32::try_from(length).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a resource of 4 GiB or more does not fit a .res file",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real manifest, as the build script reads it.
    const TESTS_MANIFEST: &[u8] = include_bytes!("../tests.manifest");

    /// One entry as a linker reads it: its header, and its data without padding.
    struct Entry<'a> {
        header: &'a [u8],
        data: &'a [u8],
    }

    fn double(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    fn word(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
    }

    /// The entries of a file, found as a linker finds them: each begins where
    /// the one before it ended, rounded up to a multiple of four, and the last
    /// one ends the file.
    fn entries(file: &[u8]) -> Vec<Entry<'_>> {
        let mut entries = Vec::new();
        let mut at = 0;
        while at < file.len() {
            let left = file.len() - at;
            assert!(left >= 32, "{left} bytes left over after the last entry");
            let data_size = usize::try_from(double(file, at)).unwrap();
            let header_size = usize::try_from(double(file, at + 4)).unwrap();
            assert!(
                header_size >= 32,
                "the header at {at} is {header_size} bytes"
            );
            let data_at = at + header_size;
            assert!(
                data_at + data_size <= file.len(),
                "the entry at {at} runs past the end of the file"
            );
            entries.push(Entry {
                header: &file[at..data_at],
                data: &file[data_at..data_at + data_size],
            });
            at = (data_at + data_size).next_multiple_of(4);
        }
        assert_eq!(
            at,
            file.len(),
            "the last entry ends the file, padding included"
        );
        entries
    }

    #[test]
    fn the_file_begins_with_the_null_entry() {
        let file = manifest(b"<assembly/>").unwrap();
        assert_eq!(
            file[..32],
            [
                0x00, 0x00, 0x00, 0x00, // DataSize
                0x20, 0x00, 0x00, 0x00, // HeaderSize
                0xFF, 0xFF, 0x00, 0x00, // TYPE: ordinal 0
                0xFF, 0xFF, 0x00, 0x00, // NAME: ordinal 0
                0x00, 0x00, 0x00, 0x00, // DataVersion
                0x00, 0x00, // MemoryFlags
                0x00, 0x00, // LanguageId
                0x00, 0x00, 0x00, 0x00, // Version
                0x00, 0x00, 0x00, 0x00, // Characteristics
            ]
        );
    }

    #[test]
    fn the_manifest_is_resource_1_of_type_24() {
        let file = manifest(TESTS_MANIFEST).unwrap();
        let entries = entries(&file);
        assert_eq!(entries.len(), 2, "the null entry, then the manifest");
        let header = entries[1].header;
        assert_eq!(header.len(), 32);
        assert_eq!(double(header, 4), 32, "HeaderSize");
        assert_eq!(header[8..12], [0xFF, 0xFF, 24, 0], "TYPE: RT_MANIFEST");
        assert_eq!(header[12..16], [0xFF, 0xFF, 1, 0], "NAME: 1");
        assert_eq!(double(header, 16), 0, "DataVersion");
        assert_eq!(word(header, 20), 0x0030, "MemoryFlags");
        assert_eq!(word(header, 22), 0x0409, "LanguageId");
        assert_eq!(double(header, 24), 0, "Version");
        assert_eq!(double(header, 28), 0, "Characteristics");
    }

    #[test]
    fn the_data_size_is_the_manifests_length_and_not_the_padded_one() {
        for length in 0..=9 {
            let file = manifest(&vec![b'm'; length]).unwrap();
            assert_eq!(
                double(&file, 32),
                u32::try_from(length).unwrap(),
                "{length} bytes"
            );
        }
    }

    #[test]
    fn the_data_is_padded_with_zeros_to_a_multiple_of_four() {
        // Every remainder of four twice, and 0 bytes: how many zeros follow.
        let padded = [
            (0, 0),
            (1, 3),
            (2, 2),
            (3, 1),
            (4, 0),
            (5, 3),
            (6, 2),
            (7, 1),
            (8, 0),
        ];
        for (length, padding) in padded {
            let data = vec![b'm'; length];
            let file = manifest(&data).unwrap();
            assert_eq!(file.len(), 64 + length + padding, "{length} bytes");
            assert_eq!(file[64..64 + length], data, "{length} bytes: the data");
            assert_eq!(
                file[64 + length..],
                vec![0; padding],
                "{length} bytes: the padding"
            );
        }
    }

    #[test]
    fn the_file_is_the_null_entry_and_the_manifest_and_nothing_else() {
        for length in 0..=9 {
            let file = manifest(&vec![b'm'; length]).unwrap();
            let entries = entries(&file);
            assert_eq!(entries.len(), 2, "{length} bytes");
            assert_eq!(entries[0].data, b"", "{length} bytes: the null entry");
            assert_eq!(entries[1].data.len(), length, "{length} bytes");
        }
    }

    #[test]
    fn the_real_manifest_goes_in_whole() {
        let file = manifest(TESTS_MANIFEST).unwrap();
        assert_eq!(entries(&file)[1].data, TESTS_MANIFEST);
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn a_resource_too_big_for_its_size_field_is_refused() {
        let biggest = usize::try_from(u32::MAX).unwrap();
        assert_eq!(data_size(biggest).unwrap(), u32::MAX);
        let error = data_size(biggest + 1).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
