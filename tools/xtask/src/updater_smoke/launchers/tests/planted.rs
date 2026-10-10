//! The commands an installed app's own `cf setup` plants, read back as the smoke needs them.

use super::*;

/// What a `cf setup` writes: both commands of the home it is run for, naming `$0` or the entry it is given.
const WRITES: &str = "mkdir -p \"$CONSENSFLOW_HOME/bin\"
for name in cf consensflow; do
  printf '#!/bin/sh\\n# Installed by ConsensFlow. Runs the app’s own cf.\\nexport CONSENSFLOW_HOME=\"%s\"\\nexec %s \"$@\"\\n' \"$CONSENSFLOW_HOME\" \"$RUNS\" > \"$CONSENSFLOW_HOME/bin/$name\"
  chmod 755 \"$CONSENSFLOW_HOME/bin/$name\"
done";

/// An installed app whose `cf` (the flip's) or whose Node and `cf.mjs` (the bridge's) set up a home.
fn installed(sandbox: &Sandbox, release: Release) -> BundleInfo {
    let app = sandbox.copy.clone();
    let cf = under(&app, &["Contents", "Resources", "cli", "bin", "cf"]);
    let node = under(&app, &["Contents", "MacOS", "node"]);
    let entry = under(&app, &["Contents", "Resources", "cli", "bin", "cf.mjs"]);
    fs::create_dir_all(cf.parent().unwrap()).unwrap();
    fs::create_dir_all(node.parent().unwrap()).unwrap();
    // The commands it writes run the app's own way: its `cf`, or its Node on its `cf.mjs`.
    write(
        &cf,
        &format!(
            "#!/bin/sh\nif [ \"$1\" = setup ]; then\n  RUNS=\"\\\"$0\\\"\"\n{WRITES}\n  exit 0\nfi\necho 3.0.0-alpha.82\n"
        ),
        0o755,
    );
    write(&entry, "the entry", 0o644);
    write(
        &node,
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"{entry}\" ] && [ \"$2\" = setup ]; then\n  RUNS=\"\\\"$0\\\" \\\"$1\\\"\"\n{WRITES}\n  exit 0\nfi\nif [ \"$1\" = \"{entry}\" ] && [ \"$2\" = --version ]; then\n  echo 3.0.0-alpha.82\n  exit 0\nfi\nexit 1\n",
            entry = entry.display()
        ),
        0o755,
    );
    BundleInfo {
        binary: app.join("Contents/MacOS/app"),
        label: format!("the {} app", release.name()),
        version: "3.0.0-alpha.82".into(),
        node: release == Release::Bridge,
        cli_version: None,
        app,
        cf,
    }
}

#[test]
fn are_the_commands_of_both_homes_as_the_installed_releases_setup_wrote_them() {
    for release in Release::ALL {
        let parent = tempfile::tempdir().unwrap();
        let sandbox = Sandbox::make(parent.path()).unwrap();
        let planted = plant_commands(&installed(&sandbox, release), &sandbox, false, release)
            .unwrap_or_else(|said| panic!("{}: {said}", release.name()));
        assert_eq!(planted.repaired, NAMES, "both names serve this home");
        for (home, commands) in [
            (&sandbox.state, &planted.own),
            (&sandbox.other, &planted.other),
        ] {
            for name in NAMES {
                let text = &commands[name];
                assert!(text.contains(MARKER), "{text}");
                assert!(text.contains(&format!("export CONSENSFLOW_HOME=\"{}\"", home.display())));
                assert_eq!(
                    text.contains("cf.mjs"),
                    release == Release::Bridge,
                    "the bridge's setup names Node's, the flip's the cf's: {text}"
                );
            }
        }
    }
}

#[test]
fn put_one_of_the_other_homes_commands_where_this_homes_app_looks_when_asked_to() {
    for release in Release::ALL {
        let parent = tempfile::tempdir().unwrap();
        let sandbox = Sandbox::make(parent.path()).unwrap();
        let planted =
            plant_commands(&installed(&sandbox, release), &sandbox, true, release).unwrap();
        assert_eq!(
            planted.repaired,
            ["cf"],
            "the other name serves the other home"
        );
        assert_eq!(planted.own["consensflow"], planted.other["consensflow"]);
        assert!(planted.own["consensflow"].contains(&format!(
            "export CONSENSFLOW_HOME=\"{}\"",
            sandbox.other.display()
        )));
        assert!(planted.own["cf"].contains(&format!(
            "export CONSENSFLOW_HOME=\"{}\"",
            sandbox.state.display()
        )));
    }
}

#[test]
fn are_refused_when_the_setup_wrote_what_is_not_the_installed_apps_own() {
    let parent = tempfile::tempdir().unwrap();
    let sandbox = Sandbox::make(parent.path()).unwrap();
    let bundle = installed(&sandbox, Release::Flip);
    let plant = || {
        plant_commands(&bundle, &sandbox, false, Release::Flip)
            .unwrap_err()
            .to_string()
    };
    // A setup that fails says so, with what it said.
    write(&bundle.cf, "#!/bin/sh\necho no >&2\nexit 2\n", 0o755);
    let said = plant();
    assert!(
        said.contains("cf setup of the flip release failed on "),
        "{said}"
    );
    assert!(said.ends_with(": no"), "{said}");
    // Commands that run another program than the installed app's cf.
    write(
        &bundle.cf,
        &format!("#!/bin/sh\nRUNS='\"/elsewhere/cf\"'\n{WRITES}\n"),
        0o755,
    );
    let said = plant();
    assert!(
        said.contains("does not run the installed app's cf"),
        "{said}"
    );
    // Commands that are nobody's mark.
    write(
        &bundle.cf,
        "#!/bin/sh\nmkdir -p \"$CONSENSFLOW_HOME/bin\"\necho '#!/bin/sh' > \"$CONSENSFLOW_HOME/bin/cf\"\ncp \"$CONSENSFLOW_HOME/bin/cf\" \"$CONSENSFLOW_HOME/bin/consensflow\"\n",
        0o755,
    );
    let said = plant();
    assert!(said.contains(" is not ours"), "{said}");
}
