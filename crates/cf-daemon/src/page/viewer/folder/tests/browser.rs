//! An HTML file read for the browser's copy: as any file is read, with the
//! assets it names beside its folder, each read the same way, so many and no
//! more.

use super::super::browser::MAX_ASSETS;
use super::*;

/// The places and bytes of what the browser is given of `path`.
fn for_browser(folder: &Folder, path: &str) -> Result<Vec<(String, String)>, String> {
    Ok(folder
        .for_browser(path, &|| {})?
        .files
        .into_iter()
        .map(|(at, bytes)| (at.join("/"), String::from_utf8(bytes).unwrap()))
        .collect())
}

/// An HTML file is read for the browser as any file is read, with the assets
/// it names beside its folder, each read the same way: what cannot be read of
/// them, or is out of the folder, is left out, and the file opens without it.
#[test]
fn only_a_regular_html_file_is_read_for_the_browser_with_the_assets_it_names() {
    let spec = "<link href=\"../assets/spec-styles.css\"><script src='../assets/spec-runtime.js'>\
                </script><img src=../assets/spec-styles.css><img src=\"../assets/gone.png\">\
                <a href=\"../assets/.env\"></a><a href=\"../assets/../../secret.env\"></a>";
    let project = project_with(&[
        ("checkout/SPEC.html", spec),
        ("checkout/mockup.HTM", "<p>m</p>"),
        ("checkout/SPEC.md", "# A"),
        ("assets/spec-styles.css", "p {}"),
        ("assets/spec-runtime.js", "run()"),
        ("assets/.env", "TOKEN=1"),
        ("top.html", "<link href=\"../assets/spec-styles.css\">"),
    ]);
    fs::write(project.path().join("secret.env"), "TOKEN=2").unwrap();
    let folder = specs(&project);
    let owned = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(at, text)| ((*at).to_owned(), (*text).to_owned()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        for_browser(&folder, "checkout/SPEC.html").unwrap(),
        owned(&[
            ("checkout/SPEC.html", spec),
            ("assets/spec-styles.css", "p {}"),
            ("assets/spec-runtime.js", "run()"),
        ])
    );
    assert_eq!(
        for_browser(&folder, "checkout/mockup.HTM").unwrap(),
        owned(&[("checkout/mockup.HTM", "<p>m</p>")])
    );
    // At the top of the folder, `../assets/` is out of it.
    assert_eq!(
        for_browser(&folder, "top.html").unwrap(),
        owned(&[("top.html", "<link href=\"../assets/spec-styles.css\">")])
    );
    for (path, words) in [
        (
            "checkout/SPEC.md",
            "checkout/SPEC.md is no HTML file: only an .html or .htm file opens in the browser",
        ),
        (
            "checkout/gone.html",
            "no file checkout/gone.html in the specs folder",
        ),
        (
            "../checkout/SPEC.html",
            "../checkout/SPEC.html climbs out of the specs folder (..)",
        ),
    ] {
        assert_eq!(for_browser(&folder, path).unwrap_err(), words);
    }
    let limit = usize::try_from(MAX_FILE).unwrap();
    fs::write(
        project.path().join(".specs/big.html"),
        "x".repeat(limit + 1),
    )
    .unwrap();
    assert_eq!(
        for_browser(&folder, "big.html").unwrap_err(),
        "big.html is 512.0 KB, more than the 512.0 KB the viewer opens"
    );
}

/// An HTML file names at most so many assets: the rest are not read.
#[test]
fn an_html_file_is_copied_with_so_many_assets_and_no_more() {
    let names: Vec<String> = (0..MAX_ASSETS + 3).map(|at| format!("a{at}.css")).collect();
    let spec: String = names
        .iter()
        .map(|name| format!("<link href=\"../assets/{name}\">"))
        .collect();
    let mut files: Vec<(String, &str)> = vec![("s/SPEC.html".to_owned(), spec.as_str())];
    files.extend(names.iter().map(|name| (format!("assets/{name}"), "p {}")));
    let listed: Vec<(&str, &str)> = files
        .iter()
        .map(|(at, text)| (at.as_str(), *text))
        .collect();
    let project = project_with(&listed);
    let read = for_browser(&specs(&project), "s/SPEC.html").unwrap();
    assert_eq!(read.len(), 1 + MAX_ASSETS);
    assert_eq!(
        read[MAX_ASSETS].0,
        format!("assets/a{}.css", MAX_ASSETS - 1)
    );
}
