//! The bootstrap looks at the profile and at the copy before it makes, locks or unpacks anything
//! (T-1368, after the third check, T-1382).
//!
//! The second round put the look in the sweep, which runs after the unpack. The unpack is `tar -xmzf`
//! into the copy, and tar replaces a file of the same name: if the copy is a junction into a folder that
//! holds the owner's `README.md`, and the tree has one too, the owner's file is gone before the sweep
//! refuses the link. So the bootstrap itself looks first, and where the profile or the copy is a link it
//! names it and ends the run (1) with nothing made there, nothing locked, nothing unpacked, and its own
//! tarball (the one thing it may remove, by its exact name) gone.
//!
//! Each case runs the bootstrap text as it is sent, in the machine's PowerShell, on a profile made for
//! it. On the Mac the unpack is done by the system's bsdtar (which is what Windows' own tar is) behind a
//! stand-in `System32/tar.exe`, so that the case that matters, a file replaced by the unpack, happens
//! there as it does on Windows.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use cf_base::env::Env;

use super::powershell::shell;
use super::root::link;
use super::*;
use crate::context::Context;
use crate::process::{self, Invocation};
use crate::windows::tree;

/// The run's name in these cases.
const RUN: &str = "0000c0de-00000001";
/// What the owner's `README.md` says, and what the tree's says.
const OWNERS: &[u8] = b"THE OWNER'S README: not the tree's to replace\n";
const TREES: &[u8] = b"the tree's README\n";

/// The script of the run, as the runner makes it for these cases.
fn script_of_case() -> Script<'static> {
    Script {
        id: RUN,
        dir: DIR,
        inside: ".",
        steps: &[],
        command: "exit 0",
        env: &[],
        node: r"C:\Program Files\nodejs",
        machine: "zeewin2",
        limits: None,
    }
}

/// This process's environment less what a hook that runs the tests would leave in it for git to read.
fn environment() -> Env {
    Env::from_vars(
        Env::from_process()
            .iter()
            .filter(|(name, _)| !name.to_string_lossy().starts_with("GIT_"))
            .map(|(name, value)| (name.to_owned(), value.to_owned())),
    )
}

/// Every file under `dir` with its bytes, by its path from `dir` with `/` (a link is not entered).
fn listing(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, at: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            let kind = fs::symlink_metadata(&path).unwrap();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if kind.is_dir() {
                into.insert(format!("{name}/"), Vec::new());
                walk(root, &path, into);
            } else if kind.is_file() {
                into.insert(name, fs::read(&path).unwrap());
            }
        }
    }
    let mut into = BTreeMap::new();
    walk(dir, dir, &mut into);
    into
}

/// What a run of the bootstrap came to.
struct Ran {
    code: Option<i32>,
    out: String,
}

/// The bootstrap of a run that sends a tree of `README.md` (the tree's) and `keep.txt`, run in `shell`
/// with `profile` as the user's: the run's tarball is put in the profile, as the upload does.
fn bootstrap_run(shell: &str, scratch: &Path, label: &str, profile: &Path) -> Ran {
    let env = environment();
    let scratch = &scratch.join(label);
    let root = scratch.join("tree");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("README.md"), TREES).unwrap();
    fs::write(root.join("keep.txt"), b"kept").unwrap();
    // A `cargo` of the tree's own, which cmd finds first: what a run makes sure of before its command is
    // none of these cases' business.
    fs::write(root.join("cargo.cmd"), b"@exit /b 0\r\n").unwrap();
    for words in [&["init", "-q"][..], &["add", "-A"]] {
        let ran = process::capture(&Invocation::new("git", &root).args(words), &env).unwrap();
        assert_eq!(ran.code, 0, "git {words:?}: {}", ran.stderr);
    }
    let context = Context {
        root,
        env: env.clone(),
    };
    let files = tree::files(&context).unwrap();
    let identity = tree::identity(&context, &files).unwrap();
    let script = script_of_case();
    let pack = scratch.join("pack");
    fs::create_dir_all(&pack).unwrap();
    let packed = tree::pack(&context, &files, &identity, &run_script(&script), &pack).unwrap();
    fs::copy(&packed, profile.join(upload_name(DIR, RUN))).unwrap();
    // Where the system's tar is: Windows has it; on the Mac, a stand-in that is the system's bsdtar.
    let system = scratch.join("system");
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(system.join("System32")).unwrap();
        let tar = system.join("System32").join("tar.exe");
        fs::write(&tar, "#!/bin/sh\nexec /usr/bin/tar \"$@\"\n").unwrap();
        fs::set_permissions(&tar, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let set_system = if cfg!(windows) {
        String::new()
    } else {
        format!("$env:SystemRoot = '{}'\n", system.display())
    };
    let file = scratch.join("bootstrap.ps1");
    fs::write(
        &file,
        format!(
            "$env:USERPROFILE = '{}'\n{set_system}{}",
            profile.display(),
            remote_script(&script)
        ),
    )
    .unwrap();
    let ran = process::capture(
        &Invocation::new(shell, scratch)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(file.display().to_string()),
        &Env::from_process().with_var(
            "XDG_CACHE_HOME",
            &scratch.join("cache").display().to_string(),
        ),
    )
    .unwrap();
    Ran {
        code: Some(ran.code),
        out: ran.stdout.replace("\r\n", "\n"),
    }
}

/// The tarballs in `profile`.
fn tarballs(profile: &Path) -> Vec<String> {
    listing(profile)
        .into_keys()
        .filter(|name| name.ends_with(".tgz"))
        .collect()
}

#[test]
fn a_copy_that_is_a_link_is_refused_before_the_unpack_can_replace_a_file_of_the_folder_it_leads_to()
{
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let scratch = tempfile::tempdir().unwrap();
    let profile = scratch.path().join("profile");
    fs::create_dir(&profile).unwrap();
    // The owner's folder, which holds a README.md the tree has one of too.
    let elsewhere = scratch.path().join("elsewhere");
    fs::create_dir_all(elsewhere.join("owner")).unwrap();
    fs::write(elsewhere.join("README.md"), OWNERS).unwrap();
    fs::write(elsewhere.join("owner/notes.txt"), b"notes").unwrap();
    let copy = profile.join(DIR);
    link(shell, scratch.path(), &copy, &elsewhere);
    let before = listing(&elsewhere);
    let ran = bootstrap_run(shell, scratch.path(), "run", &profile);
    assert_eq!(
        fs::read(elsewhere.join("README.md")).unwrap(),
        OWNERS,
        "the owner's file is as it was: {}",
        ran.out
    );
    assert_eq!(
        listing(&elsewhere),
        before,
        "nothing was made, locked or unpacked in the folder the link leads to: {}",
        ran.out
    );
    assert_eq!(ran.code, Some(1), "{}", ran.out);
    assert!(
        ran.out.contains(&format!("{} is a link", copy.display())),
        "the link is named: {}",
        ran.out
    );
    assert!(
        ran.out.contains("\n== exit 1"),
        "it ends with its exit line: {}",
        ran.out
    );
    assert!(!ran.out.contains("== tree "), "{}", ran.out);
    assert!(
        tarballs(&profile).is_empty(),
        "its own tarball is taken away: {:?}",
        tarballs(&profile)
    );
}

#[test]
fn a_profile_that_is_a_link_is_refused_before_anything_is_made_in_the_folder_it_leads_to() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let scratch = tempfile::tempdir().unwrap();
    // The profile the user has is a link to a folder that holds a copy with the owner's README.md in it.
    let real = scratch.path().join("real");
    fs::create_dir_all(real.join(DIR)).unwrap();
    fs::write(real.join(DIR).join("README.md"), OWNERS).unwrap();
    let profile = scratch.path().join("profile");
    link(shell, scratch.path(), &profile, &real);
    let before = listing(&real);
    let ran = bootstrap_run(shell, scratch.path(), "run", &profile);
    assert_eq!(
        fs::read(real.join(DIR).join("README.md")).unwrap(),
        OWNERS,
        "{}",
        ran.out
    );
    // The tarball was put in the profile before the run, and is gone after it: the folder is as it was.
    assert_eq!(
        listing(&real),
        before,
        "nothing was made, locked or unpacked behind the link: {}",
        ran.out
    );
    assert_eq!(ran.code, Some(1), "{}", ran.out);
    assert!(
        ran.out
            .contains(&format!("{} is a link", profile.display())),
        "the profile is named: {}",
        ran.out
    );
    assert!(!ran.out.contains("== tree "), "{}", ran.out);
}

#[test]
fn a_copy_that_is_a_real_folder_or_not_there_yet_is_unpacked_as_it_always_was() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let scratch = tempfile::tempdir().unwrap();
    let profile = scratch.path().join("profile");
    fs::create_dir(&profile).unwrap();
    // The first run: no copy yet.
    let first = bootstrap_run(shell, scratch.path(), "first", &profile);
    assert!(!first.out.contains("is a link"), "{}", first.out);
    assert!(first.out.contains("== tree "), "{}", first.out);
    assert_eq!(
        fs::read(profile.join(DIR).join("README.md")).unwrap(),
        TREES
    );
    // The next: a real folder with an old README.md of the tree's own, which the unpack replaces.
    fs::write(profile.join(DIR).join("README.md"), b"an old one").unwrap();
    let second = bootstrap_run(shell, scratch.path(), "second", &profile);
    assert!(!second.out.contains("is a link"), "{}", second.out);
    assert!(second.out.contains("== tree "), "{}", second.out);
    assert_eq!(
        fs::read(profile.join(DIR).join("README.md")).unwrap(),
        TREES
    );
}

#[test]
fn the_look_at_the_profile_and_the_copy_is_the_first_thing_the_bootstrap_does_in_the_folder() {
    let text = bootstrap();
    let guard = text
        .find("is a link")
        .expect("the bootstrap says a link is refused");
    for later in [
        "New-Item -ItemType Directory",
        "[IO.File]::Open(",
        "Set-Content -Path $running",
        "& $tar -xmzf",
        "[scriptblock]::Create",
    ] {
        let at = text
            .find(later)
            .unwrap_or_else(|| panic!("{later}: {text}"));
        assert!(guard < at, "the look follows {later}: {text}");
    }
    // Both folders, the profile first; a folder that is not there yet is no link; the run ends with its line.
    assert!(text.contains("$env:USERPROFILE,$dir|%{if(("), "{text}");
    assert!(text.contains("-EA 0"), "{text}");
    assert!(text.contains("-band 1024"), "{text}");
    assert!(text.contains(";Exit-Run 1}}"), "{text}");
}
