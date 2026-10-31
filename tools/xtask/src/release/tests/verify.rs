//! `release verify`, run on a checkout with real remotes and a GitHub that is a
//! script that hosts a release: the files `gh release download` gives, the feeds,
//! and the archive a feed's address gives. Nothing here reaches GitHub.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use super::fake::{
    archive_name, archive_url, good_release, latest, sha256, sums_of, write, Fake, World,
    ARCHIVE_SIG, OTHER_KEY,
};
use super::{refusal, words};
use crate::release::{verify, Error, Pace, Release};

const VERSION: &str = "3.0.6-alpha.1";
const TAG: &str = "v3.0.6-alpha.1";
/// The id of the key the stand-in checkout pins, as its public key says it.
const KEY_ID: &str = "384B143B9E2BED9E";

/// A checkout at the alpha, tagged and pushed, and a GitHub that holds its release
/// as it should be and serves it on feed-alpha.
fn published() -> (World, Fake) {
    let world = World::new(VERSION);
    world.released(VERSION);
    let mut fake = Fake::new(&world, VERSION, "3.0.5");
    fake.releases.insert(TAG.to_owned(), good_release(VERSION));
    (world, fake)
}

fn verified(world: &World, fake: &mut Fake, version: &str) -> (Result<String, Error>, String) {
    let mut said = Vec::new();
    let mut release = Release {
        root: world.root.clone(),
        system: fake,
        said: &mut said,
        pace: Pace {
            appear: Duration::ZERO,
            appear_attempts: 5,
            poll: Duration::ZERO,
            feed: Duration::ZERO,
            feed_attempts: 5,
        },
    };
    let outcome = verify::release(&mut release, version);
    (outcome, String::from_utf8(said).unwrap())
}

/// The trusted comment of the stand-in archive's signature.
fn trusted_comment() -> String {
    let text = String::from_utf8(STANDARD.decode(ARCHIVE_SIG.trim()).unwrap()).unwrap();
    text.lines()
        .find_map(|line| line.strip_prefix("trusted comment: "))
        .unwrap()
        .to_owned()
}

/// What a release that is as it should be says of itself, its feeds being `feeds`.
fn good_report(version: &str, feeds: &[&str]) -> String {
    let listed = good_release(version).len() - 1;
    let archive = archive_name(version);
    let mut text = format!(
        "release verify: v{version} on ngvoicu/consensflow\n  \
         SHA256SUMS lists {listed} files, each in the release and hashing as listed\n  \
         {archive} is signed by the key v{version}'s tauri.conf.json pins (minisign public key: {KEY_ID}): \
         the file and its trusted comment hold, \"{}\"\n  \
         a copy of {archive} with one byte changed is refused\n",
        trusted_comment()
    );
    for feed in feeds {
        text += &format!(
            "  {feed} serves {version}: its latest.json is the release's, its signature the .sig, \
             and its archive, read from its address, the release's\n"
        );
    }
    text
}

/// What a refusal says of a release with `problems`.
fn does_not_verify(problems: &[String]) -> String {
    let plural = if problems.len() == 1 { "" } else { "s" };
    let lines: String = problems
        .iter()
        .map(|problem| format!("\n- {problem}"))
        .collect();
    format!(
        "{TAG} does not verify, {} problem{plural}:{lines}",
        problems.len()
    )
}

fn files_of(fake: &mut Fake) -> &mut super::fake::Files {
    fake.releases.get_mut(TAG).unwrap()
}

#[test]
fn a_release_that_is_as_it_should_be_passes_and_says_what_it_checked() {
    let (world, mut fake) = published();
    let (outcome, _) = verified(&world, &mut fake, VERSION);
    assert_eq!(outcome.unwrap(), good_report(VERSION, &["feed-alpha"]));
}

#[test]
fn a_plain_version_is_served_by_both_feeds_and_both_are_checked() {
    let world = World::new("3.0.6");
    world.released("3.0.6");
    let mut fake = Fake::new(&world, "3.0.6", "3.0.6");
    fake.releases
        .insert("v3.0.6".to_owned(), good_release("3.0.6"));
    let (outcome, _) = verified(&world, &mut fake, "3.0.6");
    assert_eq!(
        outcome.unwrap(),
        good_report("3.0.6", &["feed-alpha", "feed-stable"])
    );
}

#[test]
fn a_file_with_a_byte_changed_since_sha256sums_was_written_fails_naming_both_hashes() {
    let (world, mut fake) = published();
    let dmg = format!("ConsensFlow_{VERSION}_aarch64.dmg");
    let listed = sha256(&files_of(&mut fake)[&dmg]);
    files_of(&mut fake).get_mut(&dmg).unwrap()[0] ^= 1;
    let got = sha256(&files_of(&mut fake)[&dmg]);
    assert_ne!(got, listed);
    let said = refusal(verified(&world, &mut fake, VERSION).0);
    assert_eq!(
        said,
        does_not_verify(&[format!(
            "{dmg} hashes to {got}, and SHA256SUMS lists {listed}"
        )])
    );
}

#[test]
fn an_archive_with_a_byte_changed_fails_its_hash_and_its_signature() {
    let (world, mut fake) = published();
    let archive = archive_name(VERSION);
    let listed = sha256(&files_of(&mut fake)[&archive]);
    files_of(&mut fake).get_mut(&archive).unwrap()[0] ^= 1;
    let got = sha256(&files_of(&mut fake)[&archive]);
    let said = refusal(verified(&world, &mut fake, VERSION).0);
    assert_eq!(
        said,
        does_not_verify(&[
            format!("{archive} hashes to {got}, and SHA256SUMS lists {listed}"),
            format!(
                "{archive} does not verify against the key {TAG}'s tauri.conf.json pins: \
                 The signature verification failed"
            ),
        ])
    );
}

#[test]
fn an_archive_signed_by_another_key_than_the_one_the_tag_pins_fails() {
    let world = World::new(VERSION);
    world.pin(VERSION, OTHER_KEY.trim());
    world.released(VERSION);
    let mut fake = Fake::new(&world, VERSION, "3.0.5");
    fake.releases.insert(TAG.to_owned(), good_release(VERSION));
    let archive = archive_name(VERSION);
    let said = refusal(verified(&world, &mut fake, VERSION).0);
    assert_eq!(
        said,
        does_not_verify(&[format!(
            "{archive} does not verify against the key {TAG}'s tauri.conf.json pins: \
             The signature was created with a different key than the one provided"
        )])
    );
}

#[test]
fn the_key_is_the_one_the_tag_pins_not_the_one_the_checkout_pins_now() {
    let (world, mut fake) = published();
    // main moved on and pins another key; the release is of the tag.
    world.pin(VERSION, OTHER_KEY.trim());
    let (outcome, _) = verified(&world, &mut fake, VERSION);
    assert_eq!(outcome.unwrap(), good_report(VERSION, &["feed-alpha"]));
}

#[test]
fn a_signature_whose_trusted_comment_was_changed_fails() {
    let (world, mut fake) = published();
    let archive = archive_name(VERSION);
    let text = String::from_utf8(STANDARD.decode(ARCHIVE_SIG.trim()).unwrap()).unwrap();
    let forged = text.replace("file:archive.bin", "file:another.bin");
    assert_ne!(forged, text);
    let sig = STANDARD.encode(forged);
    let files = files_of(&mut fake);
    files.insert(format!("{archive}.sig"), sig.clone().into_bytes());
    // Everything else is consistent with the new .sig: the sums, and the feed's entry.
    let entry = latest(VERSION).replace(ARCHIVE_SIG.trim(), &sig);
    files.insert("latest.json".to_owned(), entry.clone().into_bytes());
    let sums = sums_of(files);
    files.insert("SHA256SUMS".to_owned(), sums.into_bytes());
    fake.feeds.insert("feed-alpha".to_owned(), entry);
    let said = refusal(verified(&world, &mut fake, VERSION).0);
    assert_eq!(
        said,
        does_not_verify(&[format!(
            "{archive} does not verify against the key {TAG}'s tauri.conf.json pins: \
             The signature verification failed"
        )])
    );
}

#[test]
fn a_file_sha256sums_lists_that_the_release_lacks_fails() {
    let (world, mut fake) = published();
    let dmg = format!("ConsensFlow_{VERSION}_aarch64.dmg");
    files_of(&mut fake).remove(&dmg);
    let said = refusal(verified(&world, &mut fake, VERSION).0);
    assert_eq!(
        said,
        does_not_verify(&[format!(
            "{dmg} is listed in SHA256SUMS and is not in the release"
        )])
    );
}

#[test]
fn a_file_the_release_holds_that_sha256sums_does_not_list_fails() {
    let (world, mut fake) = published();
    files_of(&mut fake).insert("extra.bin".to_owned(), b"nothing lists this".to_vec());
    let said = refusal(verified(&world, &mut fake, VERSION).0);
    assert_eq!(
        said,
        does_not_verify(&[
            "extra.bin is in the release and SHA256SUMS does not list it".to_owned()
        ])
    );
}

#[test]
fn the_lines_of_sha256sums_are_read_as_sha256sum_writes_them_and_any_other_is_told() {
    let hash = "a".repeat(64);
    let upper = "B".repeat(64);
    let text = format!(
        "{hash}  plain.bin\n{upper} *binary.bin\n\nnot a hash  nor.bin\n{hash}\n{hash}  \n"
    );
    let (listed, odd) = verify::listed_in(&text);
    assert_eq!(
        listed.into_iter().collect::<Vec<_>>(),
        [
            ("binary.bin".to_owned(), "b".repeat(64)),
            ("plain.bin".to_owned(), hash)
        ]
    );
    assert_eq!(
        odd,
        [
            "not a hash  nor.bin",
            &"a".repeat(64),
            &format!("{}  ", "a".repeat(64))
        ]
    );
    // And a release whose SHA256SUMS holds such a line is told so.
    let (world, mut fake) = published();
    let files = files_of(&mut fake);
    let mut sums = String::from_utf8(files["SHA256SUMS"].clone()).unwrap();
    sums += "garbage\n";
    files.insert("SHA256SUMS".to_owned(), sums.into_bytes());
    assert_eq!(
        refusal(verified(&world, &mut fake, VERSION).0),
        does_not_verify(&[
            "SHA256SUMS holds a line that is not \"<sha-256>  <name>\": \"garbage\"".to_owned()
        ])
    );
}

#[test]
fn a_release_without_its_sha256sums_cannot_be_checked_at_all() {
    let (world, mut fake) = published();
    files_of(&mut fake).remove("SHA256SUMS");
    assert_eq!(
        refusal(verified(&world, &mut fake, VERSION).0),
        format!("{TAG}'s release has no SHA256SUMS, so no file of it can be checked")
    );
}

#[test]
fn an_update_archive_or_signature_the_release_lacks_fails_for_the_update_needs_both() {
    for (lacks, other) in [
        (
            format!("{}.sig", archive_name(VERSION)),
            archive_name(VERSION),
        ),
        (
            archive_name(VERSION),
            format!("{}.sig", archive_name(VERSION)),
        ),
    ] {
        let (world, mut fake) = published();
        let files = files_of(&mut fake);
        files.remove(&lacks);
        let sums = sums_of(files);
        files.insert("SHA256SUMS".to_owned(), sums.into_bytes());
        // The feed's entry still names it; the sums are consistent with what is left.
        let said = refusal(verified(&world, &mut fake, VERSION).0);
        assert!(
            said.contains(&format!(
                "- {lacks} is not in the release, and the update needs it"
            )),
            "{said}"
        );
        assert!(
            !said.contains(&format!("{other} is not in the release")),
            "{said}"
        );
    }
}

#[test]
fn a_feed_that_serves_a_later_release_is_told_and_not_checked() {
    let (world, mut fake) = published();
    fake.feeds
        .insert("feed-alpha".to_owned(), latest("3.0.6-alpha.2"));
    let (outcome, _) = verified(&world, &mut fake, VERSION);
    let done = outcome.unwrap();
    assert!(
        done.contains(&format!(
            "  feed-alpha serves 3.0.6-alpha.2, a later release than {VERSION}: not checked\n"
        )),
        "{done}"
    );
}

#[test]
fn a_feed_that_serves_an_earlier_release_or_none_fails() {
    for (feed, why) in [
        (
            Some(latest("3.0.5")),
            "feed-alpha serves 3.0.5, not 3.0.6-alpha.1".to_owned(),
        ),
        (
            None,
            "feed-alpha could not be read: it serves no release".to_owned(),
        ),
    ] {
        let (world, mut fake) = published();
        match feed {
            Some(body) => fake.feeds.insert("feed-alpha".to_owned(), body),
            None => fake.feeds.remove("feed-alpha"),
        };
        assert_eq!(
            refusal(verified(&world, &mut fake, VERSION).0),
            does_not_verify(&[why])
        );
    }
}

#[test]
fn a_feed_whose_latest_json_is_not_the_releases_fails() {
    let (world, mut fake) = published();
    let changed = latest(VERSION).replace("\"notes\":\"notes\"", "\"notes\":\"other notes\"");
    assert_ne!(changed, latest(VERSION));
    fake.feeds.insert("feed-alpha".to_owned(), changed);
    assert_eq!(
        refusal(verified(&world, &mut fake, VERSION).0),
        does_not_verify(&["feed-alpha's latest.json is not the release's".to_owned()])
    );
}

#[test]
fn a_feed_whose_signature_is_not_the_sig_fails() {
    let (world, mut fake) = published();
    // The release's own latest.json and the feed's agree, and carry another signature.
    let other = latest(VERSION).replace(ARCHIVE_SIG.trim(), "RWQ-not-the-signature");
    let files = files_of(&mut fake);
    files.insert("latest.json".to_owned(), other.clone().into_bytes());
    let sums = sums_of(files);
    files.insert("SHA256SUMS".to_owned(), sums.into_bytes());
    fake.feeds.insert("feed-alpha".to_owned(), other);
    assert_eq!(
        refusal(verified(&world, &mut fake, VERSION).0),
        does_not_verify(&["feed-alpha's signature for darwin-aarch64 is not the .sig".to_owned()])
    );
}

#[test]
fn a_feed_whose_address_gives_another_archive_than_the_releases_fails() {
    let (world, mut fake) = published();
    let url = archive_url(VERSION);
    fake.downloads
        .insert(url.clone(), b"another archive".to_vec());
    assert_eq!(
        refusal(verified(&world, &mut fake, VERSION).0),
        does_not_verify(&[format!(
            "feed-alpha's archive, read from {url}, is not the release's"
        )])
    );
}

#[test]
fn a_feed_whose_address_gives_nothing_fails_with_what_curl_said() {
    let (world, mut fake) = published();
    let url = archive_url(VERSION);
    // The release holds the archive, but the feed names an address that has none.
    let moved = latest(VERSION).replace(&url, "https://example.invalid/none.tar.gz");
    let files = files_of(&mut fake);
    files.insert("latest.json".to_owned(), moved.clone().into_bytes());
    let sums = sums_of(files);
    files.insert("SHA256SUMS".to_owned(), sums.into_bytes());
    fake.feeds.insert("feed-alpha".to_owned(), moved);
    assert_eq!(
        refusal(verified(&world, &mut fake, VERSION).0),
        does_not_verify(&[
            "feed-alpha's archive could not be read from https://example.invalid/none.tar.gz: \
             curl ended 22: curl: (22) The requested URL returned error: 404"
                .to_owned()
        ])
    );
}

#[test]
fn a_tag_the_checkout_lacks_is_fetched_by_name_from_upstream() {
    let (world, mut fake) = published();
    super::fake::git(&world.root, &["tag", "-d", TAG]);
    let (outcome, _) = verified(&world, &mut fake, VERSION);
    assert_eq!(outcome.unwrap(), good_report(VERSION, &["feed-alpha"]));
}

#[test]
fn a_tag_no_remote_has_is_refused_with_where_it_looked() {
    let world = World::new(VERSION);
    let mut fake = Fake::new(&world, VERSION, "3.0.5");
    fake.releases.insert(TAG.to_owned(), good_release(VERSION));
    let said = refusal(verified(&world, &mut fake, VERSION).0);
    assert!(
        said.starts_with(&format!(
            "{TAG} is not in the checkout, and upstream does not have it either"
        )),
        "{said}"
    );
}

#[test]
fn a_version_github_has_no_release_for_is_refused_with_what_gh_said() {
    let world = World::new(VERSION);
    world.released(VERSION);
    let mut fake = Fake::new(&world, VERSION, "3.0.5");
    let said = refusal(verified(&world, &mut fake, VERSION).0);
    assert_eq!(
        said,
        format!("{TAG} has no release to download: `gh release download {TAG} --repo ngvoicu/consensflow --dir <the folder>` ended with status 1: release not found")
    );
}

#[test]
fn it_only_reads_whatever_the_checkout_holds_and_leaves_nothing_behind() {
    let (world, mut fake) = published();
    // The checkout is on another branch with a change not committed: no matter.
    super::fake::git(&world.root, &["checkout", "-q", "-b", "wt/elsewhere"]);
    write(&world.root.join("README.md"), "an edit not committed\n");
    let before = super::fake::git(&world.root, &["status", "--porcelain"]);
    assert!(verified(&world, &mut fake, VERSION).0.is_ok());
    // A failing run too.
    files_of(&mut fake).get_mut(&archive_name(VERSION)).unwrap()[0] ^= 1;
    assert!(verified(&world, &mut fake, VERSION).0.is_err());
    assert_eq!(
        super::fake::git(&world.root, &["status", "--porcelain"]),
        before
    );
    assert!(!world.root.join("app/src-tauri/target/verify").exists());
    let ran = fake.lines();
    assert!(
        ran.iter().all(|line| !line.starts_with("git push")
            && !line.starts_with("git tag")
            && !line.starts_with("gh workflow")),
        "{ran:#?}"
    );
    for remote in [&world.origin, &world.upstream] {
        assert!(World::tagged(remote, TAG).is_some());
    }
}

#[test]
fn the_command_takes_the_version_alone() {
    assert_eq!(
        verify::version_of(&words("3.0.6-alpha.3")).unwrap(),
        "3.0.6-alpha.3"
    );
    assert_eq!(verify::version_of(&words("3.0.6")).unwrap(), "3.0.6");
    for (line, said) in [
        (
            "",
            "release verify takes the version to verify, X.Y.Z or X.Y.Z-alpha.N",
        ),
        ("3.0.6 3.0.7", "release verify does not take 3.0.7"),
        (
            "3.0.6 --notes n.txt",
            "release verify does not take --notes",
        ),
        ("--dry-run", "release verify does not take --dry-run"),
    ] {
        let Err(crate::dispatch::Failure::Usage(message)) = verify::version_of(&words(line)) else {
            panic!("{line}: taken");
        };
        assert_eq!(message, said, "{line}");
    }
    // What is no version is told as the release tool tells it.
    let Err(crate::dispatch::Failure::Usage(message)) = verify::version_of(&words("v3.0.6")) else {
        panic!("taken");
    };
    assert_eq!(
        message,
        "the version is not a canonical semantic version: v3.0.6"
    );
}
