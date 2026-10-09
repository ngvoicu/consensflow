//! A publish cut short while it makes the versioned release, and while it
//! moves the feeds: running it again finishes it. And only the push of a
//! version tag publishes, from the command line.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

use cf_publish::testing::worlds::{before, changes, did, mentioning};
use cf_publish::testing::{gh_dir, published_assets, run_binary, Github, Spec, BRIDGE, LATER};

const LATEST: &str = "latest.json";
const NEXT: &str = "latest.next.json";

fn args_include(args: &[String], word: &str) -> bool {
    args.iter().any(|arg| arg == word)
}

mod a_publish_cut_short_while_it_makes_the_versioned_release {
    use super::*;

    #[test]
    fn leaves_a_draft_that_is_no_release_and_running_it_again_fills_it_and_publishes_it() {
        let world = before(BRIDGE);
        let github = &world.github;
        let tag = format!("v{BRIDGE}");
        // The upload of the files dies after the draft is made.
        let uploading = tag.clone();
        github.fail(move |args, _| {
            (args[1] == "upload" && args[2] == uploading).then(|| "upload interrupted".to_string())
        });
        let cut = world.publish().unwrap_err().to_string();
        assert!(cut.contains("upload interrupted"), "{cut}");
        assert!(github.is_draft(&tag), "not public");
        assert!(!github.has("feed-alpha"), "no feed moved");
        github.clear_fail();
        let done = world.publish().unwrap();
        assert_eq!(done.release.to_string(), "remade");
        assert!(!github.is_draft(&tag));
        assert_eq!(world.check(), Vec::<String>::new());
    }

    #[test]
    fn empties_a_draft_that_holds_some_of_the_files_a_short_one_among_them_and_fills_it_again() {
        let world = before(BRIDGE);
        let github = &world.github;
        let tag = format!("v{BRIDGE}");
        let dmg = format!("ConsensFlow_{BRIDGE}_aarch64.dmg");
        github.release(
            &tag,
            Spec::new()
                .draft()
                .prerelease()
                .assets([(dmg.as_str(), "a short upload"), ("stray.txt", "left over")]),
        );
        let done = world.publish().unwrap();
        assert_eq!(done.release.to_string(), "remade");
        assert!(
            !github
                .names(&tag)
                .unwrap()
                .contains(&"stray.txt".to_string()),
            "the draft was emptied"
        );
        assert_eq!(github.asset_text(&tag, &dmg), world.files.text(&dmg));
        assert_eq!(world.check(), Vec::<String>::new());
    }

    #[test]
    fn does_not_publish_a_draft_whose_file_is_short_and_says_so() {
        let world = before(BRIDGE);
        let github = &world.github;
        let tag = format!("v{BRIDGE}");
        let dmg = format!("ConsensFlow_{BRIDGE}_aarch64.dmg");
        // The second look at the release is the one after its files are uploaded: one of them is found short.
        let (handle, looked_at, short) = (github.handle(), tag.clone(), dmg.clone());
        let mut views = 0;
        github.fail(move |args, _| {
            if args[1] == "view" && args[2] == looked_at {
                views += 1;
                if views == 2 {
                    handle.put(&looked_at, &short, "cut");
                }
            }
            None
        });
        let refused = world.publish().unwrap_err().to_string();
        assert!(
            refused.contains(&format!("{dmg} as 3 bytes, uploaded, not as the "))
                && refused.contains(" built"),
            "{refused}"
        );
        assert!(github.is_draft(&tag), "still not public");
        assert_eq!(
            mentioning(&github.calls(), "feed-alpha"),
            Vec::<Vec<String>>::new()
        );
    }

    #[test]
    fn publishes_the_draft_when_the_run_died_at_the_very_publishing_and_the_rest_follows() {
        let world = before(BRIDGE);
        let github = &world.github;
        let tag = format!("v{BRIDGE}");
        let publishing = tag.clone();
        github.fail(move |args, _| {
            (args[1] == "edit" && args[2] == publishing).then(|| "died".to_string())
        });
        let cut = world.publish().unwrap_err().to_string();
        assert!(cut.contains("died"), "{cut}");
        assert!(github.is_draft(&tag));
        github.clear_fail();
        world.publish().unwrap();
        assert!(!github.is_draft(&tag));
        assert_eq!(world.check(), Vec::<String>::new());
    }

    #[test]
    fn adds_what_a_published_release_lacks_and_holds_what_it_has_to_the_files_built() {
        let world = before(BRIDGE);
        let github = &world.github;
        let tag = format!("v{BRIDGE}");
        let portable = format!("ConsensFlow_{BRIDGE}_x64-portable.exe");
        let held: Vec<(String, Vec<u8>)> = published_assets(&world.files)
            .iter()
            .filter(|(name, _)| *name != portable)
            .map(|(name, data)| (name.to_string(), data.to_vec()))
            .collect();
        github.release(&tag, Spec::new().prerelease().assets(held));
        let done = world.publish().unwrap();
        assert_eq!(done.release.to_string(), "completed");
        let uploads: Vec<Vec<String>> = github
            .calls()
            .into_iter()
            .filter(|args| args[1] == "upload")
            .collect();
        assert_eq!(
            uploads,
            [
                vec![
                    "release".to_string(),
                    "upload".to_string(),
                    tag,
                    format!("portable/{portable}")
                ],
                vec![
                    "release".to_string(),
                    "upload".to_string(),
                    "feed-alpha".to_string(),
                    LATEST.to_string()
                ],
                vec![
                    "release".to_string(),
                    "upload".to_string(),
                    "update-alpha".to_string(),
                    NEXT.to_string()
                ],
            ],
            "only the missing file"
        );
        assert_eq!(world.check(), Vec::<String>::new());
    }

    #[test]
    fn refuses_a_published_release_whose_file_is_not_the_one_built_and_moves_no_feed() {
        let world = before(BRIDGE);
        let github = &world.github;
        let tag = format!("v{BRIDGE}");
        let archive = format!("ConsensFlow_{BRIDGE}_aarch64.app.tar.gz");
        let published =
            published_assets(&world.files).with(&archive, "an archive from another build");
        github.release(&tag, Spec::new().prerelease().assets(published.iter()));
        let refused = world.publish().unwrap_err().to_string();
        let words = format!(
            "the release {tag} (kept) does not hold the files built here, and no feed was moved: a published release's files are not replaced"
        );
        assert!(refused.starts_with(&words), "{refused}");
        assert!(
            refused.contains(&format!(
                ":\n- {archive} does not download from {tag} as the one built"
            )),
            "{refused}"
        );
        assert_eq!(
            changes(&github.calls()),
            Vec::<Vec<String>>::new(),
            "nothing changed at all"
        );
        assert!(!github.has("feed-alpha"));
    }
}

mod a_publish_cut_short_while_it_moves_the_feeds {
    use super::*;

    #[test]
    fn moves_the_feeds_that_are_left_and_leaves_what_is_done_as_it_is() {
        let world = before(BRIDGE);
        let github = &world.github;
        github.fail(|args, _| {
            args_include(args, "update-alpha").then(|| "died at the old feed".to_string())
        });
        let cut = world.publish().unwrap_err().to_string();
        assert!(cut.contains("died at the old feed"), "{cut}");
        github.clear_fail();
        let asked = github.calls().len();
        let done = world.publish().unwrap();
        assert_eq!(did(&done), ["feed-alpha kept", "update-alpha replaced"]);
        assert_eq!(done.release.to_string(), "kept");
        let creates = github.calls()[asked..]
            .iter()
            .filter(|args| args[1] == "create")
            .count();
        assert_eq!(creates, 0, "no release is made again");
        assert_eq!(world.check(), Vec::<String>::new());
    }

    #[test]
    fn does_nothing_at_all_to_a_release_and_its_feeds_that_are_done_which_pinned_notes_aside() {
        let world = before(BRIDGE);
        let github = &world.github;
        world.publish().unwrap();
        let asked = github.calls().len();
        let again = world.publish().unwrap();
        assert_eq!(again.version.as_str(), BRIDGE);
        assert_eq!(again.release.to_string(), "kept");
        assert_eq!(did(&again), ["feed-alpha kept", "update-alpha kept"]);
        assert_eq!(
            changes(&github.calls()[asked..]),
            [[
                "release",
                "edit",
                "update-alpha",
                "--title",
                "Alpha update feed, pinned",
                "--notes",
                github.notes("update-alpha").as_str(),
            ]],
            "only the pin, which says the same again"
        );
    }

    #[test]
    fn stops_where_gh_cannot_tell_whether_a_release_is_there_and_makes_nothing() {
        let world = before(BRIDGE);
        let github = &world.github;
        github.fail(|args, _| (args[1] == "view").then(|| "HTTP 401: Bad credentials".to_string()));
        let refused = world.publish().unwrap_err().to_string();
        assert!(
            refused.contains("could not look at the release v3.0.0-alpha.81: HTTP 401"),
            "{refused}"
        );
        assert_eq!(changes(&github.calls()), Vec::<Vec<String>>::new());
    }
}

mod only_the_push_of_a_version_tag_publishes_from_the_command_line {
    use super::*;

    const REPO: &str = "ngvoicu/consensflow";

    /// The binary run with a `gh` first on its PATH that asks the simulator, to
    /// tell if it was ever run, and the environment given.
    fn run(github: &Github, args: &[&str], env: &[(&str, &str)]) -> cf_publish::testing::Ran {
        let bin = gh_dir(Path::new(env!("CARGO_BIN_EXE_fake-gh")));
        let mut env = env.to_vec();
        env.push(("GH_SIM", github.base()));
        // A token, so that a `gh` that was run would reach the simulator, and be seen.
        env.push(("GH_TOKEN", "a-token"));
        run_binary(
            Path::new(env!("CARGO_BIN_EXE_cf-publish")),
            args,
            &env,
            Some(bin.path()),
        )
    }

    fn tag_push() -> Vec<(&'static str, String)> {
        vec![
            ("GITHUB_EVENT_NAME", "push".to_string()),
            ("GITHUB_REF_TYPE", "tag".to_string()),
            ("GITHUB_REF_NAME", format!("v{BRIDGE}")),
            ("GH_REPO", REPO.to_string()),
        ]
    }

    fn with<'a>(
        base: &'a [(&'static str, String)],
        change: (&'static str, &'a str),
    ) -> Vec<(&'a str, &'a str)> {
        let mut env: Vec<(&str, &str)> = base
            .iter()
            .filter(|(name, _)| *name != change.0)
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        env.push(change);
        env
    }

    fn arguments(tag: &str) -> Vec<String> {
        [
            "publish",
            "--dir",
            ".",
            "--tag",
            tag,
            "--base",
            "http://127.0.0.1:1",
        ]
        .iter()
        .map(|word| (*word).to_string())
        .collect()
    }

    #[test]
    fn refuses_a_hand_run_though_it_is_on_a_tag_before_it_asks_gh_anything() {
        let github = Github::start();
        let base = tag_push();
        let args = arguments(&format!("v{BRIDGE}"));
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let ran = run(
            &github,
            &args,
            &with(&base, ("GITHUB_EVENT_NAME", "workflow_dispatch")),
        );
        assert_eq!(ran.status, 1);
        assert!(
            ran.stderr.lines().any(|line| line == "publish: only the push of a version tag publishes; this is workflow_dispatch on tag"),
            "{}",
            ran.stderr
        );
        assert_eq!(github.calls(), Vec::<Vec<String>>::new(), "gh was not run");
    }

    #[test]
    fn refuses_a_push_of_a_branch_a_run_outside_a_workflow_and_a_tag_that_is_not_the_one_pushed() {
        let base = tag_push();
        let tag = format!("v{BRIDGE}");
        let args = arguments(&tag);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let later_tag = format!("v{LATER}");
        let cases: [(Vec<(&str, &str)>, String); 3] = [
            (
                with(&base, ("GITHUB_REF_TYPE", "branch")),
                "this is push on branch".to_string(),
            ),
            (
                vec![("GH_REPO", REPO)],
                "this is not a workflow on nothing".to_string(),
            ),
            (
                with(&base, ("GITHUB_REF_NAME", later_tag.as_str())),
                format!("this run is for {later_tag}, and it was asked to publish {tag}"),
            ),
        ];
        for (env, says) in cases {
            let github = Github::start();
            let ran = run(&github, &args, &env);
            assert_eq!(ran.status, 1, "{env:?}");
            assert!(ran.stderr.contains(&says), "{}", ran.stderr);
            assert_eq!(github.calls(), Vec::<Vec<String>>::new(), "{env:?}");
        }
    }

    #[test]
    fn says_what_it_lacks() {
        let github = Github::start();
        let base = tag_push();
        let env: Vec<(&str, &str)> = base
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let ran = run(&github, &["publish"], &env);
        assert_eq!(ran.status, 1);
        assert!(
            ran.stderr
                .contains("publish needs --dir, --tag and --base, and the repository (GH_REPO)"),
            "{}",
            ran.stderr
        );
    }
}
