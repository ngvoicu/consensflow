//! What every request about a pane must carry, checked before anything is
//! done: the same whether the daemon sends it over the bridge or the page
//! through a command.

use crate::pty::PaneKey;

pub(crate) const MAX_INPUT_BYTES: usize = 64 * 1024;
/// A request, or a body, no window can take.
pub(crate) const INVALID_BODY: &str = "invalid-body";
const MAX_TERMINAL_DIMENSION: u16 = 4096;

pub(crate) fn pane_key(id: &str, generation: u64) -> Result<PaneKey, String> {
    validate_text(id, "pane id")?;
    if generation == 0 {
        return Err("generation must be a positive integer".to_string());
    }
    Ok(PaneKey::new(id, generation))
}

pub(crate) fn validate_text(value: &str, label: &str) -> Result<(), String> {
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

pub(crate) fn validate_size(cols: u16, rows: u16) -> Result<(), String> {
    if cols == 0 || rows == 0 || cols > MAX_TERMINAL_DIMENSION || rows > MAX_TERMINAL_DIMENSION {
        Err(format!(
            "terminal size must be between 1 and {MAX_TERMINAL_DIMENSION}"
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn validate_seq(seq: u64) -> Result<(), String> {
    if seq == 0 {
        Err("ack seq must be a positive integer".to_string())
    } else {
        Ok(())
    }
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
}
