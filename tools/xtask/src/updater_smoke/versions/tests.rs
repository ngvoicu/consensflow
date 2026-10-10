use super::*;

#[test]
fn reads_semvers_order_pre_releases_by_number_a_release_after_its_pre_releases() {
    let order = [
        "3.0.0-alpha.9",
        "3.0.0-alpha.10",
        "3.0.0-alpha.81",
        "3.0.0-beta.1",
        "3.0.0",
        "3.0.1",
    ];
    for pair in order.windows(2) {
        let (before, after) = (pair[0], pair[1]);
        assert_eq!(
            compare_versions(before, after).unwrap(),
            Ordering::Less,
            "{before} < {after}"
        );
        assert_eq!(compare_versions(after, before).unwrap(), Ordering::Greater);
    }
    assert_eq!(
        compare_versions("3.0.0-alpha.81", "3.0.0-alpha.81").unwrap(),
        Ordering::Equal
    );
    assert_eq!(
        parse_version("3.0.0-alpha.81").unwrap(),
        Version {
            core: [3, 0, 0],
            pre: vec![Part::Word("alpha".into()), Part::Number(81)],
        }
    );
    assert!(parse_version("3.0")
        .unwrap_err()
        .to_string()
        .contains("not a semantic version"));
}

#[test]
fn is_no_version_that_semver_does_not_name() {
    for text in [
        "",
        "3",
        "3.0",
        "3.0.0.1",
        "v3.0.0",
        "3.0.0-",
        "3.0.0+build",
        "3.0.x",
        "a.b.c",
        "-1.0.0",
        "3.0.0-al pha",
    ] {
        assert!(parse_version(text).is_err(), "{text:?}");
    }
    // A pre-release's parts are numbers or words, and an empty one is none.
    assert_eq!(
        parse_version("1.2.3-a..2").unwrap().pre,
        [Part::Word("a".into()), Part::Number(2)]
    );
}

#[test]
fn is_the_next_one_after_the_newest_installed_apps_unless_a_release_has_moved_past_it() {
    assert_eq!(next_version("3.0.0-alpha.81").unwrap(), "3.0.0-alpha.82");
    assert_eq!(next_version("3.0.0").unwrap(), "3.0.1");
    // Only a number is counted up: a pre-release of words has none, so its patch is.
    assert_eq!(next_version("3.0.0-beta").unwrap(), "3.0.1");
    let update = |checkout, installed: &[&str]| update_version(checkout, installed).unwrap();
    assert_eq!(
        update("3.0.0-alpha.81", &["3.0.0-alpha.81"]),
        "3.0.0-alpha.82"
    );
    assert_eq!(
        update("3.0.0-alpha.80", &["3.0.0-alpha.81"]),
        "3.0.0-alpha.82"
    );
    assert_eq!(
        update("3.0.0-alpha.90", &["3.0.0-alpha.81"]),
        "3.0.0-alpha.90"
    );
    // Two apps are installed from (the bridge and the flip), and the update is newer than both.
    assert_eq!(
        update("3.0.0-alpha.82", &["3.0.0-alpha.81", "3.0.0-alpha.82"]),
        "3.0.0-alpha.83"
    );
    assert_eq!(
        update("3.0.0-alpha.83", &["3.0.0-alpha.82", "3.0.0-alpha.81"]),
        "3.0.0-alpha.83"
    );
    assert_eq!(
        newest(&["3.0.0-alpha.9", "3.0.0-alpha.10", "3.0.0-alpha.2"]).unwrap(),
        "3.0.0-alpha.10"
    );
    assert!(newest(&[]).is_err(), "there is no newest of none");
}

#[test]
fn has_the_flip_release_the_newest_tag_after_the_bridges_and_none_where_there_is_none() {
    let bridge = "v3.0.0-alpha.81";
    assert_eq!(
        flip_tag(
            &[
                "v3.0.0-alpha.80",
                bridge,
                "v3.0.0-alpha.82",
                "v3.0.0-alpha.9"
            ],
            bridge
        )
        .unwrap(),
        "v3.0.0-alpha.82"
    );
    // Release numbers are numbers, and what is no release tag is none.
    assert_eq!(
        flip_tag(
            &[
                bridge,
                "v3.0.0-alpha.100",
                "v3.0.0-alpha.99",
                "not-a-tag",
                "v3.0"
            ],
            bridge
        )
        .unwrap(),
        "v3.0.0-alpha.100"
    );
    assert!(flip_tag(&[], bridge)
        .unwrap_err()
        .to_string()
        .contains("no release newer than the bridge"));
    assert!(flip_tag(&["v3.0.0-alpha.80", bridge], bridge)
        .unwrap_err()
        .to_string()
        .contains("name the flip with --flip-ref"));
}
