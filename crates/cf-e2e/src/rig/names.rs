//! How Claude Code names a folder of the run on disk.

/// The name Claude Code gives a folder under its `projects`: the path with
/// every character that is not a letter or a digit made a dash.
pub fn project_name(path: &str) -> String {
    path.chars()
        .map(|at| if at.is_ascii_alphanumeric() { at } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_names_a_folder_by_its_path_with_dashes_for_what_is_no_letter_or_digit() {
        assert_eq!(
            project_name(
                "/private/var/folders/ab/x_y/T/consensflow-integration-Q1/workspace/p1-claude"
            ),
            "-private-var-folders-ab-x-y-T-consensflow-integration-Q1-workspace-p1-claude"
        );
        assert_eq!(project_name("/a.b"), "-a-b");
        assert_eq!(
            project_name(r"C:\Users\me\.consensflow-candidate"),
            "C--Users-me--consensflow-candidate"
        );
        assert_eq!(project_name(""), "");
    }
}
