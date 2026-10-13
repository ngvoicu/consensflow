use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::testing::{exists, Fake};
use super::*;

/// The folder the variants are vendored in, from the checkout.
fn vendored() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("skill")
        .join("specmint")
}

/// Every file under `folder`, by its name under it in slashes.
fn files_under(folder: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, folder: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(folder).expect("a folder") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                walk(base, &path, found);
            } else {
                let name = path
                    .strip_prefix(base)
                    .expect("under the base")
                    .to_string_lossy()
                    .replace('\\', "/");
                found.insert(name, fs::read(&path).expect("a file"));
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(folder, folder, &mut found);
    found
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn the_table_the_folder_and_the_manifest_hold_the_same_files_with_the_same_bytes() {
    let manifest: Value =
        serde_json::from_slice(&fs::read(vendored().join("manifest.json")).expect("the manifest"))
            .expect("the manifest is JSON");
    let named: Vec<&str> = manifest
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        named,
        ["core", "core-html", "tdd", "tdd-html"],
        "the manifest has the four variants, in the order the page offers them"
    );
    for variant in Variant::ALL {
        let embedded: BTreeMap<String, Vec<u8>> = files::of(variant)
            .iter()
            .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
            .collect();
        assert_eq!(
            embedded.len(),
            files::of(variant).len(),
            "{variant:?}: a file is in the table once"
        );
        let folder = files_under(&vendored().join(variant.as_str()));
        assert_eq!(
            embedded.keys().collect::<Vec<_>>(),
            folder.keys().collect::<Vec<_>>(),
            "{variant:?}: the table names the files of its folder, and no other"
        );
        assert_eq!(
            embedded, folder,
            "{variant:?}: the table holds the folder's bytes"
        );
        let entry = &manifest[variant.as_str()];
        let commit = entry["commit"].as_str().expect("a commit");
        assert!(
            commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "{variant:?}: {commit}"
        );
        let hashed: BTreeMap<String, String> = entry["files"]
            .as_object()
            .expect("the files")
            .iter()
            .map(|(name, hash)| (name.clone(), hash.as_str().expect("a hash").to_owned()))
            .collect();
        let held: BTreeMap<String, String> = embedded
            .iter()
            .map(|(name, bytes)| (name.clone(), sha256(bytes)))
            .collect();
        assert_eq!(
            hashed, held,
            "{variant:?}: the manifest hashes the table's files"
        );
    }
}

#[test]
fn every_variant_ships_its_skill_under_the_name_its_variant_gives_it_and_the_licence() {
    for variant in Variant::ALL {
        let skill = files::of(variant)
            .iter()
            .find(|(name, _)| *name == "SKILL.md")
            .expect("a SKILL.md");
        assert_eq!(
            frontmatter_name(&String::from_utf8_lossy(skill.1)),
            Some(variant.skill_name()),
            "{variant:?}"
        );
        assert!(
            files::of(variant)
                .iter()
                .any(|(name, _)| *name == "LICENSE"),
            "{variant:?}: the licence travels with the skill"
        );
        let html = files::of(variant)
            .iter()
            .any(|(name, _)| name.starts_with("assets/"));
        assert_eq!(html, variant.as_str().ends_with("html"), "{variant:?}");
    }
}

#[test]
fn a_skill_is_named_by_the_frontmatter_at_the_start_of_its_file_and_nowhere_else() {
    for (text, named) in [
        ("---\nname: specmint-core\n---\n", Some("specmint-core")),
        (
            "---\r\nname: specmint-tdd\r\ndescription: x\r\n---\r\n",
            Some("specmint-tdd"),
        ),
        (
            "---\ndescription: x\nname:   \"specmint-core-html\"  \n---\n",
            Some("specmint-core-html"),
        ),
        (
            "---\nname: 'specmint-tdd-html'\n---\n",
            Some("specmint-tdd-html"),
        ),
        // Not at the start: no frontmatter.
        ("\n---\nname: specmint-core\n---\n", None),
        ("# Title\n---\nname: specmint-core\n---\n", None),
        // After the frontmatter closed: the body's.
        ("---\ndescription: x\n---\nname: specmint-core\n", None),
        // Indented: part of another key's value.
        ("---\ndescription: >\n  name: specmint-core\n---\n", None),
        ("---\nname: specmint-core\n", Some("specmint-core")),
        ("", None),
        ("---", None),
    ] {
        assert_eq!(frontmatter_name(text), named, "{text:?}");
    }
}

#[test]
fn an_installed_variant_is_found_by_its_skills_name_in_the_first_folder_that_has_it() {
    let fake = Fake::new();
    let (first, second) = (fake.at("one"), fake.at("two"));
    let roots = [first.clone(), second.clone()];
    assert_eq!(installed(&roots, Variant::Core), None, "no folder at all");
    fs::create_dir_all(&first).expect("a folder");
    assert_eq!(installed(&roots, Variant::Core), None, "no skill in it");
    // Another variant is no copy of this one.
    fake.install("two", "specmint-tdd", "specmint-tdd");
    assert_eq!(installed(&roots, Variant::Core), None);
    let tdd = path::join(&[&second, "specmint-tdd", "SKILL.md"]);
    assert_eq!(installed(&roots, Variant::Tdd), Some(tdd));
    // The first folder read wins.
    let core = fake.install("two", "specmint-core", "specmint-core");
    assert_eq!(installed(&roots, Variant::Core), Some(core));
    let in_first = fake.install("one", "any-name", "specmint-core");
    assert_eq!(installed(&roots, Variant::Core), Some(in_first));
}

#[test]
fn a_folder_with_no_skill_file_or_no_frontmatter_has_no_skill_and_stops_nothing() {
    let fake = Fake::new();
    let root = fake.at("skills");
    fs::create_dir_all(path::join(&[&root, "empty"])).expect("a folder");
    fs::create_dir_all(path::join(&[&root, "a-file"])).expect("a folder");
    fs::write(
        path::join(&[&root, "stray.md"]),
        "---\nname: specmint-core\n---\n",
    )
    .expect("a file");
    fs::write(
        path::join(&[&root, "a-file", "SKILL.md"]),
        "no frontmatter here\n",
    )
    .expect("a file");
    let wanted = fake.install("skills", "z-last", "specmint-core");
    assert_eq!(installed(&[root], Variant::Core), Some(wanted));
}

/// A launch of a chief on Claude with `specs`, over a home of the test's own.
fn launch<'a>(id: &'a LaunchId, specs: Option<Specs<'a>>) -> Launch<'a> {
    Launch {
        id,
        project: 1,
        handle: "chief",
        role: "chief",
        directory: "/work",
        resume: None,
        message: None,
        agent: None,
        instructions: "role text",
        specs,
        first_message_ms: 1,
    }
}

fn id() -> LaunchId {
    LaunchId::new("11111111-1111-4111-8111-111111111111").expect("a launch id")
}

fn home_env(fake: &Fake) -> Env {
    Env::from_vars([("CONSENSFLOW_HOME", fake.at("cf"))])
}

#[test]
fn a_variant_not_found_installed_is_placed_beside_the_role_in_the_launch_s_folder() {
    let fake = Fake::new();
    let env = home_env(&fake);
    let id = id();
    for (harness, folder) in [
        (Harness::Claude, "claude"),
        (Harness::Pi, "pi"),
        (Harness::Devin, "devin"),
        (Harness::Opencode, "opencode"),
        (Harness::Codex, "codex"),
    ] {
        for variant in Variant::ALL {
            let placed = place(harness, &env, &id, variant, None).expect("a place");
            assert_eq!(
                placed,
                Skill::Own(path::join(&[
                    &fake.at("cf"),
                    "integrations",
                    folder,
                    id.as_str(),
                    "role",
                    ".claude",
                    "skills",
                    variant.skill_name(),
                    "SKILL.md"
                ])),
                "{harness:?} {variant:?}"
            );
        }
    }
}

#[test]
fn an_installed_variant_is_placed_where_it_is_and_its_place_needs_no_home() {
    let id = id();
    let placed = place(
        Harness::Claude,
        &Env::default(),
        &id,
        Variant::Tdd,
        Some("/home/me/.claude/skills/specmint-tdd/SKILL.md".to_owned()),
    );
    assert_eq!(
        placed,
        Ok(Skill::Global(
            "/home/me/.claude/skills/specmint-tdd/SKILL.md".to_owned()
        ))
    );
    let own = place(Harness::Claude, &Env::default(), &id, Variant::Tdd, None);
    assert_eq!(own, Err("missing home in env".to_owned()));
}

#[test]
fn the_own_copy_is_written_whole_beside_the_role_s_skill_and_its_place_is_where_the_role_text_named_it(
) {
    for variant in Variant::ALL {
        let fake = Fake::new();
        let env = home_env(&fake);
        let id = id();
        let skill = place(Harness::Claude, &env, &id, variant, None).expect("a place");
        let specs = Specs {
            variant,
            skill: &skill,
        };
        let launch = launch(&id, Some(specs));
        let role =
            crate::shared::role::write_role(Harness::Claude, "chief", &env, &id, "role text")
                .expect("the role");
        let written = write_own(Harness::Claude, &env, &launch).expect("written");
        assert_eq!(written.as_deref(), Some(skill.path()), "{variant:?}");
        let folder = Path::new(skill.path()).parent().expect("a folder");
        assert_eq!(
            folder.parent(),
            Path::new(&role.file).parent().and_then(Path::parent),
            "beside the role's skill, in the same skills folder"
        );
        assert_eq!(
            files_under(folder),
            files::of(variant)
                .iter()
                .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
                .collect::<BTreeMap<_, _>>(),
            "{variant:?}: every file, as it is vendored"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |file: &str| {
                fs::metadata(path::join(&[&folder.to_string_lossy(), file]))
                    .expect("a file")
                    .permissions()
                    .mode()
                    & 0o777
            };
            assert_eq!(
                mode("SKILL.md"),
                0o600,
                "private to this user, as the role is"
            );
            assert_eq!(mode("references"), 0o700);
        }
    }
}

#[test]
fn nothing_is_written_for_a_launch_with_no_specs_or_whose_harness_has_the_variant() {
    let fake = Fake::new();
    let env = home_env(&fake);
    let id = id();
    assert_eq!(
        write_own(Harness::Claude, &env, &launch(&id, None)),
        Ok(None)
    );
    let skill = Skill::Global("/home/me/.claude/skills/specmint-core/SKILL.md".to_owned());
    let specs = Specs {
        variant: Variant::Core,
        skill: &skill,
    };
    assert_eq!(
        write_own(Harness::Claude, &env, &launch(&id, Some(specs))),
        Ok(None)
    );
    assert!(!exists(&fake.at("cf")), "nothing of ConsensFlow's was made");
}

#[test]
fn a_window_that_has_no_home_to_write_in_says_so() {
    let id = id();
    let skill = Skill::Own("/nowhere/SKILL.md".to_owned());
    let specs = Specs {
        variant: Variant::Core,
        skill: &skill,
    };
    let none = Env::default();
    assert_eq!(
        write_own(Harness::Codex, &none, &launch(&id, Some(specs))),
        Err("missing home in env".to_owned())
    );
}
