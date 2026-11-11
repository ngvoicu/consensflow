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

    /// The stand-in of the hidden-app case (smoke/hidden.rs), the smoke's other `claude`.
    const HIDDEN: &str = include_str!("../fixtures/smoke-hidden-harness.sh");

    /// Both stand-ins of the smoke, by what they are: the page's self-test opens its project on
    /// either.
    fn scripts() -> [(&'static str, &'static str); 2] {
        [
            ("the smoke's harness", SCRIPT),
            ("the hidden case's", HIDDEN),
        ]
    }

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

    /// The models `script` lists when the daemon reads them (its `for model in …` line).
    fn models_the_script_lists(script: &str) -> Vec<String> {
        let line = script
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
    /// opens its project in the built app on these scripts, so each lists the models the
    /// self-test names, or `project.open` is refused and the smoke is red on a built app and
    /// nowhere else. (The rig runs the first too, with the models its own cases name.)
    #[test]
    fn the_script_lists_every_model_the_pages_selftest_opens_its_project_with() {
        let asked = models_the_selftest_asks_for();
        assert!(
            asked.len() >= 2,
            "the self-test names a chief's model and a worker's: {asked:?}"
        );
        for (which, script) in scripts() {
            let listed = models_the_script_lists(script);
            let missing: Vec<&String> = asked
                .iter()
                .filter(|model| !listed.contains(model))
                .collect();
            assert!(
                missing.is_empty(),
                "{which} lists {listed:?}, and the self-test opens its project with {asked:?}"
            );
        }
    }

    /// What the daemon's read gets from each script, run as the daemon runs it (`claude -p
    /// --input-format stream-json`, an `initialize` on its input): an answer to that request that
    /// holds every model the self-test names, and the script ends with its input. The hidden
    /// case's script had none (the daemon waited 30 s, then refused the project).
    #[cfg(unix)]
    #[test]
    fn each_script_answers_the_read_of_its_models_with_every_model_the_selftest_names() {
        let asked = models_the_selftest_asks_for();
        let folder = tempfile::tempdir().unwrap();
        for (which, script) in scripts() {
            let file = folder.path().join("claude");
            crate::files::write_executable(&file, script).unwrap();
            let read = r#"{"type":"control_request","request_id":"r1","request":{"subtype":"initialize"}}"#;
            let ran = crate::process::Run::new("/bin/sh")
                .arg(&file)
                .args(["-p", "--input-format", "stream-json"])
                .input(format!("{read}\n"))
                .var("CFSMOKE_PIDFILE", folder.path().join("pids"))
                .run()
                .unwrap();
            assert_eq!(ran.code, Some(0), "{which}: {}", ran.stderr);
            let answer: serde_json::Value = serde_json::from_str(ran.stdout.trim())
                .unwrap_or_else(|_| panic!("{which} answered {:?}", ran.stdout));
            assert_eq!(answer["response"]["request_id"], "r1", "{which}: {answer}");
            let values: Vec<&str> = answer["response"]["response"]["models"]
                .as_array()
                .unwrap_or_else(|| panic!("{which} lists no models: {answer}"))
                .iter()
                .filter_map(|model| model["value"].as_str())
                .collect();
            for model in &asked {
                assert!(values.contains(&model.as_str()), "{which}: {values:?}");
            }
            // It draws no window and notes no pid of one.
            assert!(!folder.path().join("pids").exists(), "{which} noted a pid");
        }
    }

    #[test]
    fn the_script_has_its_flood_sized_and_nothing_left_marked() {
        let script = script(7, 9);
        assert!(!script.contains('@'), "{script}");
        assert!(script.contains("-le 7 ]"));
        assert!(script.contains("-lt 9 ]"));
    }
}
