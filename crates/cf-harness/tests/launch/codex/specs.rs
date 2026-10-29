//! The Specmint skill a Codex chief is given where its project has specs on:
//! ConsensFlow's own copy, kept where it is the same for every launch, and read
//! by its path, which the role text names; or the human's own, where Codex has
//! the variant installed, and then nothing is written for it. A launch's own
//! folder holds its role's file and nothing else, with specs on or off.

use std::fs;
use std::path::Path;

use cf_base::path;
use cf_harness::contract::{Adapter, Skill};
use cf_harness::testing::unrooted;
use cf_proto::specs::Variant;

use super::{adapter, prepare, Home, Request, LAUNCH};
use crate::kept;

/// ConsensFlow's own folder in `home`, which the launch's window is given.
fn consensflow(home: &Home) -> String {
    path::join(&[&home.root, "consensflow"])
}

/// Where the launch's own files for Codex are: its role's alone.
fn launch_folder(home: &Home) -> String {
    path::join(&[&consensflow(home), "integrations", "codex", LAUNCH])
}

/// Every file under ConsensFlow's `integrations` in `home`, by its path below it.
fn integrations(home: &Home) -> Vec<String> {
    fn walk(folder: &Path, below: &str, found: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        for entry in entries {
            let entry = entry.unwrap();
            let name = format!("{below}{}", entry.file_name().to_string_lossy());
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), &format!("{name}/"), found);
            } else {
                found.push(name);
            }
        }
    }
    let mut found = Vec::new();
    walk(
        &Path::new(&consensflow(home)).join("integrations"),
        "",
        &mut found,
    );
    found.sort();
    found
}

/// What a launch writes of its own: its role's file.
fn the_role_alone() -> Vec<String> {
    vec![format!("codex/{LAUNCH}/developer-instructions.md")]
}

#[test]
fn a_chief_given_specs_has_the_skill_written_where_its_role_text_names_it_and_codex_is_told_nothing_more(
) {
    for variant in Variant::ALL {
        // Two homes of one make: what differs between the launches is the specs.
        let (off_home, home) = (Home::new(), Home::new());
        let (off_adapter, _fakes) = adapter(&off_home);
        let (adapter, _fakes) = adapter(&home);
        let off = prepare(&off_adapter, &Request::chief()).unwrap();
        assert_eq!(
            integrations(&off_home),
            the_role_alone(),
            "with specs off Codex writes its role and nothing else"
        );
        let request = Request {
            specs: Some(variant),
            ..Request::chief()
        };
        let on = prepare(&adapter, &request).unwrap();
        assert_eq!(
            unrooted(&on, &home.root),
            unrooted(&off, &off_home.root),
            "{variant:?}: Codex reads it by its path, and is registered nothing"
        );
        let skill = path::join(&[
            &kept::skill(&consensflow(&home), "codex", variant),
            "SKILL.md",
        ]);
        assert_eq!(
            adapter.specmint(variant).unwrap(),
            Skill::Own(skill.clone()),
            "{variant:?}: the place the role text names is the place written"
        );
        assert!(
            fs::read_to_string(&skill)
                .unwrap()
                .starts_with(&format!("---\nname: {}\n", variant.skill_name())),
            "{variant:?}"
        );
        assert_eq!(
            kept::names_in(&kept::skills(&consensflow(&home), "codex", variant)),
            [variant.skill_name()],
            "the skill alone: Codex's role is in the launch's folder"
        );
        assert_eq!(
            fs::read_dir(launch_folder(&home)).unwrap().count(),
            1,
            "the launch's folder holds its role alone: the copy outlasts it"
        );
    }
}

#[test]
fn a_second_launch_names_the_same_skill_whatever_its_id_and_its_text_names_it_the_same() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let request = Request {
        specs: Some(Variant::TddHtml),
        ..Request::chief()
    };
    // The place the role text names, asked before either launch: the adapter
    // is given no launch to ask it for.
    let named = adapter.specmint(Variant::TddHtml).unwrap();
    prepare(&adapter, &request).unwrap();
    prepare(&adapter, &request).unwrap();
    assert_eq!(adapter.specmint(Variant::TddHtml).unwrap(), named);
    assert!(
        Path::new(named.path()).is_file(),
        "{} is where it was written",
        named.path()
    );
}

#[test]
fn a_codex_that_has_the_variant_installed_in_the_agents_skills_or_its_own_is_given_nothing_to_write_and_another_variant_installed_is_left_alone(
) {
    let home = Home::new();
    let install = |folder: &str, name: &str| {
        let dir = path::join(&[&home.root, "home", folder, name]);
        fs::create_dir_all(&dir).unwrap();
        let file = path::join(&[&dir, "SKILL.md"]);
        fs::write(&file, format!("---\nname: {name}\n---\nthe human's own\n")).unwrap();
        file
    };
    let agents = install(".agents/skills", "specmint-tdd");
    let own = install(".codex/skills", "specmint-core-html");
    let (adapter, _fakes) = adapter(&home);
    assert_eq!(
        adapter.specmint(Variant::Tdd).unwrap(),
        Skill::Global(agents.clone())
    );
    assert_eq!(
        adapter.specmint(Variant::CoreHtml).unwrap(),
        Skill::Global(own.clone())
    );
    for variant in [Variant::Tdd, Variant::CoreHtml] {
        let request = Request {
            specs: Some(variant),
            ..Request::chief()
        };
        prepare(&adapter, &request).unwrap();
    }
    assert_eq!(
        integrations(&home),
        the_role_alone(),
        "an installed copy is the harness's own: nothing is written for it"
    );
    // Neither installed variant is the core one: ConsensFlow's own is written.
    assert!(matches!(
        adapter.specmint(Variant::Core).unwrap(),
        Skill::Own(_)
    ));
    let (before_agents, before_own) = (fs::read(&agents).unwrap(), fs::read(&own).unwrap());
    let request = Request {
        specs: Some(Variant::Core),
        ..Request::chief()
    };
    prepare(&adapter, &request).unwrap();
    assert!(Path::new(&kept::skill(&consensflow(&home), "codex", Variant::Core)).exists());
    assert_eq!(fs::read(&agents).unwrap(), before_agents);
    assert_eq!(fs::read(&own).unwrap(), before_own);
}
