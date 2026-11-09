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

    /// The models the page's self-test opens its project with (`app/ui/selftest.js`, the
    /// `project.open` it asks the daemon): the chief's and each worker's.
    fn models_the_selftest_asks_for() -> Vec<String> {
        let page = std::fs::read_to_string(crate::checkout::path("app/ui/selftest.js")).unwrap();
        let from = page
            .find("daemon('project.open'")
            .expect("the self-test opens a project");
        let call = &page[from..];
        let call = &call[..call.find("})\n").expect("the call ends")];
        let model = regex::Regex::new(r"model: '([^']+)'").unwrap();
        model
            .captures_iter(call)
            .map(|found| found[1].to_owned())
            .collect()
    }

    /// The models the script lists when the daemon reads them (its `for model in …` line).
    fn models_the_script_lists() -> Vec<String> {
        let line = SCRIPT
            .lines()
            .find(|line| line.trim_start().starts_with("for model in "))
            .expect("the script lists models");
        let words = line.trim_start().strip_prefix("for model in ").unwrap();
        words
            .trim_end_matches("; do")
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    }

    /// The daemon takes no model its harness does not list (A4-28, T-1329): the page's self-test
    /// opens its project in the built app on this script, so the script lists the models the
    /// self-test names, or `project.open` is refused and the smoke is red on a built app and
    /// nowhere else. (The rig runs the script too, with the models its own cases name.)
    #[test]
    fn the_script_lists_every_model_the_pages_selftest_opens_its_project_with() {
        let asked = models_the_selftest_asks_for();
        assert!(
            asked.len() >= 2,
            "the self-test names a chief's model and a worker's: {asked:?}"
        );
        let listed = models_the_script_lists();
        let missing: Vec<&String> = asked
            .iter()
            .filter(|model| !listed.contains(model))
            .collect();
        assert!(
            missing.is_empty(),
            "the script lists {listed:?}, and the self-test opens its project with {asked:?}"
        );
    }

    #[test]
    fn the_script_has_its_flood_sized_and_nothing_left_marked() {
        let script = script(7, 9);
        assert!(!script.contains('@'), "{script}");
        assert!(script.contains("-le 7 ]"));
        assert!(script.contains("-lt 9 ]"));
    }
}
