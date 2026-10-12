//! What the tests of the parts of this module have in common.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// `words` as the owned words the options and a command are made of.
pub fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// The script that `encoded` is, as PowerShell's `-EncodedCommand` reads it: the
/// base64 of its UTF-16.
pub fn script_of(encoded: &str) -> String {
    let bytes = STANDARD.decode(encoded).unwrap();
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).unwrap()
}
