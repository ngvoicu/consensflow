use super::*;

/// A Pi folder with `files` (name, text) in it, and the environment that finds it with `vars`.
fn setup_of(files: &[(&str, &str)], vars: &[(&str, &str)]) -> Setup {
    let folder = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let at = folder.path().join(name);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    let env = Env::from_vars(
        [("PI_CODING_AGENT_DIR", folder.path().as_os_str())]
            .into_iter()
            .chain(
                vars.iter()
                    .map(|(name, value)| (*name, std::ffi::OsStr::new(value))),
            ),
    );
    Setup::read(&env)
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_folder_that_holds_nothing_is_no_provider_set_up() {
    let nothing = setup_of(&[("settings.json", "{}")], &[]);
    assert!(nothing.is_empty());
    let said = nothing.unlisted();
    assert!(
        said.starts_with("Pi lists no model: no provider is set up ("),
        "{said}"
    );
    // How to give Pi one, in the three ways the human has.
    for way in ["/login", "OPENROUTER_API_KEY", "models.json"] {
        assert!(said.contains(way), "{way} in {said}");
    }
    // A folder that is not there at all is the same.
    let gone = Setup::read(&Env::from_vars([(
        "PI_CODING_AGENT_DIR",
        "/no/such/pi/folder",
    )]));
    assert!(gone.is_empty());
}

#[test]
fn each_source_of_a_provider_is_named_by_its_names_only() {
    let setup = setup_of(
        &[
            (
                "auth.json",
                r#"{"openai-codex":{"type":"oauth","access":"SECRET-ACCESS","refresh":"SECRET-REFRESH","expires":1},
                    "openrouter":{"type":"api_key","key":"SECRET-KEY"}}"#,
            ),
            (
                "models.json",
                r#"{"providers":{"mylocal":{"baseUrl":"http://SECRET-HOST/v1","apiKey":"SECRET-LOCAL","models":[{"id":"m"}]}}}"#,
            ),
            (
                "settings.json",
                r#"{"packages":["npm:@router-for-me/pi-cliproxyapi-provider",{"source":"git:github.com/x/y","extensions":["a.ts"]},7]}"#,
            ),
            ("extensions/provider.ts", ""),
            ("extensions/.hidden", ""),
            ("extensions/pack/index.ts", ""),
        ],
        &[
            ("OPENAI_API_KEY", "SECRET-ENV"),
            ("STRIPE_API_KEY", "not pi's"),
        ],
    );
    assert_eq!(
        setup,
        Setup {
            logins: names(&["openai-codex", "openrouter"]),
            custom: names(&["mylocal"]),
            packages: names(&[
                "npm:@router-for-me/pi-cliproxyapi-provider",
                "git:github.com/x/y"
            ]),
            extensions: names(&["pack", "provider.ts"]),
            keys: names(&["OPENAI_API_KEY"]),
        }
    );
    let said = setup.unlisted();
    assert!(
        said.starts_with("Pi lists no model though it has a login for openai-codex, openrouter;"),
        "{said}"
    );
    assert!(!said.contains("no provider is set up"), "{said}");
    assert!(
        said.contains("packages npm:@router-for-me/pi-cliproxyapi-provider, git:github.com/x/y"),
        "{said}"
    );
    assert!(said.contains("provider keys OPENAI_API_KEY"), "{said}");
    assert!(!said.contains("STRIPE"), "{said}");
    assert!(
        !said.contains("SECRET"),
        "nothing of a login, a key or an address: {said}"
    );
}

#[test]
fn a_file_that_is_not_json_or_not_a_table_names_nothing() {
    let setup = setup_of(
        &[
            ("auth.json", "not json"),
            ("models.json", r#"{"providers":["x"]}"#),
            ("settings.json", r#"{"packages":"npm:x"}"#),
        ],
        &[],
    );
    assert!(setup.is_empty(), "{setup:?}");
}

#[test]
fn a_provider_key_in_the_environment_alone_is_a_provider_set_up() {
    let setup = setup_of(&[], &[("OPENROUTER_API_KEY", "x"), ("META_API_KEY", "")]);
    assert_eq!(
        setup.keys,
        names(&["OPENROUTER_API_KEY"]),
        "an empty one is none"
    );
    assert!(!setup.is_empty());
}

#[test]
fn with_no_home_and_no_folder_named_only_the_environment_is_looked_at() {
    let setup = Setup::read(&Env::from_vars([("GROQ_API_KEY", "x")]));
    assert_eq!(setup.keys, names(&["GROQ_API_KEY"]));
    assert!(setup.logins.is_empty() && setup.packages.is_empty());
}
