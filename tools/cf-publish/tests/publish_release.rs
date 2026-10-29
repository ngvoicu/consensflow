//! The publisher run against GitHub as the simulator has it: the bridge, a
//! later release, and the bridge that failed before it moved update-alpha with
//! the release after it. Every half-done state a run can be cut short in is
//! finished by running it again (tests/publish_cut_short.rs,
//! tests/publish_swap.rs); no feed is ever moved backward
//! (tests/publish_rerun.rs).

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;

use cf_publish::testing::signing::{sign, sign_legacy};
use cf_publish::testing::worlds::{before, before_with, did, gap, mentioning, releasing};
use cf_publish::testing::{latest_json, published_assets, Served, BRIDGE, LATER, OLDER};

const LATEST: &str = "latest.json";
const PREVIOUS: &str = "latest.previous.json";
const ROOT: &str = "ConsensFlow.app/Contents/";

fn args_include(args: &[String], word: &str) -> bool {
    args.iter().any(|arg| arg == word)
}

mod publishing_the_bridge {
    use super::*;

    #[test]
    fn makes_the_versioned_release_whole_and_public_then_moves_the_new_feed_and_the_old_one() {
        let world = before(BRIDGE);
        let done = world.publish().unwrap();
        assert_eq!(done.version.as_str(), BRIDGE);
        assert_eq!(done.release.to_string(), "created");
        assert_eq!(did(&done), ["feed-alpha created", "update-alpha replaced"]);
        let github = &world.github;
        let tag = format!("v{BRIDGE}");
        assert!(!github.is_draft(&tag), "published");
        assert!(github.is_prerelease(&tag));
        assert_eq!(github.title(&tag), format!("ConsensFlow {BRIDGE}"));
        let mut names: Vec<String> = published_assets(&world.files)
            .names()
            .iter()
            .filter(|name| **name != "SHA256SUMS")
            .map(|name| (*name).to_string())
            .collect();
        names.push("SHA256SUMS".to_string());
        names.sort();
        assert_eq!(github.names(&tag).unwrap(), names);
        assert_eq!(github.names("feed-alpha").unwrap(), [LATEST]);
        assert_eq!(github.names("update-alpha").unwrap(), [LATEST]);
        for feed in ["feed-alpha", "update-alpha"] {
            assert_eq!(
                github.asset_text(feed, LATEST),
                world.files.text(LATEST),
                "{feed}"
            );
        }
        assert_eq!(
            world.check(),
            Vec::<String>::new(),
            "the feeds serve what the rule says"
        );
        assert!(world
            .log()
            .iter()
            .any(|line| line.starts_with("gh release create")));
    }

    #[test]
    fn makes_the_release_a_draft_and_publishes_it_only_once_every_file_is_there() {
        let world = before(BRIDGE);
        world.publish().unwrap();
        let calls = world.github.calls();
        let tag = format!("v{BRIDGE}");
        let create = calls.iter().position(|args| args[1] == "create").unwrap();
        assert_eq!(
            calls[create][..5],
            ["release", "create", tag.as_str(), "--draft", "--verify-tag"]
        );
        assert!(args_include(&calls[create], "--prerelease"));
        let publishing = calls
            .iter()
            .position(|args| {
                args[1] == "edit" && args.get(3).map(String::as_str) == Some("--draft=false")
            })
            .unwrap();
        let upload = calls
            .iter()
            .position(|args| args[1] == "upload" && args[2] == tag)
            .unwrap();
        assert!(
            create < upload && upload < publishing,
            "created, filled, then published"
        );
        let filled = world.github.trace()[publishing - 1].after[&tag].clone();
        assert!(filled.draft, "the release was a draft until then");
        assert_eq!(
            filled.assets.len(),
            8,
            "with all its files, SHA256SUMS and its signature"
        );
    }

    #[test]
    fn writes_the_sums_and_the_release_page_as_the_workflow_always_has() {
        let world = before(BRIDGE);
        world.publish().unwrap();
        let sums = fs::read_to_string(world.dir.path().join("SHA256SUMS")).unwrap();
        assert_eq!(
            sums,
            published_assets(&world.files).text("SHA256SUMS"),
            "one line per file, as sha256sum writes it"
        );
        let tag = format!("v{BRIDGE}");
        assert_eq!(world.github.asset_text(&tag, "SHA256SUMS"), sums);
        let body = world.github.notes(&tag);
        assert!(
            body.starts_with(&format!(
                "ConsensFlow {BRIDGE}, the notes\n\n**Downloads.**"
            )),
            "{body}"
        );
        assert!(
            body.contains("%LOCALAPPDATA%\\dev.ngvoicu.consensflow\\portable-runtime"),
            "{body}"
        );
        assert!(
            body.lines()
                .any(|line| line.ends_with("Check for Updates on the Alpha channel.")),
            "{body}"
        );
    }

    #[test]
    fn marks_the_old_feed_as_pinned_to_the_bridge_and_the_new_one_is_only_the_feed() {
        let world = before(BRIDGE);
        world.publish().unwrap();
        let github = &world.github;
        assert_eq!(github.title("feed-alpha"), "Alpha update feed");
        assert!(github.is_prerelease("feed-alpha"));
        assert_eq!(github.title("update-alpha"), "Alpha update feed, pinned");
        let notes = github.notes("update-alpha");
        assert!(
            notes.starts_with(
                "Pinned to ConsensFlow 3.0.0-alpha.81, the first release to read feed-alpha: "
            ) && notes.contains("from it on read feed-alpha."),
            "{notes}"
        );
    }

    #[test]
    fn moves_the_old_feed_with_no_moment_at_which_it_lacks_its_latest_json_beyond_one_call() {
        let world = before(BRIDGE);
        world.publish().unwrap();
        let github = &world.github;
        assert_eq!(
            gap(github, "update-alpha", true),
            1,
            "between the two renames, and no longer"
        );
        let deletes: Vec<Vec<String>> = mentioning(&github.calls(), "delete-asset")
            .iter()
            .map(|args| args[..4].to_vec())
            .collect();
        assert_eq!(
            deletes,
            [["release", "delete-asset", "update-alpha", PREVIOUS]],
            "only the previous file is deleted, after the new one is in place"
        );
        assert!(!github
            .calls()
            .iter()
            .flatten()
            .any(|arg| arg == "--clobber"));
        assert!(github
            .names("update-alpha")
            .unwrap()
            .contains(&LATEST.to_string()));
    }

    #[test]
    fn is_refused_for_an_archive_the_old_apps_would_refuse_and_nothing_is_moved() {
        let only_app = format!("{ROOT}MacOS/app");
        let world = before_with(BRIDGE, None, &[only_app.as_str()]);
        let refused = world.publish().unwrap_err().to_string();
        assert!(
            refused.contains("the bridge must keep the layout") && refused.contains("MacOS/node"),
            "{refused}"
        );
        assert_eq!(
            world.github.calls(),
            Vec::<Vec<String>>::new(),
            "not so much as a look at GitHub"
        );
    }

    #[test]
    fn is_refused_for_a_missing_file_or_a_missing_set_of_notes_before_anything_is_asked() {
        for path in [
            format!("nsis/ConsensFlow_{BRIDGE}_x64-setup.exe"),
            "notes.txt".to_string(),
            "latest.json".to_string(),
        ] {
            let world = before(BRIDGE);
            let file = path
                .split('/')
                .fold(world.dir.path().to_path_buf(), |at, part| at.join(part));
            fs::remove_file(file).unwrap();
            assert_eq!(
                world.publish().unwrap_err().to_string(),
                format!("missing {path}")
            );
            assert_eq!(world.github.calls(), Vec::<Vec<String>>::new());
        }
    }
}

mod publishing_only_a_signed_sha256sums {
    use super::*;

    #[test]
    fn is_refused_without_the_signature_or_the_sums_it_signs_before_anything_is_asked() {
        for name in ["SHA256SUMS.sig", "SHA256SUMS"] {
            let world = before(LATER);
            fs::remove_file(world.dir.path().join(name)).unwrap();
            let refused = world.publish().unwrap_err().to_string();
            assert!(refused.starts_with(&format!("missing {name}")), "{refused}");
            assert_eq!(world.github.calls(), Vec::<Vec<String>>::new(), "{name}");
        }
    }

    #[test]
    fn is_refused_when_what_was_signed_is_not_what_the_files_built_say() {
        let world = before(LATER);
        let installer = world
            .dir
            .path()
            .join(format!("nsis/ConsensFlow_{LATER}_x64-setup.exe"));
        fs::write(
            installer,
            "an installer built again after the sums were signed",
        )
        .unwrap();
        assert_eq!(
            world.publish().unwrap_err().to_string(),
            "the SHA256SUMS that was signed does not list the files built here"
        );
        assert_eq!(world.github.calls(), Vec::<Vec<String>>::new());
    }

    #[test]
    fn is_refused_for_a_signature_the_installers_would_refuse_before_anything_is_asked() {
        let sums_of = |world: &cf_publish::testing::worlds::Before| {
            fs::read(world.dir.path().join("SHA256SUMS")).unwrap()
        };
        type Case = (&'static str, fn(&[u8]) -> String, &'static str);
        let cases: [Case; 4] = [
            (
                "a signature that is no signature",
                |_| "dW50cnVzdGVkIGNvbW1lbnQ6IHgK".to_string(),
                "the signature file is not a minisign signature",
            ),
            (
                "another release's",
                |sums| sign(sums, "ConsensFlow_9.9.9_SHA256SUMS"),
                "the signature is for",
            ),
            (
                "an update archive's",
                |sums| sign(sums, "ConsensFlow.app.tar.gz"),
                "the signature is for",
            ),
            (
                "of other bytes",
                |_| sign(b"other bytes", &format!("ConsensFlow_{LATER}_SHA256SUMS")),
                "it does not sign SHA256SUMS under the update key",
            ),
        ];
        for (name, signature, refused) in cases {
            let world = before(LATER);
            fs::write(
                world.dir.path().join("SHA256SUMS.sig"),
                signature(&sums_of(&world)),
            )
            .unwrap();
            let said = world.publish().unwrap_err().to_string();
            assert!(
                said.starts_with("SHA256SUMS.sig does not hold: ") && said.contains(refused),
                "{name}: {said}"
            );
            assert_eq!(world.github.calls(), Vec::<Vec<String>>::new(), "{name}");
        }
        // minisign's legacy kind, correctly signed by the key for this release: the installers
        // take only the prehashed kind, and so does the publisher (allow_legacy is false).
        let world = before(LATER);
        let legacy = sign_legacy(&sums_of(&world), &format!("ConsensFlow_{LATER}_SHA256SUMS"));
        fs::write(world.dir.path().join("SHA256SUMS.sig"), legacy).unwrap();
        let said = world.publish().unwrap_err().to_string();
        assert!(
            said.starts_with(
                "SHA256SUMS.sig does not hold: it does not sign SHA256SUMS under the update key"
            ),
            "{said}"
        );
        assert_eq!(world.github.calls(), Vec::<Vec<String>>::new());
        // CRLF line ends, which the installers refuse.
        let world = before(LATER);
        let sig = fs::read_to_string(world.dir.path().join("SHA256SUMS.sig")).unwrap();
        let text = String::from_utf8(decode(&sig))
            .unwrap()
            .replace('\n', "\r\n");
        fs::write(
            world.dir.path().join("SHA256SUMS.sig"),
            encode(text.as_bytes()),
        )
        .unwrap();
        let said = world.publish().unwrap_err().to_string();
        assert!(said.contains("not a minisign signature"), "{said}");
    }

    #[test]
    fn checks_the_signature_under_the_update_key_the_installers_pin() {
        let pinned = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../install/install.sh"
        ))
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("    key=").map(str::to_string))
        .unwrap();
        assert_eq!(cf_publish::sums::update_key().unwrap(), pinned);
    }

    /// Base64, both ways, for the tests that change a signature file.
    fn decode(text: &str) -> Vec<u8> {
        let letters = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut bits = Vec::new();
        for byte in text.trim().bytes().filter(|b| *b != b'=') {
            let value = letters.iter().position(|l| *l == byte).unwrap();
            bits.extend((0..6).rev().map(|at| (value >> at) & 1));
        }
        bits.as_chunks::<8>()
            .0
            .iter()
            .map(|byte| byte.iter().fold(0u8, |n, bit| n << 1 | *bit as u8))
            .collect()
    }

    fn encode(bytes: &[u8]) -> String {
        let letters = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bits: Vec<u8> = bytes
            .iter()
            .flat_map(|b| (0..8).rev().map(move |at| (b >> at) & 1))
            .collect();
        let mut out: String = bits
            .chunks(6)
            .map(|six| {
                let n = (0..6).fold(0usize, |n, at| {
                    n << 1 | usize::from(*six.get(at).unwrap_or(&0))
                });
                letters[n] as char
            })
            .collect();
        while !out.len().is_multiple_of(4) {
            out.push('=');
        }
        out
    }

    #[test]
    fn publishes_the_sums_that_were_signed_and_their_signature_as_they_are() {
        let world = before(LATER);
        world.publish().unwrap();
        let tag = format!("v{LATER}");
        for name in ["SHA256SUMS", "SHA256SUMS.sig"] {
            assert_eq!(
                world.github.asset_text(&tag, name),
                fs::read_to_string(world.dir.path().join(name)).unwrap(),
                "{name}"
            );
        }
    }
}

mod publishing_a_later_release {
    use super::*;

    #[test]
    fn moves_the_new_feed_only_and_does_not_so_much_as_touch_the_old_one() {
        let world = before(LATER);
        let pinned = world.github.asset_text("update-alpha", LATEST);
        let done = world.publish().unwrap();
        assert_eq!(did(&done), ["feed-alpha created"]);
        assert_eq!(
            mentioning(&world.github.calls(), "update-alpha"),
            Vec::<Vec<String>>::new()
        );
        assert_eq!(
            world.github.asset_text("update-alpha", LATEST),
            pinned,
            "pinned to the bridge"
        );
        assert_eq!(world.check(), Vec::<String>::new());
    }

    #[test]
    fn moves_both_new_feeds_for_a_stable_release_and_the_old_feeds_neither() {
        let world = before("3.0.1");
        let done = world.publish().unwrap();
        assert_eq!(did(&done), ["feed-alpha created", "feed-stable created"]);
        let github = &world.github;
        assert!(
            !github.is_prerelease("v3.0.1"),
            "a stable release is not a prerelease"
        );
        assert!(
            github
                .notes("v3.0.1")
                .lines()
                .any(|line| line.ends_with("on the Stable channel.")),
            "{}",
            github.notes("v3.0.1")
        );
        let calls = github.calls();
        assert_eq!(
            mentioning(&calls, "update-alpha"),
            Vec::<Vec<String>>::new()
        );
        assert_eq!(
            mentioning(&calls, "update-stable"),
            Vec::<Vec<String>>::new()
        );
    }

    #[test]
    fn is_refused_while_the_old_feed_still_serves_the_release_before_the_bridge_and_nothing_is_moved(
    ) {
        // The bridge's tag holds the manifest; its release failed before it moved update-alpha.
        let world = before_with(LATER, Some(OLDER), &cf_publish::testing::OLD_LAYOUT);
        let refused = world.publish().unwrap_err().to_string();
        assert!(
            refused.starts_with("3.0.0-alpha.82 may not move a feed, and nothing was moved:\n- update-alpha serves 3.0.0-alpha.80, not the bridge 3.0.0-alpha.81"),
            "{refused}"
        );
        let github = &world.github;
        assert_eq!(
            github.calls(),
            Vec::<Vec<String>>::new(),
            "no release, no feed, not so much as a look"
        );
        assert!(!github.has(&format!("v{LATER}")));
        assert!(!github.has("feed-alpha"));
    }

    #[test]
    fn is_refused_for_every_way_the_old_feed_or_the_bridge_can_fail_to_be_there() {
        let changes: [(String, Served); 5] = [
            ("/update-alpha/latest.json".to_string(), Served::Status(404)),
            ("/update-alpha/latest.json".to_string(), Served::Status(503)),
            (
                format!("/v{BRIDGE}/ConsensFlow_{BRIDGE}_aarch64.app.tar.gz"),
                "junk".into(),
            ),
            (
                format!("/v{BRIDGE}/ConsensFlow_{BRIDGE}_x64-portable.exe"),
                Served::Status(404),
            ),
            (format!("/v{BRIDGE}/SHA256SUMS"), Served::Status(404)),
        ];
        for (path, served) in changes {
            let world = before(LATER);
            world.github.serve(&path, served);
            let refused = world.publish().unwrap_err().to_string();
            assert!(refused.contains("nothing was moved"), "{path}: {refused}");
            assert_eq!(world.github.calls(), Vec::<Vec<String>>::new(), "{path}");
        }
    }

    #[test]
    fn is_refused_for_a_release_before_the_bridge() {
        let world = before(OLDER);
        let refused = world.publish().unwrap_err().to_string();
        assert!(
            refused.contains("3.0.0-alpha.80 comes before the bridge"),
            "{refused}"
        );
        assert_eq!(world.github.calls(), Vec::<Vec<String>>::new());
    }
}

mod the_bridge_that_failed_before_it_moved_update_alpha_and_the_release_after_it {
    use super::*;

    #[test]
    fn is_finished_by_running_it_again_the_release_after_it_waits_until_then() {
        let world = releasing();
        let github = &world.github;
        let (bridge, later) = (world.latest(BRIDGE), world.latest(LATER));
        let tag = format!("v{BRIDGE}");

        // alpha.81, the bridge: the release is made and the new feed moved, and the run dies at update-alpha.
        github.fail(|args, _| {
            args_include(args, "update-alpha").then(|| "the network went away".to_string())
        });
        let cut = world.publish(BRIDGE).unwrap_err().to_string();
        assert!(cut.contains("the network went away"), "{cut}");
        assert!(!github.is_draft(&tag), "its release is public");
        assert_eq!(github.asset_text("feed-alpha", LATEST), bridge);
        assert_eq!(
            github.asset_text("update-alpha", LATEST),
            latest_json(github.base(), OLDER, "notes")
        );

        // alpha.82 is made from a tree that holds the tag of alpha.81 and its manifest: it is refused.
        github.clear_fail();
        let calls = github.calls().len();
        let refused = world.publish(LATER).unwrap_err().to_string();
        assert!(
            refused.contains("update-alpha serves 3.0.0-alpha.80, not the bridge"),
            "{refused}"
        );
        assert_eq!(
            github.calls().len(),
            calls,
            "nothing was asked of gh for it"
        );
        assert!(!github.has(&format!("v{LATER}")));

        // Running alpha.81 again finishes it: no "release exists" stop, no second release.
        let finished = world.publish(BRIDGE).unwrap();
        assert_eq!(did(&finished), ["feed-alpha kept", "update-alpha replaced"]);
        assert_eq!(finished.release.to_string(), "kept");
        let creates = mentioning(&github.calls(), &tag)
            .iter()
            .filter(|args| args[1] == "create")
            .count();
        assert_eq!(creates, 1);
        assert_eq!(github.asset_text("update-alpha", LATEST), bridge);

        // Now alpha.82 may move its feed.
        let next = world.publish(LATER).unwrap();
        assert_eq!(next.release.to_string(), "created");
        assert_eq!(github.asset_text("feed-alpha", LATEST), later);
        assert_eq!(
            github.asset_text("update-alpha", LATEST),
            bridge,
            "still the bridge"
        );
    }
}
