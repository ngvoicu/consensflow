//! The commands a text typed into a window teaches, as the window's own `cf` starts them.

/// Whether `text` names the command `command` (`task accept T-1`, `answer m-3`) as a
/// window is taught it: started by its own `cf` by its whole path (T-1049), which ends
/// `cf` on macOS and `cf.exe` on Windows, or by the word `cf` where none is given.
pub fn names_command(text: &str, command: &str) -> bool {
    ["cf", "cf.exe"]
        .iter()
        .any(|word| text.contains(&format!("{word} {command}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_is_named_by_the_bare_word_or_by_the_whole_path_of_either_platforms_cf() {
        assert!(names_command(
            "Decide with: cf task accept T-1",
            "task accept T-1"
        ));
        assert!(names_command(
            "Decide with: /opt/consensflow/bin/cf task accept T-1 · …",
            "task accept T-1"
        ));
        assert!(names_command(
            "Decide with: D:/a/consensflow/app/src-tauri/target/release/cf.exe task accept T-1",
            "task accept T-1"
        ));
        assert!(!names_command(
            "Decide with: cf task accept T-2",
            "task accept T-1"
        ));
        assert!(!names_command("Run: cfx answer m-3", "answer m-3"));
    }
}
