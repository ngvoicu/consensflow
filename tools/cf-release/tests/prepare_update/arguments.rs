//! The words the command takes, and the order it asks things in.

use std::fs;

use super::fixture::{run, words, Release};

const REQUIRED: [&str; 7] = [
    "bundle",
    "archive",
    "signature",
    "notes",
    "output",
    "channel",
    "date",
];

#[test]
fn rejects_invalid_channels_and_dates() {
    let release = Release::new();
    for channel in ["beta", "Alpha", "ALPHA", "", " alpha", "alpha\n", "release"] {
        release
            .run(&[("channel", channel)])
            .refused("channel must be alpha or stable\n");
    }
    for date in [
        "next friday",
        "2026-13-99T99:99:99Z",
        "2026-09-09",
        "2026-09-09T12:00:00",
        "2026-02-30T12:00:00Z",
        "2026-09-09 12:00:00Z",
        "",
    ] {
        release
            .run(&[("date", date)])
            .refused("publication date must be RFC3339 with an explicit timezone\n");
    }
    for date in [
        "2026-09-09T12:00:00Z",
        "2026-09-09T12:00:00+02:00",
        "2026-09-09T12:00:00.5-11:30",
    ] {
        release.run(&[("date", date)]).finished();
        assert_eq!(release.entry()["pub_date"], date);
    }
}

#[test]
fn rejects_flags_the_command_does_not_take() {
    let release = Release::new();
    release
        .run(&[("url", "https://evil.example/x.tar.gz")])
        .misused("unknown argument: --url\nsee `cf-release prepare-update --help`\n");
    assert!(!release.output.exists());

    // As the script took them: `--name value`, not `--name=value`, and no word without its flag.
    let mut flags = release.flags();
    flags.push(("channel=stable".into(), String::new()));
    run(&words(&flags)).misused("unknown argument: --channel=stable");
    let mut loose = words(&release.flags());
    loose.push("stray".into());
    run(&loose).misused("unknown argument: stray");
}

#[test]
fn rejects_missing_required_arguments() {
    let release = Release::new();
    for name in REQUIRED {
        release.run_without(name).misused(&format!(
            "missing required argument: --{name}\nsee `cf-release prepare-update --help`\n"
        ));
    }
    // The first the command asks for is the first missing.
    run(&words(&[])).misused("missing required argument: --bundle");
    let only_the_bundle = [("bundle".to_string(), release.bundle.display().to_string())];
    run(&words(&only_the_bundle)).misused("missing required argument: --archive");
    assert!(!release.output.exists());
}

#[test]
fn rejects_a_flag_without_a_value_or_given_twice() {
    let release = Release::new();
    let flags = release.flags();
    let mut valueless = words(&flags[..6]);
    valueless.push("--date".into());
    run(&valueless).misused("--date needs a value");

    let mut next_flag = words(&flags[..5]);
    next_flag.extend(["--channel", "--date", "2026-09-09T12:00:00Z"].map(Into::into));
    run(&next_flag).misused("--channel needs a value");

    release.run(&[]).finished();
    let mut twice = words(&flags);
    twice.extend(["--channel".into(), "stable".into()]);
    run(&twice).misused("duplicate argument: --channel");
}

#[test]
fn refuses_what_it_finds_first_in_the_order_it_asks() {
    // The channel, the date, the notes, the signature, the archive: each is
    // mended in turn, and the next is what is said.
    let release = Release::new();
    fs::write(&release.notes, "n".repeat(64 * 1024 + 1)).unwrap();
    fs::write(&release.signature, "junk").unwrap();
    fs::remove_file(&release.archive).unwrap();
    let bad = [("channel", "beta"), ("date", "next friday")];

    release
        .run(&bad)
        .refused("channel must be alpha or stable\n");
    release
        .run(&[("date", "next friday")])
        .refused("publication date must be RFC3339");
    release.run(&[]).refused("release notes exceed 64 KiB\n");
    fs::write(&release.notes, "fine\n").unwrap();
    release
        .run(&[])
        .refused("signature is not outer-base64 minisign text\n");
    fs::write(
        &release.signature,
        super::fixture::signature_for("a.app.tar.gz"),
    )
    .unwrap();
    release.run(&[]).refused("could not read archive: ");
    release.archive_again();
    release.run(&[]).finished();
}

#[test]
fn rejects_notes_that_are_missing_or_too_long() {
    let release = Release::new();
    fs::write(&release.notes, "n".repeat(64 * 1024)).unwrap();
    release.run(&[]).finished();
    fs::write(&release.notes, "n".repeat(64 * 1024 + 1)).unwrap();
    release.run(&[]).refused("release notes exceed 64 KiB\n");
    fs::remove_file(&release.notes).unwrap();
    release.run(&[]).refused(&format!(
        "could not read release notes: {}: No such file or directory",
        release.notes.display()
    ));
}
