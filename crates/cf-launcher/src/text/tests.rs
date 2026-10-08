use super::*;

/// The launcher alpha.78 wrote for `sh`, as Node's `launcher(env)` made it: the
/// pin line only when a home was pinned.
fn old_sh(runtime: &str, cli: &str, home: Option<&str>) -> String {
    let pin = home.map_or_else(String::new, |home| {
        format!("export CONSENSFLOW_HOME=\"{home}\"\n")
    });
    format!(
        "#!/bin/sh\n# Installed by ConsensFlow. Runs the app's own runtime and its own copy of\n# the CLI, so the terminal and the window never drift apart.\n{pin}exec \"{runtime}\" \"{cli}\" \"$@\"\n"
    )
}

/// The same, for cmd.exe.
fn old_cmd(runtime: &str, cli: &str, home: Option<&str>) -> String {
    let pin = home.map_or_else(String::new, |home| {
        format!("setlocal\r\nset \"CONSENSFLOW_HOME={home}\"\r\n")
    });
    format!(
        "@echo off\r\nREM Installed by ConsensFlow. Runs the app's own runtime and its own copy of\r\nREM the CLI, so the terminal and the window never drift apart.\r\n{pin}\"{runtime}\" \"{cli}\" %*\r\n"
    )
}

fn node(runtime: &str, entry: &str) -> Option<Runs> {
    Some(Runs::Node {
        runtime: runtime.to_owned(),
        entry: entry.to_owned(),
    })
}

fn native(cf: &str) -> Option<Runs> {
    Some(Runs::Native { cf: cf.to_owned() })
}

#[test]
fn the_launcher_for_sh_runs_the_cf_it_names_with_every_argument() {
    assert_eq!(
        launcher(false, "/App/Contents/Resources/cli/bin/cf", None),
        "#!/bin/sh\n# Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\n# window never drift apart.\nexec \"/App/Contents/Resources/cli/bin/cf\" \"$@\"\n"
    );
}

#[test]
fn a_pinned_launcher_for_sh_exports_the_home_before_it_runs() {
    assert_eq!(
        launcher(false, "/App/cli/bin/cf", Some("/Users/me/.consensflow-candidate")),
        "#!/bin/sh\n# Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\n# window never drift apart.\nexport CONSENSFLOW_HOME=\"/Users/me/.consensflow-candidate\"\nexec \"/App/cli/bin/cf\" \"$@\"\n"
    );
}

#[test]
fn the_launcher_for_cmd_ends_its_lines_in_cr_lf_and_forwards_the_arguments_with_percent_star() {
    assert_eq!(
        launcher(true, r"C:\Program Files\ConsensFlow\cli\bin\cf.exe", None),
        "@echo off\r\nREM Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\r\nREM window never drift apart.\r\n\"C:\\Program Files\\ConsensFlow\\cli\\bin\\cf.exe\" %*\r\n"
    );
}

#[test]
fn a_pinned_launcher_for_cmd_sets_the_home_after_setlocal_so_it_ends_with_the_command() {
    assert_eq!(
        launcher(true, r"C:\App\cli\bin\cf.exe", Some(r"C:\Users\me\.consensflow-candidate")),
        "@echo off\r\nREM Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\r\nREM window never drift apart.\r\nsetlocal\r\nset \"CONSENSFLOW_HOME=C:\\Users\\me\\.consensflow-candidate\"\r\n\"C:\\App\\cli\\bin\\cf.exe\" %*\r\n"
    );
}

#[test]
fn what_sh_reads_for_itself_is_escaped_in_a_path_and_nothing_else_is() {
    let written = launcher(false, "/a b/c\"d$e`f\\g/cf", Some("/h$"));
    assert!(
        written.contains("export CONSENSFLOW_HOME=\"/h\\$\"\n"),
        "{written}"
    );
    assert!(
        written.ends_with("exec \"/a b/c\\\"d\\$e\\`f\\\\g/cf\" \"$@\"\n"),
        "{written}"
    );
}

#[test]
fn a_percent_sign_is_doubled_in_a_cmd_path_and_nothing_else_is() {
    let written = launcher(true, r"C:\100%\^&(x)\cf.exe", Some(r"C:\%HOME%"));
    assert!(
        written.contains("set \"CONSENSFLOW_HOME=C:\\%%HOME%%\"\r\n"),
        "{written}"
    );
    assert!(
        written.ends_with("\"C:\\100%%\\^&(x)\\cf.exe\" %*\r\n"),
        "{written}"
    );
}

#[test]
fn a_launcher_this_build_wrote_reads_back_whatever_its_paths_hold() {
    let sh = [
        "/plain/cf",
        "/with space/cf",
        "/quote\"/cf",
        "/dollar$HOME/cf",
        "/back`tick`/cf",
        "/back\\slash/cf",
        "/two\\\\slashes/cf",
        "/new\nline/cf",
        "/unicode/é日本/cf",
        "/\\$\"`/cf",
    ];
    for cf in sh {
        for home in [None, Some(cf)] {
            let written = launcher(false, cf, home);
            assert_eq!(runs(&written, false), native(cf), "{cf:?}");
            assert_eq!(pinned_home(&written, false).as_deref(), home, "{cf:?}");
        }
    }
    let cmd = [
        r"C:\plain\cf.exe",
        r"C:\Program Files\x\cf.exe",
        r"C:\100%\cf.exe",
        r"C:\%%\cf.exe",
        r"C:\%PATH%\cf.exe",
        r"\\server\share\cf.exe",
    ];
    for cf in cmd {
        for home in [None, Some(cf)] {
            let written = launcher(true, cf, home);
            assert_eq!(runs(&written, true), native(cf), "{cf:?}");
            assert_eq!(pinned_home(&written, true).as_deref(), home, "{cf:?}");
        }
    }
}

#[test]
fn the_old_shape_reads_as_node_read_it_a_runtime_and_the_cf_mjs_it_runs() {
    let sh = old_sh("/App/MacOS/node", "/App/Resources/cli/bin/cf.mjs", None);
    assert_eq!(
        runs(&sh, false),
        node("/App/MacOS/node", "/App/Resources/cli/bin/cf.mjs")
    );
    let pinned = old_sh(
        "/n",
        "/x/bin/cf.mjs",
        Some("/Users/me/.consensflow-candidate"),
    );
    assert_eq!(runs(&pinned, false), node("/n", "/x/bin/cf.mjs"));
    let cmd = old_cmd(r"C:\App\node.exe", r"C:\App\cli\bin\cf.mjs", None);
    assert_eq!(
        runs(&cmd, true),
        node(r"C:\App\node.exe", r"C:\App\cli\bin\cf.mjs")
    );
    // The pin's quotes come first on Windows and are not the runtime.
    let pinned = old_cmd(r"C:\App\node.exe", r"C:\App\cli\bin\cf.mjs", Some(r"C:\h"));
    assert_eq!(
        runs(&pinned, true),
        node(r"C:\App\node.exe", r"C:\App\cli\bin\cf.mjs")
    );
}

#[test]
fn the_old_shape_is_the_first_quoted_pair_that_the_pattern_matches_and_no_other() {
    let cases: [(&str, Option<(&str, &str)>); 13] = [
        // A runtime with a space in it, and the white space of any kind between.
        (
            "\"/a b/node\" \"/x/cf.mjs\"",
            Some(("/a b/node", "/x/cf.mjs")),
        ),
        ("\"/n\"\n\"/x/cf.mjs\"", Some(("/n", "/x/cf.mjs"))),
        ("\"/n\"\t \r\n \"/x/cf.mjs\"", Some(("/n", "/x/cf.mjs"))),
        ("\"/n\"\u{a0}\"/x/cf.mjs\"", Some(("/n", "/x/cf.mjs"))),
        ("\"/n\"\u{feff}\"/x/cf.mjs\"", Some(("/n", "/x/cf.mjs"))),
        // JavaScript's `\s` has U+FEFF and not U+0085, and nothing is no space.
        ("\"/n\"\u{85}\"/x/cf.mjs\"", None),
        ("\"/n\"\"/x/cf.mjs\"", None),
        // Something has to come before `cf.mjs`, and it ends the quoted path.
        ("\"/n\" \"cf.mjs\"", None),
        ("\"/n\" \"/cf.mjs\"", Some(("/n", "/cf.mjs"))),
        ("\"/n\" \"/x/cf.mjs.bak\"", None),
        ("\"\" \"/x/cf.mjs\"", None),
        // The first pair of quotes that fit, from the left.
        (
            "\"a\" \"b\" \"/n\" \"/x/cf.mjs\"",
            Some(("/n", "/x/cf.mjs")),
        ),
        (
            "\"/n\" \"/x/cf.mjs\" \"/m\" \"/y/cf.mjs\"",
            Some(("/n", "/x/cf.mjs")),
        ),
    ];
    for (text, expected) in cases {
        let found = runs(text, false);
        let expected = expected.and_then(|(runtime, entry)| node(runtime, entry));
        assert_eq!(found, expected, "{text:?}");
    }
}

#[test]
fn the_new_shape_is_the_cf_on_a_line_of_its_own_and_in_no_other_place() {
    assert_eq!(runs("exec \"/x/cf\" \"$@\"", false), native("/x/cf"));
    assert_eq!(runs("exec \"/x/cf\" \"$@\"\r\n", false), native("/x/cf"));
    assert_eq!(
        runs(
            "a\nexport CONSENSFLOW_HOME=\"/h\"\nexec \"/x/cf\" \"$@\"\n",
            false
        ),
        native("/x/cf")
    );
    assert_eq!(runs("\"C:\\x\\cf.exe\" %*", true), native(r"C:\x\cf.exe"));
    for text in [
        "echo exec \"/x/cf\" \"$@\"\n",
        "exec \"/x/cf\" \"$@\" extra\n",
        "exec \"/x/cf\" \"$@",
        "exec \"/x/cf\"\n",
        "exec \"/x/cf \"$@\"\n",
        "# exec \"/x/cf\" \"$@\"\n",
    ] {
        assert_eq!(runs(text, false), None, "{text:?}");
    }
    assert_eq!(runs("\"C:\\x\\cf.exe\" %* >nul\r\n", true), None);
    assert_eq!(runs("REM \"C:\\x\\cf.exe\" %*\r\n", true), None);
    // The text of one is not the other's.
    assert_eq!(runs("\"C:\\x\\cf.exe\" %*\r\n", false), None);
    assert_eq!(runs("exec \"/x/cf\" \"$@\"\n", true), None);
}

#[test]
fn a_launcher_that_says_nothing_of_the_kind_runs_nothing() {
    assert_eq!(runs("", false), None);
    assert_eq!(
        runs("#!/bin/sh\n# Installed by ConsensFlow.\n", false),
        None
    );
    assert_eq!(runs("#!/bin/sh\nexec node\n", false), None);
}

#[test]
fn the_pin_is_the_home_on_the_line_the_launcher_wrote_it_in() {
    assert_eq!(
        pinned_home(&old_sh("/n", "/x/cf.mjs", Some("/Users/me/.cf")), false).as_deref(),
        Some("/Users/me/.cf")
    );
    assert_eq!(
        pinned_home(&old_cmd("n", "x\\cf.mjs", Some(r"C:\Users\me\.cf")), true).as_deref(),
        Some(r"C:\Users\me\.cf")
    );
    assert_eq!(pinned_home(&old_sh("/n", "/x/cf.mjs", None), false), None);
    assert_eq!(pinned_home(&old_cmd("n", "x\\cf.mjs", None), true), None);
    // Each form is its own system's: a `.cmd` pins nothing for `sh`.
    assert_eq!(
        pinned_home(&old_cmd("n", "x\\cf.mjs", Some(r"C:\h")), false),
        None
    );
    assert_eq!(
        pinned_home(&old_sh("/n", "/x/cf.mjs", Some("/h")), true),
        None
    );
    // A line of its own, whole: not an assignment inside another command.
    for text in [
        "echo export CONSENSFLOW_HOME=\"/h\"\n",
        "export CONSENSFLOW_HOME=\"/h\" && true\n",
        "export CONSENSFLOW_HOME=\"/h\n",
        " export CONSENSFLOW_HOME=\"/h\"\n",
        "export CONSENSFLOW_HOME=/h\n",
    ] {
        assert_eq!(pinned_home(text, false), None, "{text:?}");
    }
    // The first of two wins, as the shell would set it first.
    assert_eq!(
        pinned_home(
            "export CONSENSFLOW_HOME=\"/one\"\nexport CONSENSFLOW_HOME=\"/two\"\n",
            false
        )
        .as_deref(),
        Some("/one")
    );
}

#[test]
fn a_home_pinned_without_escapes_is_read_as_sh_reads_it() {
    // Node wrote the path as it was. A backslash that escapes nothing stays.
    assert_eq!(
        pinned_home("export CONSENSFLOW_HOME=\"/a\\b\"\n", false).as_deref(),
        Some("/a\\b")
    );
    assert_eq!(
        pinned_home("export CONSENSFLOW_HOME=\"/a\\\\b\"\n", false).as_deref(),
        Some("/a\\b")
    );
}

#[test]
fn a_path_is_spelled_plain_for_windows_and_as_it_is_for_anything_else() {
    let verbatim = |text: &str, windows| spelled(Path::new(text), windows);
    // Tauri's verbatim spelling, which cmd.exe starts no program through.
    assert_eq!(
        verbatim(r"\\?\C:\App\cli\bin\cf.exe", true),
        r"C:\App\cli\bin\cf.exe"
    );
    assert_eq!(
        verbatim(r"\\?\UNC\server\share\cf.exe", true),
        r"\\server\share\cf.exe"
    );
    // A plain one, a UNC one and a relative one are what they are.
    for same in [
        r"C:\App\cf.exe",
        r"\\server\share\cf.exe",
        r"bin\cf.exe",
        "",
    ] {
        assert_eq!(verbatim(same, true), same);
    }
    // Only a form for cmd.exe is spelled so: a folder of `sh` may be named so.
    assert_eq!(verbatim(r"\\?\C:\App\cf.exe", false), r"\\?\C:\App\cf.exe");
    assert_eq!(verbatim("/Applications/cf", false), "/Applications/cf");
}

#[test]
fn the_mark_is_what_makes_a_command_ours() {
    assert!(is_ours(&old_sh("/n", "/x/cf.mjs", None)));
    assert!(is_ours(&launcher(false, "/x/cf", None)));
    assert!(is_ours(&launcher(true, r"C:\x\cf.exe", None)));
    assert!(is_ours("#!/bin/sh\n# Installed by ConsensFlow.\n"));
    assert!(!is_ours("#!/bin/sh\necho hello\n"));
    assert!(!is_ours("# installed by consensflow\n"));
    assert!(!is_ours(""));
}

#[test]
fn a_program_that_merely_holds_the_mark_in_its_bytes_is_no_launcher_of_ours() {
    // The `cf` of this build holds the mark, as a program that writes launchers
    // must: it is not one, and must never be replaced by one.
    assert!(!is_ours("\u{7f}ELF\0\0\0Installed by ConsensFlow\0\0"));
    assert!(!is_ours("Installed by ConsensFlow\0"));
    assert!(!is_ours("\0Installed by ConsensFlow"));
}
