//! The two commands against a checkout in a temporary folder: their defaults,
//! their arguments, what they say, and what they refuse. The library's own tests
//! hold the format; these hold what `cargo xtask` makes of it.

use super::*;

use std::fs;
use std::path::Path;

use cf_base::env::Env;

/// The app's exe in the release folders below: seven bytes, so that where the
/// payload starts is known.
const APP: &[u8] = b"the app";

/// Where the build leaves the release folder in a checkout at `root`, folder by
/// folder as the system writes a path: what xtask says of it is written so.
fn release_folder(root: &Path) -> PathBuf {
    ["app", "src-tauri", "target", "release"]
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// A release folder as `tauri build` leaves it on Windows, in `dir`.
fn release(dir: &Path) {
    for (path, body) in [
        ("ConsensFlow.exe", "the app"),
        ("cli/bin/cf.exe", "the native cf"),
        ("conpty.dll", "the console host"),
        ("OpenConsole.exe", "its process"),
        ("OpenConsole-LICENSE.txt", "its license"),
    ] {
        write(&dir.join(path), body);
    }
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A checkout in a temporary folder whose sources all say `version`, with the
/// release folder where the build leaves it.
fn checkout(version: &str) -> (tempfile::TempDir, Context) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("package.json"),
        &format!(r#"{{"version": "{version}"}}"#),
    );
    write(
        &root.join("Cargo.toml"),
        &format!("[workspace.package]\nversion = \"{version}\"\n"),
    );
    write(
        &root.join("app/src-tauri/tauri.conf.json"),
        &format!(r#"{{"version": "{version}"}}"#),
    );
    release(&release_folder(root));
    let context = Context {
        root: root.to_path_buf(),
        env: Env::default(),
    };
    (dir, context)
}

/// How a command ended, and what it said on its standard output.
type Ran = (Result<i32, Failure>, String);

/// A command's function, as the table of commands holds it.
type Handler = fn(&Context, &[OsString], &mut Console) -> Result<i32, Failure>;

fn ran(command: Handler, context: &Context, args: &[&str]) -> Ran {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let result = command(
        context,
        &args,
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
    );
    assert!(
        err.is_empty(),
        "the commands leave the error stream to xtask"
    );
    (result, String::from_utf8(out).unwrap())
}

fn pack_with(context: &Context, args: &[&str]) -> Ran {
    ran(run_pack, context, args)
}

fn inspect_with(context: &Context, args: &[&str]) -> Ran {
    ran(run_inspect, context, args)
}

/// What a command that was refused said, and that it was a command line it
/// does not take (the status 2), with nothing said on the standard output.
fn usage_said((result, out): Ran) -> String {
    assert_eq!(out, "", "nothing is said of a refused command line");
    match result.unwrap_err() {
        Failure::Usage(said) => said,
        other => panic!("not a usage failure: {other}"),
    }
}

/// What a command that failed said of it (the status 1), with nothing said on
/// the standard output.
fn failed_with((result, out): Ran) -> String {
    assert_eq!(out, "", "nothing is said of a command that failed");
    let failure = result.unwrap_err();
    assert!(!matches!(failure, Failure::Usage(_)), "{failure}");
    failure.to_string()
}

/// The build's own bundle folder, where `pack` puts the exe by default.
fn bundle(root: &Path) -> PathBuf {
    release_folder(root).join("bundle")
}

fn exe_named(version: &str) -> String {
    format!("ConsensFlow_{version}_x64-portable.exe")
}

#[test]
fn the_two_commands_run_in_rust_and_are_named_pack_and_inspect() {
    let words: Vec<_> = COMMANDS.iter().map(|command| command.words).collect();
    assert_eq!(
        words,
        [&["portable", "pack"][..], &["portable", "inspect"][..]]
    );
    for command in COMMANDS {
        assert!(matches!(command.run, Run::Native(_)), "{:?}", command.words);
    }
}

#[test]
fn pack_with_no_options_packs_the_builds_release_folder_into_its_bundle_named_by_the_sources() {
    let (dir, context) = checkout("3.0.0-alpha.99");

    let (result, out) = pack_with(&context, &[]);

    assert_eq!(result.unwrap(), 0);
    let exe = bundle(dir.path())
        .join("portable")
        .join(exe_named("3.0.0-alpha.99"));
    assert_eq!(out, format!("portable: {}\n", exe.display()));
    let bytes = fs::read(&exe).unwrap();
    assert_eq!(&bytes[..APP.len()], APP, "the app first, byte for byte");
    assert_eq!(&bytes[bytes.len() - 8..], b"CFPAYLD1");
    assert_eq!(cf_portable::inspect(&exe).unwrap().offset, APP.len() as u64);
}

#[test]
fn an_option_takes_its_value_after_it_or_joined_to_it_by_the_first_equals_sign() {
    let (dir, context) = checkout("3.0.0-alpha.99");
    let root = dir.path();
    release(&root.join("build"));
    let build = root.join("build");
    let (first, second) = (root.join("dist"), root.join("a=b"));

    let by_words = pack_with(
        &context,
        &[
            "--release",
            build.to_str().unwrap(),
            "--out",
            first.to_str().unwrap(),
            "--version",
            "1.2.3",
        ],
    );
    let joined = pack_with(
        &context,
        &[
            &format!("--release={}", build.display()),
            &format!("--out={}", second.display()),
            "--version=1.2.3",
        ],
    );

    assert_eq!(by_words.0.unwrap(), 0);
    assert_eq!(joined.0.unwrap(), 0);
    assert_eq!(
        by_words.1,
        format!("portable: {}\n", first.join(exe_named("1.2.3")).display())
    );
    assert_eq!(
        joined.1,
        format!("portable: {}\n", second.join(exe_named("1.2.3")).display())
    );
    assert_eq!(
        fs::read(first.join(exe_named("1.2.3"))).unwrap(),
        fs::read(second.join(exe_named("1.2.3"))).unwrap()
    );
}

/// The folders it was told are from the checkout's root, whichever folder
/// `cargo xtask` was run in.
#[test]
fn a_relative_path_is_from_the_checkouts_root() {
    let (dir, context) = checkout("3.0.0-alpha.99");
    release(&dir.path().join("build"));

    let (result, out) = pack_with(
        &context,
        &["--release", "build", "--out", "dist", "--version", "9.9.9"],
    );

    assert_eq!(result.unwrap(), 0);
    let exe = dir.path().join("dist").join(exe_named("9.9.9"));
    assert_eq!(out, format!("portable: {}\n", exe.display()));
    assert!(exe.is_file());
}

/// With no `--out`, the exe goes beside the release folder it was packed from,
/// not into the build's.
#[test]
fn a_release_folder_alone_has_its_exe_in_its_own_bundle_folder() {
    let (dir, context) = checkout("3.0.0-alpha.99");
    release(&dir.path().join("elsewhere"));

    let (result, out) = pack_with(&context, &["--release", "elsewhere"]);

    assert_eq!(result.unwrap(), 0);
    let exe = dir
        .path()
        .join("elsewhere")
        .join("bundle")
        .join("portable")
        .join(exe_named("3.0.0-alpha.99"));
    assert_eq!(out, format!("portable: {}\n", exe.display()));
    assert!(exe.is_file());
    assert!(
        !bundle(dir.path()).exists(),
        "the build's own is left alone"
    );
}

#[test]
fn a_version_given_names_the_file_and_the_sources_are_not_asked() {
    let (dir, context) = checkout("1.0.0");
    // The sources disagree, which only a pack that asks them would mind.
    write(&dir.path().join("package.json"), r#"{"version": "2.0.0"}"#);

    let (result, out) = pack_with(&context, &["--version", "9.9.9"]);

    assert_eq!(result.unwrap(), 0);
    let exe = bundle(dir.path()).join("portable").join(exe_named("9.9.9"));
    assert_eq!(out, format!("portable: {}\n", exe.display()));
}

#[test]
fn sources_that_disagree_refuse_a_pack_without_a_version_and_nothing_is_written() {
    let (dir, context) = checkout("1.0.0");
    write(&dir.path().join("package.json"), r#"{"version": "2.0.0"}"#);
    write(
        &dir.path().join("app/src-tauri/tauri.conf.json"),
        r#"{"version": "3.0.0"}"#,
    );

    let said = failed_with(pack_with(&context, &[]));

    assert_eq!(
        said,
        "source package, Cargo and Tauri versions do not match: \
         package.json 2.0.0, Cargo.toml 1.0.0, tauri.conf.json 3.0.0"
    );
    assert!(!bundle(dir.path()).exists());
}

#[test]
fn a_piece_missing_from_the_release_folder_is_refused_naming_it_and_nothing_is_written() {
    let (dir, context) = checkout("3.0.0-alpha.99");
    let folder = release_folder(dir.path());
    fs::remove_file(folder.join("conpty.dll")).unwrap();

    let said = failed_with(pack_with(&context, &[]));

    assert_eq!(
        said,
        format!(
            "portable: conpty.dll is missing from {}; build first with npm --prefix app run build",
            folder.display()
        )
    );
    assert!(!bundle(dir.path()).exists());
}

/// A command line that is not one `pack` takes is refused before anything is
/// done: with the status 2, and nothing written.
#[test]
fn pack_refuses_what_it_does_not_take() {
    let takes = "portable pack takes [--release DIR] [--out DIR] [--version X]";
    let (dir, context) = checkout("3.0.0-alpha.99");
    for (args, said) in [
        (vec!["--ou", "dist"], format!("{takes}, not --ou")),
        (vec!["--ou=dist"], format!("{takes}, not --ou=dist")),
        (vec!["dist"], format!("{takes}, not dist")),
        (vec!["-x"], format!("{takes}, not -x")),
        (vec!["--help"], format!("{takes}, not --help")),
        (
            vec!["--out", "dist", "extra", "words"],
            format!("{takes}, not extra words"),
        ),
        (
            vec!["--out"],
            "portable pack: --out needs a value".to_string(),
        ),
        (
            vec!["--out", "--version", "1.2.3"],
            "portable pack: --out needs a value".to_string(),
        ),
        (
            vec!["--version="],
            "portable pack: --version needs a value".to_string(),
        ),
        (
            vec!["--out", "a", "--out", "b"],
            "portable pack: --out is given twice".to_string(),
        ),
        (
            vec!["--out=a", "--out", "b"],
            "portable pack: --out is given twice".to_string(),
        ),
        (
            vec!["--version", "1.2.3", "--version=1.2.4"],
            "portable pack: --version is given twice".to_string(),
        ),
    ] {
        assert_eq!(usage_said(pack_with(&context, &args)), said, "{args:?}");
        assert!(!bundle(dir.path()).exists(), "{args:?}");
        assert!(!dir.path().join("dist").exists(), "{args:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_version_that_is_not_text_is_refused() {
    use std::os::unix::ffi::OsStringExt;

    let (dir, context) = checkout("3.0.0-alpha.99");
    let args = [
        OsString::from("--version"),
        OsString::from_vec(vec![b'1', 0xff]),
    ];
    let (mut out, mut err) = (Vec::new(), Vec::new());

    let failure = run_pack(
        &context,
        &args,
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
    )
    .unwrap_err();

    assert!(
        matches!(&failure, Failure::Usage(said) if said == "portable pack: --version is not text"),
        "{failure}"
    );
    assert!(!bundle(dir.path()).exists());
}

/// A file as `pack` writes it, in the checkout's default place.
fn packed(dir: &Path, context: &Context, version: &str) -> PathBuf {
    let (result, _) = pack_with(context, &[]);
    assert_eq!(result.unwrap(), 0);
    bundle(dir).join("portable").join(exe_named(version))
}

#[test]
fn inspect_says_the_payload_and_the_crc_and_with_the_version_the_runtime_folder() {
    let (dir, context) = checkout("3.0.0-alpha.99");
    let exe = packed(dir.path(), &context, "3.0.0-alpha.99");
    let found = cf_portable::inspect(&exe).unwrap();
    // The payload is all of the file between the app and the footer's sixteen bytes.
    let size = fs::metadata(&exe).unwrap().len();
    assert_eq!(found.length, size - APP.len() as u64 - 16);
    let path = exe.to_str().unwrap();

    let bare = inspect_with(&context, &[path]);
    let after = inspect_with(&context, &[path, "--version", "3.0.0-alpha.99"]);
    let before = inspect_with(&context, &["--version=3.0.0-alpha.99", path]);

    assert_eq!(bare.0.unwrap(), 0);
    assert_eq!(
        bare.1,
        format!("payload: {} bytes\ncrc: {:08x}\n", found.length, found.crc)
    );
    let said = format!(
        "payload: {} bytes\ncrc: {:08x}\nruntime: 3.0.0-alpha.99-{:08x}\n",
        found.length, found.crc, found.crc
    );
    assert_eq!(after.0.unwrap(), 0);
    assert_eq!(after.1, said);
    assert_eq!(before.0.unwrap(), 0);
    assert_eq!(before.1, said);
}

/// The file is from the checkout's root when it is a relative path.
#[test]
fn inspect_takes_a_relative_path_from_the_checkouts_root() {
    let (dir, context) = checkout("3.0.0-alpha.99");
    let exe = packed(dir.path(), &context, "3.0.0-alpha.99");
    let relative = exe.strip_prefix(dir.path()).unwrap().to_str().unwrap();
    assert!(Path::new(relative).is_relative());

    let (result, out) = inspect_with(&context, &[relative]);

    assert_eq!(result.unwrap(), 0);
    assert!(out.starts_with("payload: "), "{out}");
}

#[test]
fn inspect_refuses_what_it_does_not_take() {
    let takes = "portable inspect takes EXE [--version X]";
    let (_dir, context) = checkout("3.0.0-alpha.99");
    for (args, said) in [
        (vec![], takes.to_string()),
        (vec!["--version", "1.2.3"], takes.to_string()),
        (vec!["a.exe", "b.exe"], format!("{takes}, not a.exe b.exe")),
        (vec!["a.exe", "--out", "x"], format!("{takes}, not --out")),
        (
            vec!["a.exe", "--version"],
            "portable inspect: --version needs a value".to_string(),
        ),
        (
            vec!["a.exe", "--version=1", "--version=2"],
            "portable inspect: --version is given twice".to_string(),
        ),
    ] {
        assert_eq!(usage_said(inspect_with(&context, &args)), said, "{args:?}");
    }
}

/// A file that cannot be inspected is an error that names it (the status 1),
/// whichever way it is wrong.
#[test]
fn inspect_names_a_file_that_carries_no_footer_or_a_damaged_one_or_is_not_there() {
    let (dir, context) = checkout("3.0.0-alpha.99");
    let none = dir.path().join("installed.exe");
    write(&none, "MZ an installed app, longer than a footer");
    let damaged = dir.path().join("damaged.exe");
    let mut bytes = b"MZ the app and some".to_vec();
    bytes.extend_from_slice(&cf_portable::footer(1_000));
    fs::write(&damaged, &bytes).unwrap();
    let absent = dir.path().join("absent.exe");

    let no_footer = failed_with(inspect_with(&context, &[none.to_str().unwrap()]));
    let damaged_said = failed_with(inspect_with(&context, &[damaged.to_str().unwrap()]));
    let absent_said = failed_with(inspect_with(&context, &[absent.to_str().unwrap()]));

    assert_eq!(
        no_footer,
        format!(
            "portable: {} does not end with the portable footer",
            none.display()
        )
    );
    assert_eq!(
        damaged_said,
        format!(
            "portable: could not read {}: its footer names 1000 bytes of payload in a file of {}",
            damaged.display(),
            bytes.len()
        )
    );
    assert!(
        absent_said.starts_with(&format!("portable: could not open {}: ", absent.display())),
        "{absent_said}"
    );
}
