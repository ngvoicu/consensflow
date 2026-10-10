//! A PNG file's structure, read without its pixels: enough to say that what a
//! harness's image tool saved is a whole PNG, and how large its picture is. The
//! signature first, then each chunk by its length, its type and its checksum,
//! from the header chunk to the end chunk. A file that was cut short, one with
//! a chunk altered, and another kind of image under a `.png` name (a JPEG, a
//! WebP) are all told apart from a PNG. The pixels are not inflated: a checksum
//! that holds says the bytes are as the encoder wrote them.

/// How large a PNG's picture is, in pixels, as its header says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub width: u32,
    pub height: u32,
}

/// What every PNG starts with.
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The header chunk's data is 13 bytes: width, height, bit depth, colour type,
/// compression, filter and interlace.
const HEADER_LENGTH: usize = 13;

/// The most a side of a picture may be: what the specification allows.
const MOST: u32 = i32::MAX.unsigned_abs();

/// The checksum's table, for the polynomial the specification gives.
const TABLE: [u32; 256] = {
    let mut table = [0_u32; 256];
    let mut at = 0;
    while at < 256 {
        let mut value = at as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                0xEDB8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[at] = value;
        at += 1;
    }
    table
};

/// The CRC-32 the specification checksums each chunk with.
fn crc(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(u32::MAX, |crc, byte| {
        TABLE[usize::from((crc as u8) ^ byte)] ^ (crc >> 8)
    })
}

/// A big-endian number from the first four bytes of `bytes`.
fn be_u32(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?))
}

/// A chunk read off the start of a file: its type and its data, and what
/// follows it.
struct Chunk<'a> {
    kind: [u8; 4],
    data: &'a [u8],
    after: &'a [u8],
}

/// One chunk read off the start of `bytes`. A chunk that ends the file early or
/// whose checksum does not hold is the reason it is none.
fn chunk(bytes: &[u8]) -> Result<Chunk<'_>, String> {
    let length = be_u32(bytes)
        .and_then(|length| usize::try_from(length).ok())
        .ok_or("the file ends inside a chunk's length")?;
    // The length, the type, the data and the checksum.
    let Some(end) = length.checked_add(12).filter(|end| *end <= bytes.len()) else {
        return Err(format!(
            "the file ends inside a chunk of {length} bytes: it holds {} after the length",
            bytes.len().saturating_sub(4)
        ));
    };
    let (covered, sum) = bytes[4..end].split_at(4 + length);
    let (kind, data) = covered.split_at(4);
    let kind: [u8; 4] = kind.try_into().map_err(|_| "a chunk has no type")?;
    if be_u32(sum) != Some(crc(covered)) {
        return Err(format!(
            "the chunk {} has a checksum that does not hold: the file was altered or cut",
            String::from_utf8_lossy(&kind)
        ));
    }
    Ok(Chunk {
        kind,
        data,
        after: &bytes[end..],
    })
}

/// The header a chunk's data holds, if what it says is something a picture can
/// be: sides of the sizes the specification allows, a colour type it names with
/// a bit depth that colour type has, and the one compression, filter and
/// interlace methods it defines.
fn header(data: &[u8]) -> Result<Header, String> {
    if data.len() != HEADER_LENGTH {
        return Err(format!(
            "the header chunk is {} bytes, not {HEADER_LENGTH}",
            data.len()
        ));
    }
    let (width, height) = (be_u32(data).unwrap_or(0), be_u32(&data[4..]).unwrap_or(0));
    let (bit_depth, colour_type) = (data[8], data[9]);
    if width == 0 || height == 0 || width > MOST || height > MOST {
        return Err(format!("its picture is {width} by {height} pixels"));
    }
    let depths: &[u8] = match colour_type {
        0 => &[1, 2, 4, 8, 16],
        3 => &[1, 2, 4, 8],
        2 | 4 | 6 => &[8, 16],
        _ => return Err(format!("its colour type is {colour_type}, which is none")),
    };
    if !depths.contains(&bit_depth) {
        return Err(format!(
            "a bit depth of {bit_depth} is none of colour type {colour_type}'s"
        ));
    }
    let (compression, filter, interlace) = (data[10], data[11], data[12]);
    if compression != 0 || filter != 0 || interlace > 1 {
        return Err(format!(
            "its compression, filter and interlace are {compression}, {filter}, {interlace}"
        ));
    }
    Ok(Header { width, height })
}

/// What `bytes` is, if it is a whole PNG: its header. Otherwise why it is not,
/// in a sentence: it starts another way (a JPEG, a WebP, text), a chunk is cut
/// or altered, the picture has no pixels or no end chunk, or something follows
/// the end.
pub fn inspect(bytes: &[u8]) -> Result<Header, String> {
    let Some(mut rest) = bytes.strip_prefix(&SIGNATURE) else {
        let start: Vec<String> = bytes.iter().take(8).map(|b| format!("{b:02x}")).collect();
        return Err(format!(
            "it is no PNG: it starts with {}, not a PNG's signature",
            start.join(" ")
        ));
    };
    let mut found: Option<Header> = None;
    let mut pixels = false;
    while !rest.is_empty() {
        let Chunk { kind, data, after } = chunk(rest)?;
        rest = after;
        match (&kind, found) {
            (b"IHDR", None) => found = Some(header(data)?),
            (_, None) => return Err("its first chunk is not the header".to_owned()),
            (b"IHDR", Some(_)) => return Err("it has a second header chunk".to_owned()),
            (b"IDAT", Some(_)) => pixels = true,
            (b"IEND", Some(whole)) => {
                if !data.is_empty() {
                    return Err("its end chunk holds data".to_owned());
                }
                if !pixels {
                    return Err("it ends with no pixels in it".to_owned());
                }
                if !rest.is_empty() {
                    return Err(format!("{} bytes follow its end chunk", rest.len()));
                }
                return Ok(whole);
            }
            _ => {}
        }
    }
    Err("it has no end chunk: the file was cut short".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::checkout;

    /// A chunk as an encoder writes it.
    fn written(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut chunk = u32::try_from(data.len()).unwrap().to_be_bytes().to_vec();
        chunk.extend_from_slice(kind);
        chunk.extend_from_slice(data);
        let covered: Vec<u8> = chunk[4..].to_vec();
        chunk.extend_from_slice(&crc(&covered).to_be_bytes());
        chunk
    }

    fn header_data(width: u32, height: u32, depth: u8, colour: u8) -> Vec<u8> {
        let mut data = width.to_be_bytes().to_vec();
        data.extend_from_slice(&height.to_be_bytes());
        data.extend_from_slice(&[depth, colour, 0, 0, 0]);
        data
    }

    /// A PNG of the given chunks after its signature.
    fn png(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut file = SIGNATURE.to_vec();
        file.extend(chunks.iter().flatten());
        file
    }

    /// The smallest whole PNG there is, 3 by 2 pixels (its pixels are not real:
    /// nothing here inflates them).
    fn whole() -> Vec<u8> {
        png(&[
            written(b"IHDR", &header_data(3, 2, 8, 6)),
            written(b"IDAT", &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]),
            written(b"IEND", &[]),
        ])
    }

    #[test]
    fn the_checksum_is_the_one_the_specification_gives() {
        // The check value of CRC-32, and the checksum every PNG's end chunk carries.
        assert_eq!(crc(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc(b"IEND"), 0xAE42_6082);
        assert_eq!(whole()[whole().len() - 4..], 0xAE42_6082_u32.to_be_bytes());
    }

    #[test]
    fn a_whole_png_says_how_large_its_picture_is() {
        assert_eq!(
            inspect(&whole()),
            Ok(Header {
                width: 3,
                height: 2
            })
        );
    }

    #[test]
    fn the_pngs_of_the_repository_are_whole_and_as_large_as_their_names_say() {
        for (name, side) in [("32x32", 32), ("64x64", 64), ("128x128", 128)] {
            let file = checkout::path(&format!("app/src-tauri/icons/{name}.png"));
            let bytes = std::fs::read(&file).unwrap();
            let read = inspect(&bytes).unwrap();
            assert_eq!((read.width, read.height), (side, side), "{name}");
        }
    }

    #[test]
    fn a_file_that_is_another_kind_of_image_or_no_image_is_no_png() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F'];
        let said = inspect(&jpeg).unwrap_err();
        assert!(
            said.starts_with("it is no PNG: it starts with ff d8 ff e0"),
            "{said}"
        );
        assert!(inspect(b"").unwrap_err().starts_with("it is no PNG"));
        assert!(inspect(b"RIFF....WEBP")
            .unwrap_err()
            .contains("52 49 46 46"));
    }

    #[test]
    fn a_file_cut_anywhere_is_told_apart_from_a_whole_one() {
        let file = whole();
        for length in 0..file.len() {
            assert!(inspect(&file[..length]).is_err(), "cut at {length}");
        }
    }

    #[test]
    fn a_chunk_with_a_byte_altered_has_a_checksum_that_does_not_hold() {
        let mut file = whole();
        // A byte of the header's width.
        file[SIGNATURE.len() + 8] ^= 1;
        let said = inspect(&file).unwrap_err();
        assert!(said.starts_with("the chunk IHDR has a checksum"), "{said}");
    }

    #[test]
    fn a_picture_with_no_pixels_and_one_with_no_end_are_refused() {
        let header = written(b"IHDR", &header_data(3, 2, 8, 6));
        let no_pixels = png(&[header.clone(), written(b"IEND", &[])]);
        assert_eq!(
            inspect(&no_pixels),
            Err("it ends with no pixels in it".to_owned())
        );
        let no_end = png(&[header, written(b"IDAT", &[1, 2, 3])]);
        assert_eq!(
            inspect(&no_end),
            Err("it has no end chunk: the file was cut short".to_owned())
        );
    }

    #[test]
    fn a_header_that_is_not_first_or_that_describes_no_picture_is_refused() {
        let idat = written(b"IDAT", &[1]);
        let end = written(b"IEND", &[]);
        let misplaced = png(&[idat.clone(), end.clone()]);
        assert_eq!(
            inspect(&misplaced),
            Err("its first chunk is not the header".to_owned())
        );
        for (data, said) in [
            (header_data(0, 2, 8, 6), "0 by 2 pixels"),
            (header_data(3, 0, 8, 6), "3 by 0 pixels"),
            (header_data(3, 2, 8, 5), "colour type is 5"),
            (
                header_data(3, 2, 4, 6),
                "a bit depth of 4 is none of colour type 6's",
            ),
            (header_data(u32::MAX, 2, 8, 6), "pixels"),
            (vec![0; 12], "12 bytes, not 13"),
        ] {
            let file = png(&[written(b"IHDR", &data), idat.clone(), end.clone()]);
            let refused = inspect(&file).unwrap_err();
            assert!(refused.contains(said), "{said}: {refused}");
        }
    }

    #[test]
    fn something_after_the_end_chunk_is_refused() {
        let mut file = whole();
        file.extend_from_slice(b"junk");
        assert_eq!(
            inspect(&file),
            Err("4 bytes follow its end chunk".to_owned())
        );
    }
}
