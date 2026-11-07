use std::io::Cursor;

use super::*;

/// A PNG chunk: its length, its type, its data and a checksum that is not read.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = (data.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(data);
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes
}

/// The header chunk of a picture `width` by `height`, of the colour type given.
fn ihdr(width: u32, height: u32, colour_type: u8) -> Vec<u8> {
    let mut data = width.to_be_bytes().to_vec();
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&[8, colour_type, 0, 0, 0]);
    chunk(b"IHDR", &data)
}

/// A PNG of `colour_type` with `between` chunks after its header and before its pixels.
fn png(width: u32, height: u32, colour_type: u8, between: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend(ihdr(width, height, colour_type));
    for chunk in between {
        bytes.extend_from_slice(chunk);
    }
    bytes.extend(chunk(b"IDAT", &[1, 2, 3, 4]));
    bytes.extend(chunk(b"IEND", &[]));
    bytes
}

fn facts(bytes: Vec<u8>) -> Option<Facts> {
    facts_of(&mut Cursor::new(bytes))
}

fn the(width: u32, height: u32, kind: Kind, alpha: bool) -> Option<Facts> {
    Some(Facts {
        width,
        height,
        kind,
        alpha,
    })
}

#[test]
fn a_png_with_an_alpha_channel_says_its_size_and_alpha() {
    assert_eq!(
        facts(png(1280, 720, 6, &[])),
        the(1280, 720, Kind::Png, true),
        "RGBA"
    );
    assert_eq!(
        facts(png(64, 32, 4, &[])),
        the(64, 32, Kind::Png, true),
        "grey with alpha"
    );
}

#[test]
fn a_png_with_none_says_so_and_width_is_not_height() {
    assert_eq!(
        facts(png(1672, 941, 2, &[])),
        the(1672, 941, Kind::Png, false),
        "RGB"
    );
    assert_eq!(
        facts(png(10, 20, 0, &[])),
        the(10, 20, Kind::Png, false),
        "grey"
    );
    assert_eq!(
        facts(png(10, 20, 3, &[chunk(b"PLTE", &[0; 6])])),
        the(10, 20, Kind::Png, false),
        "a palette with no transparency"
    );
}

#[test]
fn a_transparency_chunk_before_the_pixels_is_alpha_in_a_palette_a_grey_and_a_colour_picture() {
    for colour_type in [3, 0, 2] {
        let between = [chunk(b"PLTE", &[0; 6]), chunk(b"tRNS", &[0, 255])];
        assert_eq!(
            facts(png(7, 9, colour_type, &between)),
            the(7, 9, Kind::Png, true),
            "colour type {colour_type} with tRNS"
        );
    }
}

#[test]
fn a_transparency_chunk_is_found_past_a_chunk_of_a_hundred_thousand_bytes() {
    // Codex's pictures carry a provenance chunk (`caBX`) of their own before the pixels.
    let between = [chunk(b"caBX", &vec![7; 100_000]), chunk(b"tRNS", &[0])];
    assert_eq!(facts(png(5, 5, 3, &between)), the(5, 5, Kind::Png, true));
    // And a picture without one has none however long the chunk before its pixels is.
    assert_eq!(
        facts(png(5, 5, 3, &[chunk(b"caBX", &vec![7; 100_000])])),
        the(5, 5, Kind::Png, false)
    );
}

#[test]
fn a_transparency_chunk_after_the_pixels_is_none_of_the_picture_s() {
    let mut bytes = png(5, 5, 2, &[]);
    bytes.extend(chunk(b"tRNS", &[0]));
    assert_eq!(facts(bytes), the(5, 5, Kind::Png, false));
}

/// A JPEG whose frame header (`marker`) is behind an application segment of `app` bytes.
fn jpeg(marker: u8, width: u16, height: u16, app: usize) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8];
    bytes.extend_from_slice(&[0xFF, 0xE1]);
    bytes.extend_from_slice(&((app + 2) as u16).to_be_bytes());
    bytes.extend(vec![0; app]);
    bytes.extend_from_slice(&[0xFF, 0xC4, 0, 4, 0, 0]);
    bytes.extend_from_slice(&[0xFF, marker, 0, 17, 8]);
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    bytes.extend_from_slice(&[0xFF, 0xDA, 0, 2]);
    bytes
}

#[test]
fn a_jpeg_says_its_size_past_segments_before_its_frame_and_has_no_alpha() {
    assert_eq!(
        facts(jpeg(0xC0, 800, 600, 10)),
        the(800, 600, Kind::Jpeg, false),
        "baseline"
    );
    assert_eq!(
        facts(jpeg(0xC2, 1920, 1080, 60_000)),
        the(1920, 1080, Kind::Jpeg, false),
        "progressive, behind a 60 KB segment"
    );
}

/// A WebP file of the chunks given.
fn webp(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut body = b"WEBP".to_vec();
    for (kind, data) in chunks {
        body.extend_from_slice(*kind);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        if data.len() % 2 == 1 {
            body.push(0);
        }
    }
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes.extend(body);
    bytes
}

/// The data of a lossy frame `width` by `height`.
fn lossy(width: u16, height: u16) -> Vec<u8> {
    let mut data = vec![0x10, 0x02, 0x00, 0x9D, 0x01, 0x2A];
    data.extend_from_slice(&width.to_le_bytes());
    data.extend_from_slice(&height.to_le_bytes());
    data.extend_from_slice(&[0; 8]);
    data
}

/// The data of a lossless frame `width` by `height`, with the bit that says it has alpha as given.
fn lossless(width: u32, height: u32, alpha: bool) -> Vec<u8> {
    let bits = (width - 1) | ((height - 1) << 14) | (u32::from(alpha) << 28);
    let mut data = vec![0x2F];
    data.extend_from_slice(&bits.to_le_bytes());
    data.extend_from_slice(&[0; 8]);
    data
}

/// The data of the extended header: canvas `width` by `height`, with the flags given.
fn extended(width: u32, height: u32, flags: u8) -> Vec<u8> {
    let mut data = vec![flags, 0, 0, 0];
    data.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
    data.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
    data
}

#[test]
fn a_webp_says_its_size_and_whether_it_has_alpha_in_each_of_its_three_forms() {
    assert_eq!(
        facts(webp(&[(b"VP8 ", lossy(640, 480))])),
        the(640, 480, Kind::Webp, false),
        "lossy"
    );
    assert_eq!(
        facts(webp(&[(b"VP8L", lossless(300, 200, true))])),
        the(300, 200, Kind::Webp, true),
        "lossless with alpha"
    );
    assert_eq!(
        facts(webp(&[(b"VP8L", lossless(300, 200, false))])),
        the(300, 200, Kind::Webp, false),
        "lossless without"
    );
    assert_eq!(
        facts(webp(&[
            (b"VP8X", extended(4000, 3000, 0x10)),
            (b"VP8 ", lossy(1, 1))
        ])),
        the(4000, 3000, Kind::Webp, true),
        "extended, flagged for alpha: the canvas is the size, not the frame"
    );
    assert_eq!(
        facts(webp(&[
            (b"VP8X", extended(4000, 3000, 0x00)),
            (b"VP8 ", lossy(1, 1))
        ])),
        the(4000, 3000, Kind::Webp, false),
        "extended, not"
    );
}

#[test]
fn what_is_cut_inside_its_header_or_is_no_image_has_no_facts() {
    let whole = png(1280, 720, 6, &[]);
    for cut in [0, 7, 8, 12, 20, 25] {
        assert_eq!(facts(whole[..cut].to_vec()), None, "a PNG cut at {cut}");
    }
    assert_eq!(
        facts(b"just some words, not a picture at all".to_vec()),
        None
    );
    assert_eq!(facts(Vec::new()), None, "empty");
    assert_eq!(facts(png(0, 720, 6, &[])), None, "a picture of no width");
    assert_eq!(
        facts(jpeg(0xC0, 800, 600, 10)[..30].to_vec()),
        None,
        "a JPEG cut"
    );
    assert_eq!(
        facts(webp(&[(b"VP8 ", lossy(64, 64))])[..14].to_vec()),
        None,
        "a WebP cut"
    );
}

#[test]
fn the_type_is_the_content_s_and_never_the_name_s() {
    let dir = tempfile::tempdir().unwrap();
    let named_png = dir.path().join("kite.png");
    std::fs::write(&named_png, jpeg(0xC0, 800, 600, 10)).unwrap();
    assert_eq!(read(&named_png), the(800, 600, Kind::Jpeg, false));
    let gone = dir.path().join("gone.png");
    assert_eq!(read(&gone), None, "a path that is gone");
    assert_eq!(read(dir.path()), None, "a folder");
}

#[test]
fn a_file_is_read_no_further_than_the_bound() {
    // Pixels of no end after a header whose frame is past the bound: not looked for.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("far.jpg");
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE1];
    bytes.extend_from_slice(&60_000u16.to_be_bytes());
    bytes.extend(vec![0; 59_998]);
    // Segments of 60,000 bytes until the bound is past, then a frame.
    while (bytes.len() as u64) < MOST_BYTES + 100_000 {
        bytes.extend_from_slice(&[0xFF, 0xE1]);
        bytes.extend_from_slice(&60_000u16.to_be_bytes());
        bytes.extend(vec![0; 59_998]);
    }
    bytes.extend_from_slice(&[
        0xFF, 0xC0, 0, 17, 8, 0, 10, 0, 10, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1,
    ]);
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(read(&path), None);
}

#[test]
fn the_files_a_result_names_are_the_absolute_paths_of_images_in_order_each_once() {
    let words = "/Users/me/proj/images/kite.png\n\
        `/Users/me/proj/images/logo.webp`\n\
        - /Users/me/My Project/images/photo.JPG\n\
        1. C:\\Users\\me\\proj\\images\\icon.png\n\
        D:/art/sky.jpeg\n\
        /Users/me/proj/images/kite.png\n\
        A kite over a hill, saved as /Users/me/proj/images/hill.png, and its notes in /Users/me/proj/notes.txt.\n\
        images/relative.png\n\
        a path and/or a word.png";
    assert_eq!(
        files_named(words),
        [
            "/Users/me/proj/images/kite.png",
            "/Users/me/proj/images/logo.webp",
            "/Users/me/My Project/images/photo.JPG",
            "C:\\Users\\me\\proj\\images\\icon.png",
            "D:/art/sky.jpeg",
            "/Users/me/proj/images/hill.png",
        ]
    );
}

#[test]
fn no_more_than_the_most_files_get_a_line() {
    let words: String = (0..40).map(|n| format!("/a/{n}.png\n")).collect();
    assert_eq!(files_named(&words).len(), MOST_FILES);
}

#[test]
fn a_line_says_the_path_the_size_the_type_and_the_alpha_after_words() {
    let dir = tempfile::tempdir().unwrap();
    let kite = dir.path().join("kite.png");
    let sky = dir.path().join("sky.jpg");
    std::fs::write(&kite, png(1280, 720, 6, &[])).unwrap();
    std::fs::write(&sky, jpeg(0xC0, 800, 600, 10)).unwrap();
    let words = format!("{}\n{}\nA kite over a sky.", kite.display(), sky.display());
    assert_eq!(
        lines_for(&words),
        format!(
            "(ConsensFlow read {}: 1280x720 pixels, PNG, with alpha)\n(ConsensFlow read {}: 800x600 pixels, JPEG, no alpha)",
            kite.display(),
            sky.display()
        )
    );
    assert_eq!(
        with_images(words.clone()),
        format!("{words}\n\n{}", lines_for(&words))
    );
}

#[test]
fn a_file_that_is_gone_or_is_no_image_gets_no_line_and_a_result_naming_none_is_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.png");
    let fake = dir.path().join("fake.png");
    std::fs::write(&real, png(16, 16, 2, &[])).unwrap();
    std::fs::write(&fake, b"not a picture").unwrap();
    let gone = dir.path().join("gone.png");
    let words = format!(
        "{}\n{}\n{}\nThree.",
        real.display(),
        fake.display(),
        gone.display()
    );
    assert_eq!(
        lines_for(&words),
        format!(
            "(ConsensFlow read {}: 16x16 pixels, PNG, no alpha)",
            real.display()
        )
    );
    let none = "Drawn: a kite.".to_owned();
    assert_eq!(lines_for(&none), "");
    assert_eq!(with_images(none.clone()), none);
    assert_eq!(
        with_images(format!("{}\n{}", fake.display(), gone.display())),
        format!("{}\n{}", fake.display(), gone.display())
    );
}
