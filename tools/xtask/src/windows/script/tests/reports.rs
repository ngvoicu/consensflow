//! The script that lists what a run left in `evals/reports` there, to be brought back
//! ([`reports_script`]): read in its text, and, on Windows, run by the machine's PowerShell on a
//! profile made for it.

use super::*;

#[test]
fn the_listing_names_each_file_by_its_path_and_size_and_leaves_the_matrixs_folder_out() {
    let text = reports_script(DIR);
    assert!(
        text.contains("$reports = Join-Path (Join-Path $env:USERPROFILE 'consensflow-build') 'evals\\reports'\n"),
        "{text}"
    );
    assert!(
        text.contains("if ($name -notlike 'windows/*') { \"$name`t$($file.Length)\" }"),
        "{text}"
    );
    assert!(!text.contains('«'), "{text}");
    assert!(text.is_ascii(), "{text}");
}

/// The prune refuses a reports folder that is itself a link (it would be walked to where it
/// goes), and the listing does as well: a junction to a folder elsewhere would have that
/// folder's files brought back as the reports (T-194, P3-6). A link under the folder is not
/// entered by `Get-ChildItem -Recurse` of Windows PowerShell 5.1.
#[test]
fn the_listing_does_not_walk_a_reports_folder_that_is_itself_a_link() {
    let text = reports_script(DIR);
    let guard = "  if (-not ($folder.Attributes -band [IO.FileAttributes]::ReparsePoint)) {\n";
    assert!(
        text.contains("  $folder = Get-Item -LiteralPath $reports\n"),
        "{text}"
    );
    assert!(text.contains(guard), "{text}");
    assert!(
        text.find(guard).unwrap() < text.find("Get-ChildItem").unwrap(),
        "the listing is made only inside the guard: {text}"
    );
}

/// The listing run by the machine's PowerShell: a real folder lists its files, and a folder that
/// is a link lists none.
#[cfg(windows)]
#[test]
fn powershell_lists_the_reports_of_a_folder_and_none_of_a_link() {
    use std::fs;

    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    let profile = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let reports = profile.path().join(DIR).join("evals").join("reports");
    let list = |profile: &std::path::Path| {
        let vars: Vec<_> = Env::from_process()
            .iter()
            .filter(|(name, _)| !name.to_string_lossy().eq_ignore_ascii_case("USERPROFILE"))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain([("USERPROFILE".into(), profile.as_os_str().to_owned())])
            .collect();
        let ran = process::capture(
            &Invocation::new("powershell", ".")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                .arg(encode_script(&reports_script(DIR)).unwrap()),
            &Env::from_vars(vars),
        )
        .unwrap();
        assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
        ran.stdout.replace("\r\n", "\n")
    };

    // A profile with no folder lists none.
    assert_eq!(list(profile.path()), "");
    // A folder lists its files by their paths with `/` and their sizes, and what is under `windows`
    // is left out.
    for (name, text) in [
        ("long/2026-10-20T12-00-00-000Z-long.md", "report"),
        ("top.json", "{}"),
        ("windows/2026-10-20T10-00-00/run.log", "matrix"),
    ] {
        let file = reports.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    let mut lines: Vec<String> = list(profile.path()).lines().map(str::to_owned).collect();
    lines.sort();
    assert_eq!(
        lines,
        ["long/2026-10-20T12-00-00-000Z-long.md\t6", "top.json\t2"]
    );
    // The same folder made a junction to one elsewhere that holds reports: none is listed.
    fs::write(elsewhere.path().join("outside.json"), "{}").unwrap();
    fs::remove_dir_all(&reports).unwrap();
    let junction = Invocation::new("cmd", profile.path())
        .args(["/c", "mklink", "/J"])
        .arg(reports.as_os_str())
        .arg(elsewhere.path());
    assert_eq!(
        process::capture(&junction, &Env::from_process())
            .unwrap()
            .code,
        0
    );
    assert_eq!(list(profile.path()), "");
}
