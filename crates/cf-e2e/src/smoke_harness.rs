//! The packaged smoke's stand-in harness (`fixtures/smoke-harness.sh`), the
//! `claude` every window of the smoke runs, its flood sized. The smoke runs it in
//! the built app; the rig runs it too (`tests/rig/smoke_stand_in.rs`), on the
//! checkout's daemon and pane host, so that what the daemon asks of a Claude
//! window before it hands it a message is asked of this script on every gate,
//! and not only by the smoke, which needs a built app (T-863).

/// The script, with where its flood's size goes marked.
const SCRIPT: &str = include_str!("../fixtures/smoke-harness.sh");

/// The script, its flood `lines` long and each of its lines `width` wide.
pub fn script(lines: usize, width: usize) -> String {
    SCRIPT
        .replace("@FLOOD_LINES@", &lines.to_string())
        .replace("@FLOOD_WIDTH@", &width.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_has_its_flood_sized_and_nothing_left_marked() {
        let script = script(7, 9);
        assert!(!script.contains('@'), "{script}");
        assert!(script.contains("-le 7 ]"));
        assert!(script.contains("-lt 9 ]"));
    }
}
