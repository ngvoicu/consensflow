//! A design task's result says what its image files are (designer P1, T-1280): after the
//! designer's words, a line for each image file they name, with its size, its type and
//! whether it has alpha, read from the file's header. The chief ordered a size and a
//! transparency in words and learns whether the picture has them without opening it
//! (Codex's tool drew 1672x941 for an order of 1280x720). It is the result of a design task
//! alone, however it comes: the turn's end, or the chief's Finish. Node's dispatcher had no
//! designer to read, so no trace holds these.

use std::path::Path;

use cf_engine::testing::Context;
use cf_harness::records::Pending;
use cf_ledger::{NewTask, ProjectView};
use serde_json::json;

/// A job, as a reading counts it, which holds a turn's end until the chief finishes the task.
const A_JOB: Pending = Pending {
    agents: 0,
    jobs: 1,
    retired: 0,
    noticed: 0,
};

/// A project with an image designer and a worker.
fn project(context: &Context) -> ProjectView {
    context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "model": "apollo" },
            "staff": [
                { "agent": "osiris", "harness": "claude-code", "roles": ["designer"], "designer": true, "tier": "light" },
                { "agent": "zeus", "harness": "claude-code", "roles": ["worker"], "tier": "standard" },
            ],
        }))
        .unwrap()
        .expect("the project")
}

/// The chief orders T-1 of `pool`, and `handle`'s window takes it in.
fn ordered(context: &Context, project: &ProjectView, pool: &str, handle: &str) {
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some(pool.to_owned()),
            tier: (pool != "designer").then(|| "standard".to_owned()),
            body: "Draw a kite".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let thread = context.task(project.id, 1);
    assert_eq!(thread.task.state, "working");
    let window = thread.task.assignee.expect("a window has it");
    assert!(
        window.starts_with(handle),
        "{window} is a window of @{handle}"
    );
}

/// The results the chief was given.
fn results(context: &Context, project: &ProjectView) -> Vec<String> {
    context
        .inbox(context.id(project.id, "chief"))
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

/// A PNG of `width` by `height`, RGBA: the header, some pixels and the end.
fn png(width: u32, height: u32) -> Vec<u8> {
    let chunk = |kind: &[u8; 4], data: &[u8]| {
        let mut bytes = (data.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(&[0; 4]);
        bytes
    };
    let mut header = width.to_be_bytes().to_vec();
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend(chunk(b"IHDR", &header));
    bytes.extend(chunk(b"IDAT", &[0; 4]));
    bytes.extend(chunk(b"IEND", &[]));
    bytes
}

fn saved(dir: &Path, name: &str, width: u32, height: u32) -> String {
    let path = dir.join(name);
    std::fs::write(&path, png(width, height)).unwrap();
    path.display().to_string()
}

#[test]
fn a_design_task_s_result_says_the_size_the_type_and_the_alpha_of_each_file_it_names() {
    let context = Context::new();
    let project = project(&context);
    ordered(&context, &project, "designer", "osiris");
    let dir = tempfile::tempdir().unwrap();
    let (kite, sky) = (
        saved(dir.path(), "kite.png", 1672, 941),
        saved(dir.path(), "sky.png", 64, 64),
    );
    let words = format!("{kite}\n{sky}\nA kite over a sky.");
    context.adapter.answer("osiris", &words);
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");
    assert_eq!(
        results(&context, &project),
        [format!(
            "{words}\n\n(ConsensFlow read {kite}: 1672x941 pixels, PNG, with alpha)\n(ConsensFlow read {sky}: 64x64 pixels, PNG, with alpha)"
        )]
    );
}

#[test]
fn the_result_the_chief_takes_at_its_finish_says_it_too() {
    let context = Context::new();
    let project = project(&context);
    ordered(&context, &project, "designer", "osiris");
    let dir = tempfile::tempdir().unwrap();
    let kite = saved(dir.path(), "kite.png", 1280, 720);
    let words = format!("{kite}\nA kite.");
    // The turn ends with a job running: the task stays working until the chief finishes it.
    context.adapter.answer_leaving("osiris", &words, A_JOB);
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    let finished = context
        .finish_task(project.id, 1, "chief")
        .expect("finished");
    assert_eq!(finished.task.state, "done");
    let results = results(&context, &project);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(
        results[0].starts_with(&format!(
            "{words}\n\n(ConsensFlow read {kite}: 1280x720 pixels, PNG, with alpha)\n\n(ConsensFlow: Finished at @chief's word"
        )),
        "the facts come before what ConsensFlow says of the leftovers: {results:?}"
    );
}

#[test]
fn a_file_that_is_gone_or_is_no_image_leaves_the_result_as_the_designer_wrote_it() {
    let context = Context::new();
    let project = project(&context);
    ordered(&context, &project, "designer", "osiris");
    let dir = tempfile::tempdir().unwrap();
    let words = format!(
        "{}\nA kite, which did not save.",
        dir.path().join("gone.png").display()
    );
    context.adapter.answer("osiris", &words);
    context.pass().unwrap();
    assert_eq!(results(&context, &project), [words]);
}

#[test]
fn only_a_design_task_s_result_is_read_for_images() {
    let context = Context::new();
    let project = project(&context);
    ordered(&context, &project, "worker", "zeus");
    let dir = tempfile::tempdir().unwrap();
    let words = format!(
        "{}\nDrawn by hand.",
        saved(dir.path(), "kite.png", 1280, 720)
    );
    context.adapter.answer("zeus", &words);
    context.pass().unwrap();
    assert_eq!(results(&context, &project), [words]);
}
