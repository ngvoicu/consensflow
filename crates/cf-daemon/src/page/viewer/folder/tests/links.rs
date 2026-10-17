//! Links, FIFOs and hard links, which Unix lets a test make: a link is never
//! listed, read or opened for the browser, wherever it leads, a FIFO is never
//! waited on, a file of two names is never read, and the words say what refused
//! each.

use std::os::unix::fs::symlink;

use nix::sys::stat::Mode as Permissions;
use nix::unistd::mkfifo;

use super::*;

#[test]
fn a_link_is_left_out_of_the_tree_and_refused_wherever_it_leads() {
    let project = project_with(&[
        ("user-auth/SPEC.md", "# Auth"),
        ("assets/spec-runtime.js", "1"),
    ]);
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("id_rsa"), "KEY").unwrap();
    let root = project.path().join(".specs");
    symlink(
        root.join("user-auth/SPEC.md"),
        root.join("user-auth/alias.md"),
    )
    .unwrap();
    symlink(outside.path().join("id_rsa"), root.join("user-auth/key.md")).unwrap();
    symlink(outside.path(), root.join("user-auth/away")).unwrap();
    symlink(&root, root.join("user-auth/loop")).unwrap();
    symlink(root.join("gone.md"), root.join("broken.md")).unwrap();
    symlink(
        outside.path().join("id_rsa"),
        root.join("user-auth/page.html"),
    )
    .unwrap();
    let folder = specs(&project);
    assert_eq!(
        serde_json::to_value(folder.tree().entries).unwrap(),
        json!([
            { "name": "assets", "path": "assets", "entries": [
                { "name": "spec-runtime.js", "path": "assets/spec-runtime.js", "size": 1 },
            ] },
            { "name": "user-auth", "path": "user-auth", "entries": [
                { "name": "SPEC.md", "path": "user-auth/SPEC.md", "size": 6 },
            ] },
        ])
    );
    for path in ["user-auth/alias.md", "user-auth/key.md", "broken.md"] {
        assert_eq!(
            read(&folder, path).unwrap_err(),
            format!("{path} is a link: the viewer opens no link"),
        );
    }
    assert_eq!(
        read(&folder, "user-auth/away/id_rsa").unwrap_err(),
        "user-auth/away/id_rsa goes through user-auth/away, a link: the viewer follows none"
    );
    assert_eq!(
        folder
            .for_browser("user-auth/page.html", &|| {})
            .unwrap_err(),
        "user-auth/page.html is a link: the viewer opens no link"
    );
}

/// The specs folder is found by a walk from the project's folder that follows
/// no link: one that is a link, or goes through one, is refused wherever it
/// leads, inside the project too; the project's own folder, which the human
/// chose, may be one.
#[test]
fn a_specs_folder_that_is_a_link_or_goes_through_one_is_refused_wherever_it_leads() {
    let project = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    symlink(elsewhere.path(), project.path().join(".specs")).unwrap();
    assert_eq!(
        found(project.path(), ".specs").err().unwrap(),
        "the specs folder .specs is a link: the viewer follows none"
    );
    fs::create_dir_all(project.path().join("docs/plans")).unwrap();
    fs::write(project.path().join("docs/plans/registry.md"), "r").unwrap();
    symlink(
        project.path().join("docs/plans"),
        project.path().join("plans"),
    )
    .unwrap();
    assert_eq!(
        found(project.path(), "plans").err().unwrap(),
        "the specs folder plans is a link: the viewer follows none"
    );
    symlink(
        project.path().join("docs"),
        project.path().join("documents"),
    )
    .unwrap();
    assert_eq!(
        found(project.path(), "documents/plans").err().unwrap(),
        "the specs folder documents/plans goes through documents, a link: the viewer follows \
         none"
    );
    let linked = tempfile::tempdir().unwrap();
    symlink(project.path(), linked.path().join("project")).unwrap();
    let folder = found(&linked.path().join("project"), "docs/plans")
        .unwrap()
        .unwrap();
    assert_eq!(
        read(&folder, "registry.md").unwrap().content,
        Content::Text("r".to_owned())
    );
}

/// The race at the folder's own level: a folder on the way to it swapped for a
/// link out, after the project's folder is held and before the walk opens it.
/// Opened in the folder held, as itself, it is refused: nothing outside is read.
#[test]
fn a_folder_on_the_way_swapped_for_a_link_out_is_refused() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join("docs/plans")).unwrap();
    fs::write(project.path().join("docs/plans/registry.md"), "ours").unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir_all(outside.path().join("plans")).unwrap();
    fs::write(outside.path().join("plans/registry.md"), "TOKEN=1").unwrap();
    let swap = || {
        fs::rename(project.path().join("docs"), project.path().join("moved")).unwrap();
        symlink(outside.path(), project.path().join("docs")).unwrap();
    };
    assert_eq!(
        open(project.path(), "docs/plans", &swap).err().unwrap(),
        "the specs folder docs/plans goes through docs, a link: the viewer follows none"
    );
}

#[test]
fn a_fifo_is_left_out_of_the_tree_and_refused_and_a_hard_link_is_refused() {
    let project = project_with(&[("a/notes.md", "notes")]);
    let root = project.path().join(".specs");
    mkfifo(
        &root.join("a/pipe.html"),
        Permissions::S_IRUSR | Permissions::S_IWUSR,
    )
    .unwrap();
    fs::hard_link(root.join("a/notes.md"), root.join("a/twin.html")).unwrap();
    let folder = specs(&project);
    assert_eq!(
        serde_json::to_value(folder.tree().entries).unwrap(),
        json!([{ "name": "a", "path": "a", "entries": [
            { "name": "notes.md", "path": "a/notes.md", "size": 5 },
            { "name": "twin.html", "path": "a/twin.html", "size": 5 },
        ] }])
    );
    assert_eq!(
        read(&folder, "a/pipe.html").unwrap_err(),
        "a/pipe.html is a FIFO, not a regular file"
    );
    assert_eq!(
        folder.for_browser("a/pipe.html", &|| {}).unwrap_err(),
        "a/pipe.html is a FIFO, not a regular file"
    );
    let twice = "a/twin.html has 2 names (hard links): the viewer reads only a file of one \
                 name, which no path outside the folder can share";
    assert_eq!(read(&folder, "a/twin.html").unwrap_err(), twice);
    assert_eq!(
        folder.for_browser("a/twin.html", &|| {}).unwrap_err(),
        twice
    );
}

/// The race at the folder's level: a file read through `read` swapped for a
/// link to a file outside between its look and its opening is refused.
#[test]
fn a_file_swapped_for_a_link_out_while_it_is_read_is_refused() {
    let project = project_with(&[("a/SPEC.md", "safe")]);
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.env"), "TOKEN=1").unwrap();
    let file = project.path().join(".specs/a/SPEC.md");
    let swap = || {
        fs::remove_file(&file).unwrap();
        symlink(outside.path().join("secret.env"), &file).unwrap();
    };
    assert_eq!(
        specs(&project).read("a/SPEC.md", &swap).unwrap_err(),
        "a/SPEC.md is a link: the viewer opens no link"
    );
}
