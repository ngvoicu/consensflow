//! A rig tells of a case that failed, and of no case that passed. A rig dropped
//! unclosed is one its case left on the way: by a check that failed (a panic), or
//! by an error it returned (a case is a `Result`, and the wait that timed out ends
//! most of the failed ones that way, with no panic in flight when the rig is
//! dropped). It keeps what its windows showed in a file beside its home
//! (`<home>-left.txt`, `rig/report.rs`) for the person who reads the failure, and
//! says where in the output of that case. A case that passes ends by closing its
//! rig, and the rig has nothing to tell of it.
//!
//! CI's run of 2026-10-21 said it "kept in" fourteen files for as many cases that
//! had passed: thirteen of the folder-trust regression's and the follow-up's one
//! ended without closing their rigs. The rig cannot tell a case that returned `Ok`
//! and forgot the close from one that returned an error, so the cases are held to
//! the close here, as they are written.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::process::Run;
use cf_e2e::rig::Rig;
use regex::Regex;

use crate::Outcome;

/// Where the rig whose home is `home` keeps its report.
fn report_beside(home: &Path) -> PathBuf {
    let name = home.file_name().unwrap_or_default().to_string_lossy();
    home.with_file_name(format!("{name}-left.txt"))
}

/// The home of `rig`.
fn home_of(rig: &Rig) -> PathBuf {
    let workspace = rig.workspace();
    workspace.parent().unwrap_or(workspace).to_path_buf()
}

/// A case that fails as most do: it waits for what no window does, and the
/// timeout ends it with an error it returns, its rig still up. `home` is where
/// its rig's home was.
fn a_case_that_waits_for_what_never_comes(home: &mut Option<PathBuf>) -> Outcome {
    let rig = crate::rig()?;
    *home = Some(home_of(&rig));
    rig.wait_for("what no window does", Duration::from_millis(200), || {
        Ok(false)
    })?;
    rig.close()?;
    Ok(())
}

/// A case whose check fails after it shut its rig down to look at what was left
/// (`shut_down_within` keeps the rig, which is the case's still): it panics.
fn a_case_whose_check_fails_after_a_shutdown(home: &mut Option<PathBuf>) {
    let Ok(mut rig) = crate::rig() else {
        panic!("no rig to fail with");
    };
    *home = Some(home_of(&rig));
    assert!(rig.shut_down_within(Duration::from_secs(10)).is_ok());
    panic!("a check of the case that does not hold");
}

#[test]
fn a_rig_its_case_closed_leaves_no_report() -> Outcome {
    let rig = crate::rig()?;
    let home = home_of(&rig);

    rig.close()?;

    assert!(
        !report_beside(&home).exists(),
        "a report was kept of a case that closed its rig"
    );
    assert!(!home.exists(), "the home of a rig that closed is gone");
    Ok(())
}

/// What every report starts with.
const HEADING: &str = "--- the rig, as the failed case left it";

#[test]
fn a_rig_its_case_left_on_an_error_it_returned_keeps_its_report() -> Outcome {
    let mut home = None;

    let failed = a_case_that_waits_for_what_never_comes(&mut home);

    assert!(failed.is_err(), "the wait for what never comes ended well");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    let kept = fs::read_to_string(&report)?;
    assert!(kept.contains(HEADING), "{kept}");
    fs::remove_file(report)?;
    Ok(())
}

#[test]
fn a_rig_shut_down_and_dropped_while_a_check_fails_keeps_its_report() -> Outcome {
    let mut home = None;

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a_case_whose_check_fails_after_a_shutdown(&mut home);
    }));

    assert!(caught.is_err(), "the check held");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    let kept = fs::read_to_string(&report)?;
    assert!(kept.contains(HEADING), "{kept}");
    fs::remove_file(report)?;
    Ok(())
}

/// What the test binary says of one case it is asked to run alone: its output,
/// as `cargo test` shows it (`shown`: as `--nocapture` does).
fn output_of_the_case_alone(shown: bool) -> Result<String, Box<dyn std::error::Error>> {
    let case = "reports::a_rig_its_case_left_on_an_error_it_returned_keeps_its_report";
    let mut asked = vec!["--exact", case, "--test-threads=1"];
    if shown {
        asked.push("--nocapture");
    }
    let ran = Run::new(&std::env::current_exe()?)
        .args(asked)
        .inheriting_env()
        .run()?;
    assert_eq!(ran.code, Some(0), "{ran}");
    Ok(format!("{}{}", ran.stdout, ran.stderr))
}

#[test]
fn where_a_report_is_kept_is_said_in_the_output_of_the_case_that_failed_and_of_no_other() -> Outcome
{
    // The case above fails inside itself and passes: its report is the rig's own, said
    // as a failed case's would be, and a test run holds it unless it is asked for.
    let held = output_of_the_case_alone(false)?;
    assert!(!held.contains("is kept in"), "{held}");
    let shown = output_of_the_case_alone(true)?;
    assert!(
        shown.contains("the rig, as the failed case left it, is kept in"),
        "{shown}"
    );
    Ok(())
}

/// What starts a rig in a case: the rig's own `start`, and the suite's `rig()` (not
/// a `.rig()` of a project's members, which is one already started). A case that
/// expects the start to be refused, `let Err(…) = Rig::start(…)`, has none to close.
const STARTS: &str = r"\bRig::start\(|(?:^|[^.\w])rig\(\)";
const REFUSED: &str = r"let Err\(";

/// What ends one: a close of any of its kinds. (`shut_down_within` keeps the rig.)
const CLOSES: &str = r"\.close(?:_\w+)?\(";

/// The cases of `text`, each with its name and its lines: the functions under a
/// `#[test]`, to the line that closes them, which rustfmt puts at the indent of
/// their `fn`.
fn cases_of(text: &str) -> Vec<(String, Vec<&str>)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut cases = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        at += 1;
        if lines[at - 1].trim() != "#[test]" {
            continue;
        }
        let Some(head) =
            (at..lines.len()).find(|&line| lines[line].trim_start().starts_with("fn "))
        else {
            break;
        };
        let indent = lines[head].len() - lines[head].trim_start().len();
        let end = (head..lines.len())
            .find(|&line| lines[line].starts_with(&format!("{}}}", " ".repeat(indent))))
            .unwrap_or(lines.len() - 1);
        let name = lines[head].trim_start()[3..]
            .split(['(', '<'])
            .next()
            .unwrap_or_default()
            .to_owned();
        cases.push((name, lines[head..=end].to_vec()));
        at = end + 1;
    }
    cases
}

/// The source files under `folder`.
fn sources_in(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(sources_in(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
    found.sort();
    found
}

#[test]
fn every_case_of_the_rig_and_the_regressions_that_starts_a_rig_closes_it() -> Outcome {
    let (starts, refused, closes) = (
        Regex::new(STARTS)?,
        Regex::new(REFUSED)?,
        Regex::new(CLOSES)?,
    );
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut left = Vec::new();
    let mut seen = 0;
    for folder in ["regressions", "rig"] {
        for file in sources_in(&tests.join(folder)) {
            let text = fs::read_to_string(&file)?;
            for (name, lines) in cases_of(&text) {
                seen += 1;
                let started = lines
                    .iter()
                    .filter(|line| !refused.is_match(line))
                    .map(|line| starts.find_iter(line).count())
                    .sum::<usize>();
                let closed = lines
                    .iter()
                    .map(|line| closes.find_iter(line).count())
                    .sum::<usize>();
                if started > closed {
                    left.push(format!(
                        "{folder}/{}: {name} starts {started} rigs and closes {closed}",
                        file.file_name().unwrap_or_default().to_string_lossy()
                    ));
                }
            }
        }
    }

    assert!(
        seen > 150,
        "only {seen} cases were found: the scan is blind"
    );
    assert!(
        left.is_empty(),
        "a case that passes ends by closing its rig, or its drop keeps a report of a case that did not fail:\n{}",
        left.join("\n")
    );
    Ok(())
}

/// CI's gate job as a list of steps, each one's lines as written, the comments
/// and the list's dash left out.
fn ci_steps() -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    let workflow = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml");
    let mut steps: Vec<Vec<String>> = Vec::new();
    for line in fs::read_to_string(workflow)?.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with("      - ") {
            steps.push(Vec::new());
        } else if !line.starts_with("        ") {
            continue;
        }
        if let Some(step) = steps.last_mut() {
            step.push(line.trim().trim_start_matches("- ").to_owned());
        }
    }
    Ok(steps)
}

/// The step of `steps` with a line that holds `needle`.
fn step_with<'a>(
    steps: &'a [Vec<String>],
    needle: &str,
) -> Result<&'a [String], Box<dyn std::error::Error>> {
    steps
        .iter()
        .find(|step| step.iter().any(|line| line.contains(needle)))
        .map(Vec::as_slice)
        .ok_or_else(|| format!("CI has no step with {needle}").into())
}

/// Whether the upload action's `pattern` finds `path`: as it reads one on Windows,
/// where both slashes part a path and its case does not matter, and `*` is any
/// run of characters of a name.
fn finds(pattern: &str, path: &str) -> Result<bool, regex::Error> {
    let slashes = |text: &str| text.replace('\\', "/");
    let parts: Vec<String> = slashes(pattern).split('*').map(regex::escape).collect();
    let case = if cfg!(windows) { "(?i)" } else { "" };
    Ok(Regex::new(&format!("{case}^{}$", parts.join("[^/]*")))?.is_match(&slashes(path)))
}

/// The path the upload step collects from, `RIG_TEMP` read as the step that names it
/// says it is.
const COLLECTED: &str = "path: ${{ env.RIG_TEMP }}/consensflow-integration-*-left.txt";

#[test]
fn ci_uploads_for_a_failed_job_the_reports_a_failed_case_keeps_and_no_other_file() -> Outcome {
    let steps = ci_steps()?;
    let names = step_with(&steps, "RIG_TEMP=")?;
    let upload = step_with(&steps, "uses: actions/upload-artifact@")?;

    // Only for a failed job, a pinned action, a week, and nothing said where there is nothing.
    for (what, step) in [
        ("the step that names the folder", names),
        ("the upload", upload),
    ] {
        assert!(
            step.iter().any(|line| line == "if: failure()"),
            "{what} runs for a job that did not fail: {step:?}"
        );
    }
    let pinned = Regex::new(r"^uses: actions/upload-artifact@[0-9a-f]{40} # v\d+\.\d+\.\d+$")?;
    assert!(
        upload.iter().any(|line| pinned.is_match(line)),
        "{upload:?}"
    );
    for line in [
        "if-no-files-found: ignore",
        "retention-days: 7",
        "name: rig-reports-${{ matrix.os }}",
        COLLECTED,
    ] {
        assert!(
            upload.iter().any(|have| have == line),
            "no `{line}`: {upload:?}"
        );
    }

    // The folder the step names is the one the rig keeps its reports in: Node's temp
    // folder, as the step asks it, is the Rust's, where there is a Node to ask.
    let node = Regex::new(r#"node -p "([^"]*)""#)?;
    let asked = names
        .iter()
        .find_map(|line| node.captures(line))
        .map(|found| found[1].to_owned())
        .ok_or("the step that names the folder asks no node")?;
    let rust = std::env::temp_dir().to_string_lossy().into_owned();
    let temp = match Run::new(Path::new("node"))
        .args(["-p", &asked])
        .inheriting_env()
        .run()
    {
        Ok(ran) if ran.code == Some(0) => ran.stdout.trim().to_owned(),
        _ => {
            eprintln!("there is no node here: the folder the step names is not asked of it");
            rust.clone()
        }
    };
    let bare = |folder: &str| folder.trim_end_matches(['/', '\\']).to_lowercase();
    assert_eq!(
        bare(&temp),
        bare(&rust),
        "node and the rig's temp folders differ"
    );

    // The report a failed case keeps is found, and neither the home beside it nor
    // another file of the folder.
    let mut home = None;
    assert!(a_case_that_waits_for_what_never_comes(&mut home).is_err());
    let home = home.ok_or("the case never started its rig")?;
    let report = report_beside(&home);
    let collected = COLLECTED
        .trim_start_matches("path: ")
        .replace("${{ env.RIG_TEMP }}", &temp);
    assert!(
        finds(&collected, &report.to_string_lossy())?,
        "`{collected}` does not find {}",
        report.display()
    );
    assert!(!finds(&collected, &home.to_string_lossy())?, "the home");
    let other = format!("{}.old", report.display());
    assert!(!finds(&collected, &other)?, "{other}");
    fs::remove_file(report)?;
    Ok(())
}

#[test]
fn the_reports_are_found_in_the_temp_folders_of_the_runners_of_both_platforms() -> Outcome {
    // The folders CI's run of 2026-10-21 said it kept reports in: the Mac's, as
    // Node says it (TMPDIR without its last slash), and Windows', as TEMP is.
    for (temp, slash) in [
        ("/var/folders/n6/hn37q6pj7sj_9dmqqc2113800000gn/T", '/'),
        (r"C:\Users\RUNNER~1\AppData\Local\Temp", '\\'),
    ] {
        let collected = COLLECTED
            .trim_start_matches("path: ")
            .replace("${{ env.RIG_TEMP }}", temp);
        let report = format!("{temp}{slash}consensflow-integration-AbC123-left.txt");
        assert!(
            finds(&collected, &report)?,
            "`{collected}` does not find {report}"
        );
        let home = format!("{temp}{slash}consensflow-integration-AbC123");
        assert!(!finds(&collected, &home)?, "{home}");
        let deeper = format!("{temp}{slash}other{slash}consensflow-integration-AbC123-left.txt");
        assert!(!finds(&collected, &deeper)?, "{deeper}");
    }
    Ok(())
}
