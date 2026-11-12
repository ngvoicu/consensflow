use std::fs;
use std::path::Path;

use cf_base::env::Env;
use serde_json::{json, Value};

use super::*;
use crate::contract::{Agent, LaunchId, Specs};
use crate::testing::{finished, Fakes};

const LAUNCH: &str = "11111111-1111-4111-8111-111111111111";

/// A Muse installed where its installer puts it under `home`, its launcher saying it
/// runs `version` (none: no `.muse-version`), the binary it names beside it.
fn installed(home: &Path, version: Option<&str>) -> Env {
    let bin = home.join(".local").join("bin");
    fs::create_dir_all(&bin).unwrap();
    let launcher = bin.join(if cfg!(windows) { "muse.cmd" } else { "muse" });
    fs::write(&launcher, "").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
    }
    if let Some(version) = version {
        fs::write(bin.join(".muse-version"), format!("{version}\n")).unwrap();
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        fs::write(bin.join(format!("muse-bin-{version}{suffix}")), "binary").unwrap();
    }
    Env::from_vars([
        ("HOME", home.to_str().unwrap()),
        ("USERPROFILE", home.to_str().unwrap()),
        ("XDG_DATA_HOME", home.join("data").to_str().unwrap()),
        ("XDG_CONFIG_HOME", home.join("config").to_str().unwrap()),
        (
            "CONSENSFLOW_HOME",
            home.join("consensflow").to_str().unwrap(),
        ),
    ])
}

fn adapter(env: &Env) -> MuseAdapter {
    MuseAdapter::new(&Fakes::new(env).services(env, Path::new("")))
}

/// A worker's launch on `muse-spark-1.3` at `minimal`, on `resume` where given.
fn launch<'a>(id: &'a LaunchId, resume: Option<&'a str>) -> Launch<'a> {
    Launch {
        id,
        project: 1,
        handle: "muse-1",
        role: "worker",
        directory: "/work",
        resume,
        previous_gone: true,
        message: Some("Do it."),
        agent: Some(Agent {
            model: Some("muse-spark-1.3"),
            effort: Some("minimal"),
            thinking: None,
            designer: false,
        }),
        instructions: "# ConsensFlow worker",
        specs: None,
        compaction: None,
        advisor_off: false,
        suggestion_off: false,
        first_message_ms: 1,
        starts_given_up: 0,
    }
}

fn prepared(env: &Env, resume: Option<&str>) -> Result<Prepared, String> {
    let id = LaunchId::new(LAUNCH).unwrap();
    finished(adapter(env).prepare(&launch(&id, resume)))
}

/// [`prepared`] for a window whose role text is `instructions`.
fn prepared_with(env: &Env, resume: Option<&str>, instructions: &str) -> Result<Prepared, String> {
    let id = LaunchId::new(LAUNCH).unwrap();
    let mut launch = launch(&id, resume);
    launch.instructions = instructions;
    finished(adapter(env).prepare(&launch))
}

/// A role text of `lines` lines of 70 characters: 71 bytes a line.
fn role_of(lines: usize) -> String {
    format!("{}\n", "a".repeat(70)).repeat(lines)
}

/// The hooks a command line gives Muse with `-c hooks=`.
fn hooks_of(argv: &[String]) -> Value {
    let at = argv.iter().position(|word| word == "-c").unwrap();
    serde_json::from_str(argv[at + 1].strip_prefix("hooks=").unwrap()).unwrap()
}

#[test]
fn a_launch_is_refused_in_plain_words_where_muse_is_missing_or_older_than_consensflow_reads() {
    let home = tempfile::tempdir().unwrap();
    let nowhere = Env::from_vars([("HOME", home.path().to_str().unwrap())]);
    assert_eq!(
        prepared(&nowhere, None).err().unwrap(),
        "muse is not installed on this machine"
    );
    let old = installed(home.path(), Some("1.4.12-R6000.1"));
    assert_eq!(
        prepared(&old, None).err().unwrap(),
        "Muse Code 1.4.13 or newer is required. Update Muse Code before opening a pane."
    );
}

#[test]
fn a_fresh_window_runs_the_pinned_binary_in_full_permission_with_its_hook_and_no_message_on_its_command_line(
) {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let prepared = prepared(&env, None).unwrap();
    let argv = &prepared.argv;
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let binary = home
        .path()
        .join(".local")
        .join("bin")
        .join(format!("muse-bin-1.4.13-R6996.1{suffix}"));
    assert_eq!(argv[0], binary.to_string_lossy());
    assert_eq!(
        argv[3..],
        [
            "--model",
            "muse-spark-1.3",
            "--reasoning-effort",
            "minimal",
            "--yolo",
            "--contributor-mode",
            "off",
            "--disable-request-user-input",
        ]
    );
    assert_eq!(
        prepared.env[0],
        ("MUSE_NO_AUTO_UPDATE".to_owned(), "1".to_owned())
    );
    assert!(
        prepared.env[1..]
            .iter()
            .all(|(name, _)| name == "XDG_DATA_HOME"),
        "{:?}",
        prepared.env
    );
    assert_eq!(prepared.drop_env, ["META_API_KEY"]);
    assert_eq!(prepared.native_session, None);
    // The hook: the window's own `cf`, the role written for the launch, and the file it
    // writes the window's sessions in, both in the launch's folder.
    let hooks = hooks_of(argv);
    let command = hooks["SessionStart"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    let folder = home
        .path()
        .join("consensflow")
        .join("integrations")
        .join("muse")
        .join(LAUNCH);
    let role = [
        "role",
        ".claude",
        "skills",
        "consensflow-worker",
        "SKILL.md",
    ]
    .iter()
    .fold(folder.clone(), |path, part| path.join(part));
    assert!(command.contains(" hook muse-session "), "{command}");
    assert!(
        command.contains(&format!("'{}'", role.display())),
        "{command}"
    );
    // Its role fits in a hook: the first hook says it, and the hooks past it, every launch's
    // eight, write a filler over any block an older role left.
    assert!(
        command.ends_with(&format!(
            "'{}' '0'",
            folder.join("sessions.jsonl").display()
        )),
        "{command}"
    );
    assert_eq!(
        hooks["SessionStart"][0]["hooks"].as_array().unwrap().len(),
        crate::muse::MOST_PARTS,
        "{hooks}"
    );
    assert_eq!(fs::read_to_string(role).unwrap(), "# ConsensFlow worker");
    assert_eq!(
        hooks.as_object().unwrap().len(),
        1,
        "no hooks of the owner's"
    );
}

#[test]
fn a_resumed_window_opens_on_its_session_with_no_message_on_its_command_line() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let session = "01a17bc9-e758-77f3-ac8d-a725edf373ae";
    let prepared = prepared(&env, Some(session)).unwrap();
    assert_eq!(prepared.argv[3..5], ["resume", session]);
    assert!(
        !prepared.argv.iter().any(|word| word == "Do it."),
        "{:?}",
        prepared.argv
    );
    assert_eq!(prepared.native_session.as_deref(), Some(session));
    assert_eq!(adapter(&env).first_message(), FirstMessage::Delivered);
    assert_eq!(
        prepared.argv.last().map(String::as_str),
        Some("--disable-request-user-input")
    );
}

/// MUS-13 (T-790): Muse sends no `PreToolUse` for its question tools (T-705, 1.4.13),
/// so no hook can answer one from the board, and nobody watches a member's window to
/// answer its dialog. A member is launched without them and asks with `cf ask`, as Pi's
/// is; the chief keeps its dialog, which the human answers in its window.
#[test]
fn a_members_window_has_no_question_tool_and_the_chiefs_keeps_its_own() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let id = LaunchId::new(LAUNCH).unwrap();
    let withheld = |role: &str| {
        let launch = Launch {
            role,
            ..launch(&id, None)
        };
        let prepared = finished(adapter(&env).prepare(&launch)).unwrap();
        prepared
            .argv
            .iter()
            .any(|word| word == "--disable-request-user-input")
    };
    assert!(withheld("worker"));
    assert!(withheld("reviewer"));
    assert!(!withheld("chief"));
}

/// T-931 P3-2: Muse 1.4.14 takes a data folder reached through a link for unsafe
/// (`unsafe_registry_root`: a Mac's `/tmp`), writes no `route_facts`, and a fresh window
/// never learns its session. The window's Muse is given the same folder by its real path,
/// and ConsensFlow reads its sessions there.
#[cfg(unix)]
#[test]
fn a_data_folder_reached_through_a_link_is_given_to_muse_by_its_real_path() {
    let folder = tempfile::tempdir().unwrap();
    let real = folder.path().join("real");
    fs::create_dir_all(&real).unwrap();
    let linked = folder.path().join("linked");
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    let env = installed(&linked, Some("1.4.13-R6996.1"));
    let prepared = prepared(&env, None).unwrap();
    let data = fs::canonicalize(&real).unwrap().join("data");
    assert!(
        prepared.env.contains(&(
            "XDG_DATA_HOME".to_owned(),
            data.to_string_lossy().into_owned()
        )),
        "{:?}",
        prepared.env
    );
    assert_eq!(paths::data(&env).unwrap(), data.join("muse"));
}

#[test]
fn the_owner_s_hooks_are_kept_for_the_run_and_their_settings_are_never_written() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let settings = home.path().join("config/muse/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let owner = json!({
        "schema_version": 1,
        "hooks": { "Stop": [{ "matcher": "", "hooks": [{ "type": "command", "command": "owner-stop" }] }] },
    });
    fs::write(&settings, owner.to_string()).unwrap();
    let prepared = prepared(&env, None).unwrap();
    let hooks = hooks_of(&prepared.argv);
    assert_eq!(hooks["Stop"], owner["hooks"]["Stop"]);
    assert_eq!(hooks["SessionStart"].as_array().unwrap().len(), 1);
    assert_eq!(fs::read_to_string(&settings).unwrap(), owner.to_string());
}

#[test]
fn a_session_is_resumable_as_its_log_says() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let muse = adapter(&env);
    let resumable = |session| finished(muse.resumable(session));
    assert_eq!(resumable("s1"), Some(false), "no log, and no index");
    let folder = home.path().join("data/muse/sessions/2026/10/26/s1");
    fs::create_dir_all(&folder).unwrap();
    let accepted = r#"{"schema_version":1,"id":"r1","sequence":1,"stream":{"kind":"session","id":"s1"},"payload_type":"runtime.user_intent.accepted","payload":{"intent_id":"t1","surface":"main","semantic_kind":{"kind":"chat"},"refill_blocks":[{"kind":"text","text":"Go."}]}}"#;
    fs::write(folder.join("session.jsonl"), format!("{accepted}\n")).unwrap();
    assert_eq!(resumable("s1"), Some(true));
    // A log there and unreadable, with no index: nothing is told.
    fs::write(folder.join("session.jsonl"), "null\n").unwrap();
    assert_eq!(resumable("s1"), None);
}

#[test]
fn its_servers_start_with_the_window_on_a_mac_and_its_box_is_cleared_with_control_c() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let expected = if cfg!(target_os = "macos") {
        Servers::AtLaunch
    } else {
        Servers::Unknown
    };
    assert_eq!(adapter(&env).servers(), expected);
    let windows = Env::from_vars([("OS", "Windows_NT"), ("HOME", "/h")]);
    assert_eq!(adapter(&windows).servers(), Servers::Unknown);
    assert_eq!(adapter(&env).input(), Input::Pasted { clear: &[0x03] });
    assert!(
        !adapter(&env).keeps_first_instructions(),
        "the hook says the role at each start"
    );
}

#[test]
fn a_specmint_muse_has_installed_is_used_where_it_is_and_consensflow_s_own_copy_otherwise() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let muse = adapter(&env);
    assert!(matches!(
        muse.specmint(Variant::Core, ".specs").unwrap(),
        Skill::Own(_)
    ));
}

#[test]
fn a_specmint_muse_has_installed_serves_only_a_project_that_keeps_its_specs_where_it_says() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    // As components: the adapter joins a path with its system's separator, which a `/` in a
    // literal is not on Windows.
    let skill = home
        .path()
        .join(".agents")
        .join("skills")
        .join("specmint-core")
        .join("SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(
        &skill,
        "---\nname: specmint-core\n---\nin .specs/registry.md\n",
    )
    .unwrap();
    let muse = adapter(&env);
    let own = skill.to_string_lossy().into_owned();
    assert_eq!(
        muse.specmint(Variant::Core, ".specs").unwrap(),
        Skill::Global(own.clone())
    );
    // Its files call `.specs` the folder, and are never edited for one project (T-662): any
    // other folder is given ConsensFlow's own copy of it.
    assert!(matches!(
        muse.specmint(Variant::Core, "docs/specs").unwrap(),
        Skill::Own(copy) if copy != own
    ));
}

#[test]
fn a_chief_whose_window_is_open_is_given_the_skill_for_its_folder_where_a_launch_would_have_it() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.13-R6996.1"));
    let adapter = adapter(&env);
    let variant = Variant::Tdd;
    let Skill::Own(file) = adapter.specmint(variant, "docs/specs").unwrap() else {
        panic!("a copy of its own for another folder");
    };
    assert!(
        fs::metadata(&file).is_err(),
        "nothing is written before the window is told"
    );
    let skill = Skill::Own(file.clone());
    let specs = Specs {
        variant,
        folder: "docs/specs",
        skill: &skill,
    };
    let id = LaunchId::new(LAUNCH).unwrap();
    adapter.place_specmint(&specs, &id).unwrap();
    // No file of the copy sends the chief to `.specs`.
    let skill_folder = Path::new(&file).parent().unwrap();
    for entry in fs::read_dir(skill_folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            assert!(
                !fs::read_to_string(&path).unwrap().contains(".specs"),
                "{}",
                path.display()
            );
        }
    }
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../skill/specmint")
                .join(variant.as_str())
                .join("SKILL.md")
        )
        .unwrap()
        .replace(".specs", "docs/specs"),
        "the words a launch writes for the folder"
    );
    // Told again, it leaves what is there as it is.
    let first = fs::read(&file).unwrap();
    adapter.place_specmint(&specs, &id).unwrap();
    assert_eq!(fs::read(&file).unwrap(), first);
}

/// T-1246: Muse takes at most 16,384 bytes of a hook's output, and a chief's role is longer,
/// so a window whose role needs more than one hook is given one for each part, numbered, all
/// of them on the one role file: the part a hook says is its last word. And every window is
/// given all of them, whatever its role needs: a window can be put on a conversation an older
/// role left blocks in (a human's `/resume` inside it), and the hooks past the role's parts
/// write a filler over each.
#[test]
fn every_window_is_given_all_the_hooks_the_role_s_parts_first_and_a_filler_for_each_past_them() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.14-R7309.1"));
    for (name, role, parts) in [
        ("a role of one part", "# ConsensFlow worker".to_owned(), 1),
        // 480 lines of 71 bytes: 34 KB, three parts.
        ("a role of three", role_of(480), 3),
    ] {
        assert_eq!(crate::muse::parts(&role).len(), parts, "{name}");
        for resume in [None, Some("01a17bc9-e758-77f3-ac8d-a725edf373ae")] {
            let prepared = prepared_with(&env, resume, &role).unwrap();
            let hooks = hooks_of(&prepared.argv);
            let given = hooks["SessionStart"][0]["hooks"].as_array().unwrap();
            assert_eq!(
                given.len(),
                crate::muse::MOST_PARTS,
                "{name}, resume {resume:?}"
            );
            let numbers: Vec<String> = given
                .iter()
                .map(|hook| {
                    let command = hook["command"].as_str().unwrap();
                    command.rsplit_once(' ').unwrap().1.to_owned()
                })
                .collect();
            let wanted: Vec<String> = (0..crate::muse::MOST_PARTS)
                .map(|at| format!("'{at}'"))
                .collect();
            assert_eq!(numbers, wanted, "{name}");
            // Each says its block of the file the launch wrote, the same for every one.
            let files: Vec<_> = given
                .iter()
                .map(|hook| {
                    hook["command"]
                        .as_str()
                        .unwrap()
                        .split(" '")
                        .nth(1)
                        .unwrap()
                        .to_owned()
                })
                .collect();
            assert!(files.windows(2).all(|pair| pair[0] == pair[1]), "{files:?}");
        }
    }
}

#[test]
fn a_role_that_needs_more_hooks_than_muse_was_measured_to_take_is_refused_in_words() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.14-R7309.1"));
    // Past the most parts: 355 KB is about 23 parts of 16,000 bytes.
    let role = role_of(5_000);
    let refused = prepared_with(&env, None, &role).err().unwrap();
    assert!(
        refused.contains("355000 bytes")
            && refused.contains(&format!("{} hooks", crate::muse::MOST_PARTS)),
        "{refused}"
    );
    // 106 KB is 7 parts: the most a role that fits has is 8.
    let allowed = role_of(1_500);
    assert_eq!(crate::muse::parts(&allowed).len(), 7);
    assert!(prepared_with(&env, None, &allowed).is_ok());
}

/// Claude Code's own advisor tool (A5-20): Muse Code has none (a strict scan of 1.4.15's binary
/// for the word found only its public-suffix domain list), so a Muse member's launch is the
/// launch it always was whether the project has its members run with the tool or not (the
/// engine tells every harness the same; only Claude's adapter acts on it).
#[test]
fn a_muse_members_launch_is_the_same_whether_the_advisor_tool_is_switched_off_or_not() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.14-R7309.1"));
    let id = LaunchId::new(LAUNCH).unwrap();
    for role in ["worker", "advisor", "reviewer", "tester", "designer"] {
        for resume in [None, Some("0f8fad5b-d9cb-469f-a165-70867728950e")] {
            let prepare = |advisor_off: bool| {
                let launch = Launch {
                    role,
                    advisor_off,
                    ..launch(&id, resume)
                };
                finished(adapter(&env).prepare(&launch)).unwrap()
            };
            let (plain, off) = (prepare(false), prepare(true));
            assert_eq!(
                off.argv, plain.argv,
                "{role} {resume:?}: the same command line"
            );
            assert_eq!(
                off.env, plain.env,
                "{role} {resume:?}: the same environment"
            );
            assert_eq!(off.drop_env, plain.drop_env, "{role} {resume:?}");
            assert_eq!(
                off.native_session, plain.native_session,
                "{role} {resume:?}"
            );
        }
    }
}

#[test]
fn a_muse_windows_launch_is_the_same_whether_the_prompt_suggestion_is_switched_off_or_not() {
    // Muse Code 1.4.15 makes no such call (T-1434): the chief's launch as a member's, fresh or
    // resumed, is the launch it always was whatever the engine tells every harness.
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.14-R7309.1"));
    let id = LaunchId::new(LAUNCH).unwrap();
    for role in [
        "chief", "worker", "advisor", "reviewer", "tester", "designer",
    ] {
        for resume in [None, Some("0f8fad5b-d9cb-469f-a165-70867728950e")] {
            let prepare = |suggestion_off: bool| {
                let launch = Launch {
                    role,
                    suggestion_off,
                    ..launch(&id, resume)
                };
                finished(adapter(&env).prepare(&launch)).unwrap()
            };
            let (plain, off) = (prepare(false), prepare(true));
            assert_eq!(
                off.argv, plain.argv,
                "{role} {resume:?}: the same command line"
            );
            assert_eq!(
                off.env, plain.env,
                "{role} {resume:?}: the same environment"
            );
            assert_eq!(off.drop_env, plain.drop_env, "{role} {resume:?}");
            assert_eq!(
                off.native_session, plain.native_session,
                "{role} {resume:?}"
            );
        }
    }
}

/// The words a launch gives Muse its compaction limit by, where it gives one.
fn limits_of(argv: &[String]) -> Vec<String> {
    argv.windows(2)
        .filter(|pair| {
            pair[0] == "-c"
                && pair[1].starts_with("context_compaction.provider_context_limit_tokens=")
        })
        .map(|pair| pair[1].clone())
        .collect()
}

/// The size a project chose for its members to compact at (smaller-contexts, SCX-28 to
/// SCX-30, T-1265): Muse treats it as the model's context limit for the run, so it begins a
/// background compaction at 75% of it and a blocking one at 90% (its own fractions, left as
/// they are): it compacts by the size, never past it. Muse takes any limit, and a limit above
/// the model's real window would put both points past it, so a size above
/// [`super::REAL_WINDOW`] gives nothing, which leaves Muse's own point.
#[test]
fn a_members_window_is_given_a_size_up_to_muse_s_real_window_as_its_limit_and_none_past_it() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.14-R7309.1"));
    let id = LaunchId::new(LAUNCH).unwrap();
    for role in ["worker", "advisor", "reviewer", "tester", "designer"] {
        let argv = |compaction: Option<u32>| {
            let launch = Launch {
                role,
                compaction,
                ..launch(&id, None)
            };
            finished(adapter(&env).prepare(&launch)).unwrap().argv
        };
        let plain = argv(None);
        assert_eq!(
            limits_of(&plain),
            Vec::<String>::new(),
            "{role}: none chosen"
        );
        for size in [100_000, 400_000, super::REAL_WINDOW] {
            let given = argv(Some(size));
            assert_eq!(
                limits_of(&given),
                [format!(
                    "context_compaction.provider_context_limit_tokens={size}"
                )],
                "{role} at {size}"
            );
            // Nothing else of the command line moves, and the hooks are still the first `-c`.
            let without: Vec<String> = given
                .iter()
                .filter(|word| !word.starts_with("context_compaction."))
                .cloned()
                .collect();
            let mut plain_without = plain.clone();
            let mut given_without = without.clone();
            // The `-c` of the limit, which leaves an empty pair where it was taken out.
            let dashes = |words: &[String]| words.iter().filter(|word| *word == "-c").count();
            assert_eq!(dashes(&given), dashes(&plain) + 1, "{role} at {size}");
            plain_without.retain(|word| word != "-c");
            given_without.retain(|word| word != "-c");
            assert_eq!(given_without, plain_without, "{role} at {size}");
            assert_eq!(hooks_of(&given), hooks_of(&plain), "{role} at {size}");
        }
        for size in [super::REAL_WINDOW + 1, 600_000, 1_000_000] {
            assert_eq!(
                argv(Some(size)),
                plain,
                "{role} at {size}: Muse's own point"
            );
        }
    }
}

#[test]
fn the_size_is_given_to_a_window_resumed_as_to_one_launched() {
    let home = tempfile::tempdir().unwrap();
    let env = installed(home.path(), Some("1.4.14-R7309.1"));
    let id = LaunchId::new(LAUNCH).unwrap();
    let launch = Launch {
        compaction: Some(250_000),
        advisor_off: false,
        ..launch(&id, Some("01a17923-56fa-7f60-9d86-84709f9d4b25"))
    };
    let argv = finished(adapter(&env).prepare(&launch)).unwrap().argv;
    assert!(argv.iter().any(|word| word == "resume"), "{argv:?}");
    assert_eq!(
        limits_of(&argv),
        ["context_compaction.provider_context_limit_tokens=250000"]
    );
}

/// The smallest real window of the models Muse lists here, as Muse 1.4.14 measured it: 512,000
/// tokens for muse-spark-1.3-contributor (its soft and hard points at 0.03 and 0.9 were 15,360
/// and 460,800, live on 2026-11-06, and 384,000 and 460,800 in the recorded
/// `live/contributor-full-run.jsonl`), though its catalog says 1,007,997. Unproven for
/// muse-wonder-1.0, muse-spark-1.3 and muse-spark-1.2, whose sessions recorded no threshold.
#[test]
fn muse_s_real_window_is_the_smallest_one_measured() {
    assert_eq!(super::REAL_WINDOW, 512_000);
}
