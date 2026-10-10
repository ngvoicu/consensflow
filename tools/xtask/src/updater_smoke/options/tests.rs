use super::*;

fn read(line: &str) -> Result<Options, String> {
    let args: Vec<OsString> = line.split_whitespace().map(OsString::from).collect();
    Options::read(&args)
}

#[test]
fn takes_no_option_at_all_and_then_runs_the_flip_and_the_bridge_in_that_order() {
    assert_eq!(read("").unwrap(), Options::default());
    assert_eq!(
        read("").unwrap().releases().unwrap(),
        [Release::Flip, Release::Bridge]
    );
}

#[test]
fn reads_each_option_as_the_script_it_replaces_did() {
    let options = read(
        "--from bridge,flip --from-app a.app --from-release flip --to-app b.app --bridge bridge-dir \
         --flip flip-dir --flip-ref v3.0.0-alpha.82 --cache some/cache --build-only --reuse \
         --export-bridge --only refused,update --machines /m --timeout 60000 --keep",
    )
    .unwrap();
    assert_eq!(
        options,
        Options {
            from: Some("bridge,flip".into()),
            from_app: Some("a.app".into()),
            from_release: Some("flip".into()),
            to_app: Some("b.app".into()),
            bridge: Some("bridge-dir".into()),
            flip: Some("flip-dir".into()),
            flip_ref: Some("v3.0.0-alpha.82".into()),
            cache: Some("some/cache".into()),
            build_only: true,
            reuse: true,
            export_bridge: true,
            only: vec!["refused".into(), "update".into()],
            machines: Some("/m".into()),
            timeout: Some(Duration::from_millis(60_000)),
            keep: true,
        }
    );
}

#[test]
fn takes_a_value_after_an_equals_sign_as_well_and_keeps_what_follows_it_whole() {
    let options = read("--only=a,,b --cache=x=y --flip-ref=--odd").unwrap();
    assert_eq!(options.only, ["a", "b"]);
    assert_eq!(options.cache, Some(PathBuf::from("x=y")));
    assert_eq!(options.flip_ref.as_deref(), Some("--odd"));
}

#[test]
fn refuses_a_value_that_is_missing_or_looks_like_an_option() {
    for line in ["--only", "--only --keep", "--from-app", "--timeout -5"] {
        let said = read(line).unwrap_err();
        assert!(said.contains("takes a value"), "{line}: {said}");
    }
    assert!(read("--cache a --only")
        .unwrap_err()
        .starts_with("--only takes a value"));
}

#[test]
fn refuses_what_is_no_option_of_the_smoke_and_a_flag_given_a_value() {
    assert_eq!(read("--nope").unwrap_err(), "unknown option: --nope");
    assert_eq!(read("--nope=1").unwrap_err(), "unknown option: --nope");
    assert_eq!(read("-k").unwrap_err(), "unknown option: -k");
    assert_eq!(read("a.app").unwrap_err(), "unexpected argument: a.app");
    assert_eq!(read("--keep=yes").unwrap_err(), "--keep takes no value");
    // An option is the whole of its name.
    assert_eq!(read("--keeps").unwrap_err(), "unknown option: --keeps");
    assert_eq!(
        read("--from-appx a").unwrap_err(),
        "unknown option: --from-appx"
    );
}

#[test]
fn takes_a_timeout_that_is_a_number_of_milliseconds() {
    assert_eq!(
        read("--timeout 250").unwrap().timeout,
        Some(Duration::from_millis(250))
    );
    assert_eq!(
        read("--timeout fast").unwrap_err(),
        "--timeout is a number of milliseconds, not fast"
    );
    assert_eq!(
        read("--timeout=1.5").unwrap_err(),
        "--timeout is a number of milliseconds, not 1.5"
    );
}

#[test]
fn names_the_releases_asked_for_in_the_order_given() {
    let releases = |line: &str| read(line).unwrap().releases();
    assert_eq!(releases("--from flip").unwrap(), [Release::Flip]);
    assert_eq!(
        releases("--from bridge,flip").unwrap(),
        [Release::Bridge, Release::Flip]
    );
    assert_eq!(releases("--from flip,").unwrap(), [Release::Flip]);
    // A built app is one release, which is the flip's unless it is said.
    assert_eq!(releases("--from-app a.app").unwrap(), [Release::Flip]);
    assert_eq!(
        releases("--from-app a.app --from-release bridge").unwrap(),
        [Release::Bridge]
    );
}

#[test]
fn refuses_releases_that_are_none_and_options_that_do_not_go_together() {
    let releases = |line: &str| read(line).unwrap().releases().unwrap_err();
    assert_eq!(
        releases("--from flip,nothing"),
        "nothing is no release to install from: bridge or flip"
    );
    assert_eq!(
        releases("--from-app a.app --from-release nothing"),
        "nothing is no release to install from: bridge or flip"
    );
    assert_eq!(
        releases("--from-app a.app --from flip"),
        "--from-app takes one release: name it with --from-release"
    );
    assert_eq!(
        releases("--from-release flip"),
        "--from-release says which release --from-app is"
    );
}
