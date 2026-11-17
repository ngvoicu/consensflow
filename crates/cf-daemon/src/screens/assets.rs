//! The same bundled font stylesheet and files the board uses. The daemon serves
//! only these fixed public assets, never a path read from the filesystem.

use crate::api::answer::Answer;

macro_rules! font {
    ($name:literal) => {
        include_bytes!(concat!("../../../../app/ui/vendor/fonts/", $name)) as &'static [u8]
    };
}

/// A bundled font asset at its board-relative path, or none for any other path.
pub(super) fn get(path: &str) -> Option<Answer> {
    let name = path.strip_prefix("/vendor/fonts/")?;
    let (content_type, bytes) = match name {
        "fonts.css" => ("text/css; charset=utf-8", font!("fonts.css")),
        "geist-cyrillic-ext-wght-normal.woff2" => {
            ("font/woff2", font!("geist-cyrillic-ext-wght-normal.woff2"))
        }
        "geist-cyrillic-wght-normal.woff2" => {
            ("font/woff2", font!("geist-cyrillic-wght-normal.woff2"))
        }
        "geist-vietnamese-wght-normal.woff2" => {
            ("font/woff2", font!("geist-vietnamese-wght-normal.woff2"))
        }
        "geist-latin-ext-wght-normal.woff2" => {
            ("font/woff2", font!("geist-latin-ext-wght-normal.woff2"))
        }
        "geist-latin-wght-normal.woff2" => ("font/woff2", font!("geist-latin-wght-normal.woff2")),
        "geist-mono-cyrillic-ext-wght-normal.woff2" => (
            "font/woff2",
            font!("geist-mono-cyrillic-ext-wght-normal.woff2"),
        ),
        "geist-mono-cyrillic-wght-normal.woff2" => {
            ("font/woff2", font!("geist-mono-cyrillic-wght-normal.woff2"))
        }
        "geist-mono-symbols2-wght-normal.woff2" => {
            ("font/woff2", font!("geist-mono-symbols2-wght-normal.woff2"))
        }
        "geist-mono-vietnamese-wght-normal.woff2" => (
            "font/woff2",
            font!("geist-mono-vietnamese-wght-normal.woff2"),
        ),
        "geist-mono-latin-ext-wght-normal.woff2" => (
            "font/woff2",
            font!("geist-mono-latin-ext-wght-normal.woff2"),
        ),
        "geist-mono-latin-wght-normal.woff2" => {
            ("font/woff2", font!("geist-mono-latin-wght-normal.woff2"))
        }
        _ => return None,
    };
    Some(Answer::asset(content_type, bytes))
}
