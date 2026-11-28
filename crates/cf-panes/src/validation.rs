//! What every request about a pane must carry, checked before anything is
//! done: the same whether the daemon sends it over the bridge or the page
//! through a command.

use crate::pty::PaneKey;

pub const MAX_INPUT_BYTES: usize = 64 * 1024;
/// The most one call from the page may carry: a paste goes in as one call,
/// whole or not at all, so this is the largest paste a window takes. The
/// page checks the same number (`MAX_PASTE_BYTES` in `terminal-link.js`) and
/// refuses a larger one before it sends anything.
pub const MAX_PAGE_INPUT_BYTES: usize = 4 * 1024 * 1024;
/// A request, or a body, no window can take.
pub(crate) const INVALID_BODY: &str = "invalid-body";
pub(crate) const MAX_TERMINAL_DIMENSION: u16 = 4096;

pub fn pane_key(id: &str, generation: u64) -> Result<PaneKey, String> {
    validate_text(id, "pane id")?;
    if generation == 0 {
        return Err("generation must be a positive integer".to_string());
    }
    Ok(PaneKey::new(id, generation))
}

pub fn validate_text(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} is required"))
    } else {
        Ok(())
    }
}

pub(crate) fn validate_input(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_INPUT_BYTES {
        Err(format!("pane input exceeds {MAX_INPUT_BYTES} bytes"))
    } else {
        Ok(())
    }
}

/// What the page's own calls carry: [`MAX_PAGE_INPUT_BYTES`].
pub(crate) fn validate_page_input(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_PAGE_INPUT_BYTES {
        Err(format!("pane input exceeds {MAX_PAGE_INPUT_BYTES} bytes"))
    } else {
        Ok(())
    }
}

pub fn validate_size(cols: u16, rows: u16) -> Result<(), String> {
    if cols == 0 || rows == 0 || cols > MAX_TERMINAL_DIMENSION || rows > MAX_TERMINAL_DIMENSION {
        Err(format!(
            "terminal size must be between 1 and {MAX_TERMINAL_DIMENSION}"
        ))
    } else {
        Ok(())
    }
}

pub fn validate_seq(seq: u64) -> Result<(), String> {
    if seq == 0 {
        Err("ack seq must be a positive integer".to_string())
    } else {
        Ok(())
    }
}

/// The bytes of a page call, from the base64 text they travel as and the length
/// the page counted. A length over the cap, text that is not the base64 of that
/// many bytes, or bytes that come out another length are refused.
pub fn decode_page_bytes(length: u64, text: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    if length > MAX_PAGE_INPUT_BYTES as u64 {
        return Err(format!("pane input exceeds {MAX_PAGE_INPUT_BYTES} bytes"));
    }
    let mismatch = || "pane input bytes do not match their length".to_string();
    if text.len() as u64 != length.div_ceil(3) * 4 {
        return Err(mismatch());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|_| "pane input bytes are not base64".to_string())?;
    if bytes.len() as u64 != length {
        return Err(mismatch());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_input_and_dimensions_are_bounded() {
        assert!(validate_input(&vec![0; MAX_INPUT_BYTES]).is_ok());
        assert!(validate_input(&vec![0; MAX_INPUT_BYTES + 1]).is_err());
        assert!(validate_size(80, 24).is_ok());
        assert!(validate_size(0, 24).is_err());
        assert!(validate_size(MAX_TERMINAL_DIMENSION + 1, 24).is_err());
        assert!(pane_key("", 1).is_err());
        assert!(pane_key("pane", 0).is_err());
    }

    fn base64_of(bytes: &[u8]) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// The text a page sends is the bytes it counted, whole: a cut, a changed
    /// or a mistaken one is refused, whatever its length claims.
    #[test]
    fn a_page_calls_bytes_are_the_length_it_stated_or_refused() {
        let bytes: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let text = base64_of(&bytes);
        assert_eq!(decode_page_bytes(1000, &text), Ok(bytes.clone()));
        assert_eq!(decode_page_bytes(0, ""), Ok(Vec::new()));
        // Cut at the end, cut at the start, a length that claims more or less.
        for (length, text) in [
            (1000, text[..text.len() - 4].to_string()),
            (1000, text[4..].to_string()),
            (1001, text.clone()),
            (999, text.clone()),
            (1000, format!("{text}AAAA")),
        ] {
            assert_eq!(
                decode_page_bytes(length, &text),
                Err("pane input bytes do not match their length".to_string()),
                "length {length}, text of {}",
                text.len()
            );
        }
        // The same count of characters, not base64.
        assert_eq!(
            decode_page_bytes(3, "!!!!"),
            Err("pane input bytes are not base64".to_string())
        );
        // Over the cap is refused before the text is read.
        assert_eq!(
            decode_page_bytes(MAX_PAGE_INPUT_BYTES as u64 + 1, ""),
            Err(format!("pane input exceeds {MAX_PAGE_INPUT_BYTES} bytes"))
        );
        let largest = vec![7u8; MAX_PAGE_INPUT_BYTES];
        assert_eq!(
            decode_page_bytes(MAX_PAGE_INPUT_BYTES as u64, &base64_of(&largest)),
            Ok(largest)
        );
    }
}
