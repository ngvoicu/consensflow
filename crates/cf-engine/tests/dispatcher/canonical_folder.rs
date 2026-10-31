//! The folder a window works in, as the system names it (T-783): a project whose folder is
//! reached through a link (a Mac's `/var/folders/…`, whose real path is
//! `/private/var/folders/…`, or a project under `/tmp`) has its windows opened in the real
//! folder, and their launches prepared for it, on every harness. Devin 3000.11's screen
//! never showed an answer in a folder spelled through the link (T-724's D3), and what each
//! harness writes back names the real folder, its program's own path to it. Held to no
//! trace: Node opened a window in the folder as the project spelled it.

use cf_engine::testing::{Context, Made};
use cf_proto::agents::Harness;
use serde_json::json;

#[cfg(unix)]
#[test]
fn a_window_of_a_project_reached_through_a_link_opens_in_the_real_folder_on_every_harness() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("project");
    std::fs::create_dir(&real).unwrap();
    let linked = root.path().join("linked");
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    let real = std::fs::canonicalize(&real).unwrap();
    let (real, linked) = (real.to_string_lossy(), linked.to_string_lossy());
    assert_ne!(real, linked);
    // Every harness there is, as the code lists them.
    let harnesses: Vec<&'static str> = Harness::ALL.into_iter().map(Harness::kind).collect();
    for &harness in &harnesses {
        let context = Context::made(Made {
            harnesses: harnesses.clone(),
            ..Made::default()
        });
        context
            .open_project(json!({
                "directory": &*linked,
                "name": "app",
                "chief": { "harness": harness, "agent": "apollo" },
            }))
            .unwrap()
            .unwrap();
        context.settle();
        let opened = context.host.opened();
        let window = opened.last().expect("the chief's window opened");
        assert_eq!(
            window.cwd, real,
            "{harness}: the window works in the real folder"
        );
        let launch = context.adapter.prepared();
        assert_eq!(
            launch.last().expect("the chief's launch")["directory"],
            json!(&*real),
            "{harness}: the launch is prepared for the real folder"
        );
        // The project keeps the folder as the human named it.
        assert_eq!(
            context.ledger.borrow().projects().unwrap()[0].directory,
            linked
        );
        context.close();
    }
}

#[test]
fn a_window_of_a_project_whose_folder_is_no_link_opens_in_it_as_named() {
    // What every trace holds: a folder that is no link, and one not there at all, as named.
    let context = Context::new();
    context.with_staff(&[]);
    assert_eq!(context.host.opened()[0].cwd, "/work/app");
    assert_eq!(
        context.adapter.prepared()[0]["directory"],
        json!("/work/app")
    );
    context.close();
}
