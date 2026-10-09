//! The archive as a file: one that is not there, is empty, is not an archive or
//! is cut short, a name an asset cannot have, and the archives the system's own
//! `tar` makes, which the release's are.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use cf_base::env::Env;
use cf_release::process;
use flate2::write::GzEncoder;
use flate2::Compression;

use super::fixture::{differs, unsound, Release, VERSION};

#[test]
fn rejects_missing_and_empty_archives() {
    let release = Release::new();
    let missing = release.dir.path().join("nope.tar.gz");
    release
        .run(&[("archive", missing.to_str().unwrap())])
        .refused(&format!(
            "could not read archive: {}: No such file or directory",
            missing.display()
        ));

    fs::write(&release.archive, "").unwrap();
    release
        .run(&[])
        .refused("could not read archive: archive is empty\n");
}

#[test]
fn rejects_what_is_not_an_archive() {
    let release = Release::new();
    let said = "archive safety/content check failed: could not read the archive: ";

    // Not gzip; gzip of what is not tar; gzip of nothing; the right file cut short.
    fs::write(&release.archive, "this is not an archive").unwrap();
    release.run(&[]).refused(said);

    let gzip = |bytes: &[u8]| {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    };
    fs::write(&release.archive, gzip(b"hello")).unwrap();
    release.run(&[]).refused(said);
    fs::write(&release.archive, gzip(&[0xAB; 4096])).unwrap();
    release.run(&[]).refused(said);

    fs::write(&release.archive, gzip(b"")).unwrap();
    release
        .run(&[])
        .refused(&unsound("the update archive has no ConsensFlow.app root"));

    release.archive_again();
    let whole = fs::read(&release.archive).unwrap();
    fs::write(&release.archive, &whole[..whole.len() * 6 / 10]).unwrap();
    release.run(&[]).refused(said);
}

#[test]
fn rejects_a_folder_for_an_archive() {
    let release = Release::new();
    let folder = release
        .dir
        .path()
        .join(format!("ConsensFlow-{VERSION}.app.tar.gz"));
    fs::create_dir(&folder).unwrap();
    release
        .run(&[("archive", folder.to_str().unwrap())])
        .refused("archive safety/content check failed: could not read the archive: Is a directory");
}

#[test]
fn rejects_archive_filenames_without_the_version_or_not_of_a_safe_asset() {
    let release = Release::new();
    for name in [
        "ConsensFlow-latest.app.tar.gz",
        "ConsensFlow-3.0.0-alpha.98_aarch64.app.tar.gz",
        "ConsensFlow-3.0.0-alpha.99_aarch64.tar.gz",
        "ConsensFlow-3.0.0-alpha.99_aarch64.app.tgz",
        "ConsensFlow 3.0.0-alpha.99.app.tar.gz",
        "ConsensFlow-3.0.0-alpha.99;touch x.app.tar.gz",
    ] {
        let renamed = release.dir.path().join(name);
        fs::copy(&release.archive, &renamed).unwrap();
        release
            .run(&[("archive", renamed.to_str().unwrap())])
            .refused("archive filename must be a safe, versioned .app.tar.gz asset\n");
    }
    // Judged before the file is read.
    let renamed = release.dir.path().join("not-an-archive.txt");
    fs::write(&renamed, "text").unwrap();
    release
        .run(&[("archive", renamed.to_str().unwrap())])
        .refused("archive filename must be a safe, versioned .app.tar.gz asset\n");

    // And a name that is right is taken wherever the file is.
    let ok = release.dir.path().join("x-3.0.0-alpha.99.app.tar.gz");
    fs::copy(&release.archive, &ok).unwrap();
    release.run(&[("archive", ok.to_str().unwrap())]).finished();
    assert_eq!(
        release.entry()["platforms"]["darwin-aarch64"]["url"],
        "https://github.com/ngvoicu/consensflow/releases/download/v3.0.0-alpha.99/x-3.0.0-alpha.99.app.tar.gz"
    );
}

/// Three folders of this many letters each, below `parent`.
fn deep(parent: PathBuf, letters: usize) -> PathBuf {
    ["d", "e", "f"]
        .iter()
        .fold(parent, |path, part| path.join(part.repeat(letters)))
}

#[test]
fn takes_an_archive_the_systems_tar_makes_of_the_bundle_paths_of_any_length_among_them() {
    // `tar` as the release runs it (plain ustar), and as it runs by default, which
    // writes an extension for a path that a ustar header cannot hold.
    for format in [&["--format", "ustar"][..], &[]] {
        let release = Release::new();
        let name = "a-file-with-a-name-long-enough-to-matter.txt";
        // Past the 100 bytes of a ustar name, and cut at a slash into the header's
        // prefix and name: 127 bytes of folders, and the name.
        let split = deep(release.bundle.join("Contents").join("Resources"), 30);
        fs::create_dir_all(&split).unwrap();
        fs::write(split.join(name), "deep\n").unwrap();
        if format.is_empty() {
            // And past what a prefix holds (155 bytes), where ustar cannot go at all.
            let beyond = deep(release.bundle.join("Contents"), 60);
            fs::create_dir_all(&beyond).unwrap();
            fs::write(beyond.join("x.txt"), "deeper\n").unwrap();
        }

        let mut words: Vec<OsString> = vec!["-czf".into(), release.archive.clone().into()];
        words.extend(format.iter().map(OsString::from));
        words.extend([
            "-C".into(),
            release.bundle.parent().unwrap().into(),
            "ConsensFlow.app".into(),
        ]);
        let env = Env::from_vars([("COPYFILE_DISABLE", "1")]);
        let made = process::capture(OsStr::new("/usr/bin/tar"), &words, &env).unwrap();
        assert_eq!(made.code, 0, "{format:?}: {}", made.stderr);
        release.run(&[]).finished();

        // The same archive, with one byte of that file changed on disk since.
        fs::write(split.join(name), "deeP\n").unwrap();
        let long = format!(
            "ConsensFlow.app/Contents/Resources/{0}/{1}/{2}/{name}",
            "d".repeat(30),
            "e".repeat(30),
            "f".repeat(30)
        );
        release
            .run(&[])
            .refused(&differs(&long, "differs: the bytes differ"));
    }
}
