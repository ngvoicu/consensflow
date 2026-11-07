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
    for way in ["/login", "providers.md", "models.json"] {
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
                r#"{"packages":["npm:@router-for-me/pi-cliproxyapi-provider",{"source":"git:github.com/x/y","extensions":["a.ts"],"token":"SECRET-PKG"},7,null],
                    "extensions":["./declared.ts","-./off.ts"],"theme":"SECRET-THEME"}"#,
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
            extensions: names(&["pack", "provider.ts", "./declared.ts"]),
            keys: names(&["OPENAI"]),
            unread: Vec::new(),
            late: false,
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
    assert!(
        said.contains("a key in the environment for OPENAI"),
        "{said}"
    );
    assert!(!said.contains("STRIPE"), "{said}");
    assert!(
        !said.contains("SECRET"),
        "nothing of a login, a key or an address: {said}"
    );
}

#[test]
fn a_file_that_is_there_and_not_what_pi_s_files_are_is_unread_and_never_nothing() {
    let setup = setup_of(
        &[
            ("auth.json", "not json"),
            ("models.json", r#"{"providers":["x"]}"#),
            ("settings.json", r#"{"packages":"npm:x"}"#),
            ("extensions", "a file where the folder should be"),
        ],
        &[],
    );
    assert_eq!(
        setup.unread,
        names(&[
            "the file of logins",
            "models.json",
            "extensions",
            "settings.json"
        ])
    );
    assert!(
        !setup.is_empty(),
        "a setup that could not be read is not an empty one"
    );
    let said = setup.unlisted();
    assert!(
        said.contains("could not read the file of logins, models.json, extensions, settings.json"),
        "{said}"
    );
    assert!(!said.contains("no provider is set up"), "{said}");
}

#[test]
fn a_provider_key_in_the_environment_alone_is_a_provider_set_up() {
    let setup = setup_of(&[], &[("OPENROUTER_API_KEY", "x"), ("META_API_KEY", "")]);
    assert_eq!(setup.keys, names(&["OPENROUTER"]), "an empty one is none");
    assert!(!setup.is_empty());
}

#[test]
fn with_no_home_and_no_folder_named_only_the_environment_is_looked_at() {
    let setup = Setup::read(&Env::from_vars([("GROQ_API_KEY", "x")]));
    assert_eq!(setup.keys, names(&["GROQ"]));
    assert!(setup.logins.is_empty() && setup.packages.is_empty());
}

#[test]
fn the_extensions_of_the_settings_are_the_ones_that_declare_one() {
    for (entry, declares) in [
        ("./prov.ts", Some("./prov.ts")),
        ("  ~/ext/dir ", Some("~/ext/dir")),
        ("+./pinned.ts", Some("./pinned.ts")),
        ("-./off.ts", None),
        ("!glob/*", None),
        ("builtin:llama.cpp", None),
        ("+builtin:mcp", None),
        ("-builtin:mcp", None),
        ("", None),
    ] {
        assert_eq!(declares_one(entry).as_deref(), declares, "{entry:?}");
    }
}

/// A package source is shown without the user and password of its URL, its query and its
/// fragment, and with whatever names the package.
#[test]
fn a_source_is_shown_without_what_could_carry_a_credential() {
    for (source, shown_as) in [
        (
            "git:https://user:TOKEN@host.invalid/team/p.git?k=v#frag",
            "git:https://host.invalid/team/p.git",
        ),
        ("https://TOKEN@host.invalid/o/r", "https://host.invalid/o/r"),
        (
            "https://user:TOKEN@host.invalid:8443/o/r@v1",
            "https://host.invalid:8443/o/r@v1",
        ),
        ("ssh://git@host.invalid/o/r", "ssh://host.invalid/o/r"),
        ("git@host.invalid:o/r", "host.invalid:o/r"),
        ("git:TOKEN@host.invalid/o/r@v2", "git:host.invalid/o/r@v2"),
        ("git:git@host.invalid:o/r", "git:host.invalid:o/r"),
        ("git:host.invalid/o/r@ref", "git:host.invalid/o/r@ref"),
        ("git://host.invalid/o/r", "git://host.invalid/o/r"),
        ("npm:@scope/name@1.2.3", "npm:@scope/name@1.2.3"),
        ("npm:name", "npm:name"),
        (
            "npm:name@https://u:TOKEN@host.invalid/x.tgz",
            "npm:name@https://host.invalid/x.tgz",
        ),
        ("./pi-tools", "./pi-tools"),
        ("/Users/me/c#/pkg?", "/Users/me/c#/pkg?"),
        (r"C:\work\pkg", r"C:\work\pkg"),
    ] {
        let shown = shown(source);
        assert_eq!(shown, shown_as, "{source}");
        assert!(!shown.contains("TOKEN"), "{shown}");
    }
}

/// A pipe where a file should be is not opened (it waits for a writer for ever), and a file
/// larger than a setup is is not read.
#[cfg(unix)]
#[test]
fn a_pipe_and_a_file_too_large_are_unread_and_never_opened_whole() {
    use std::sync::mpsc;
    use std::time::Duration;

    let (tell, told) = mpsc::channel();
    std::thread::spawn(move || {
        let folder = tempfile::tempdir().unwrap();
        #[allow(clippy::disallowed_methods)] // `mkfifo` makes the pipe: nothing else of std can.
        let made = std::process::Command::new("/usr/bin/mkfifo")
            .arg(folder.path().join("models.json"))
            .status()
            .unwrap();
        assert!(made.success());
        let big = format!(r#"{{"x":"{}"}}"#, "y".repeat(FILE_CAP as usize));
        std::fs::write(folder.path().join("auth.json"), big).unwrap();
        let env = Env::from_vars([("PI_CODING_AGENT_DIR", folder.path().as_os_str())]);
        let _ = tell.send(Setup::read(&env));
    });
    let setup = told
        .recv_timeout(Duration::from_secs(10))
        .expect("the read ended: it opened no pipe");
    assert_eq!(setup.unread, names(&["the file of logins", "models.json"]));
    assert!(setup.logins.is_empty());
}

#[test]
fn a_credential_variable_is_named_by_its_provider_and_no_label_says_key_token_or_credentials() {
    for (variable, provider) in [
        ("OPENROUTER_API_KEY", "OPENROUTER"),
        ("ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC"),
        ("ANT_LING_API_KEY", "ANT_LING"),
        ("QWEN_TOKEN_PLAN_API_KEY", "QWEN_PLAN"),
        ("AWS_ACCESS_KEY_ID", "AWS"),
        ("AWS_BEARER_TOKEN_BEDROCK", "AWS_BEDROCK"),
        ("GOOGLE_APPLICATION_CREDENTIALS", "GOOGLE_APPLICATION"),
        ("HF_TOKEN", "HF"),
        ("LLAMA_BASE_URL", "LLAMA_BASE_URL"),
        ("AWS_PROFILE", "AWS_PROFILE"),
    ] {
        assert_eq!(provider_of(variable), provider, "{variable}");
    }
    for variable in CREDENTIALS {
        let label = provider_of(variable).to_lowercase();
        for word in WITHHELD_WORDS {
            assert!(!label.contains(word), "{variable} is {label}");
        }
    }
    // Two variables of one provider are one provider.
    let setup = setup_of(
        &[],
        &[("ANTHROPIC_API_KEY", "x"), ("ANTHROPIC_OAUTH_TOKEN", "x")],
    );
    assert_eq!(setup.keys, names(&["ANTHROPIC"]));
}

/// The words the engine's rule takes a line for (`cf_engine::quotable`: a secret-like word, or a
/// credential's word with a value after it), as substrings and so more than it takes: none of
/// the words ConsensFlow says of a setup holds one. The daemon holds the same words to the
/// engine itself (`none_of_the_words_about_a_setup_is_taken_for_a_credential`).
const WITHHELD_WORDS: [&str; 13] = [
    "password",
    "passwd",
    "passphrase",
    "pwd",
    "apikey",
    "api_key",
    "api key",
    "private key",
    "credential",
    "secret",
    "token",
    "auth",
    "bearer",
];

#[test]
fn no_word_said_of_a_setup_is_a_word_a_line_is_withheld_for() {
    let many = Setup {
        logins: names(&["openai", "openrouter"]),
        custom: names(&["mylocal"]),
        packages: names(&["npm:@scope/slow-provider@1.2.3"]),
        extensions: names(&["pack", "./declared.ts"]),
        keys: names(&["OPENAI", "AWS_BEDROCK"]),
        unread: names(&[
            "the file of logins",
            "models.json",
            "extensions",
            "settings.json",
        ]),
        late: false,
    };
    let said = [
        Setup::default().unlisted(),
        Setup::late().unlisted(),
        many.unlisted(),
        Setup {
            unread: names(&["the file of logins"]),
            ..Setup::default()
        }
        .unlisted(),
        many.loading().unwrap(),
    ];
    let valued_key = regex::Regex::new(r"key\s*[:=]").unwrap();
    for words in said {
        let lower = words.to_lowercase();
        for word in WITHHELD_WORDS {
            assert!(!lower.contains(word), "{word} in {words}");
        }
        assert!(
            !valued_key.is_match(&lower),
            "a key with a value after it in {words}"
        );
    }
}

#[test]
fn a_folder_not_read_in_time_is_said_so_and_is_never_called_empty() {
    let late = Setup::late();
    assert!(!late.is_empty());
    let said = late.unlisted();
    assert!(
        said.starts_with("Pi lists no model, and Pi's folder could not be read in time"),
        "{said}"
    );
    assert!(!said.contains("no provider is set up"), "{said}");
    assert_eq!(late.loading(), None, "nothing is known of what Pi loads");
}
