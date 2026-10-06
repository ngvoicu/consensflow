//! The real reader of the pane host's frames (`cf_bridge::local`'s
//! `read_loop`), with the daemon's checkpoint ([`daemon_bridge`]): the engine
//! opens a worker's window, and the host's answer to the open and the exit of
//! the window it opened come in the arrangements below.
//!
//! In Node the frames of one `data` event were all handled before any promise
//! continuation ran, and the continuations all ran before the next `data`
//! event. So the exit of a window whose open is answered in the same read is
//! handled while the window is still opening, and the launch takes it
//! (`started` is never asked); the exit that comes in the next read finds the
//! window launched (`started` was asked). Each arrangement is held to the
//! kit's run of the same callbacks ([`super::reference`]), and to the one
//! thing that tells them apart, whether the adapter was asked `started`.
//!
//! The reader is ahead of the executor's driver in every one of them: it
//! reads again without yielding, the second read being ready, so the driver
//! cannot run between the reads, and only the checkpoint after the first can
//! put the work the answer woke before the exit.

use cf_proto::bridge::Frame;
use serde_json::Value;

use super::host::{answer, padding, pane_of, READ_BYTES};
use super::reference::{exit_after_the_answer, exit_with_the_answer};
use super::rig::Rig;
use super::{scene, settle};

/// A rig whose chief window is open and whose worker `zeus` has been given a
/// task and its open asked: the request, unanswered, and the pane it names.
async fn zeus_opening() -> (Rig, Frame, Value) {
    let mut rig = Rig::new().await;
    let project = rig.open_project(&["zeus"]).await;
    rig.give(project.id, "zeus", "Parser");
    rig.pass().await;
    // Everything the pass asks but zeus's open is answered, until the rest of
    // the pass has nothing left to wait for.
    let mut opens = Vec::new();
    for _ in 0..4 {
        opens.extend(rig.pieces.host.serve(|frame| frame.op == "pane.open").await);
    }
    assert_eq!(opens.len(), 1, "zeus's window is asked for: {opens:?}");
    let open = opens.remove(0);
    assert_eq!(open.body["id"], "p1-zeus");
    let pane = pane_of(&open);
    (rig, open, pane)
}

/// What the engine did, as the kit's runs of both arrangements say it.
async fn finished(rig: &mut Rig) -> (String, Vec<(String, Value)>, usize) {
    rig.quiet().await;
    (rig.task_state(1, 1), rig.events(), rig.calls("started"))
}

#[test]
fn the_kit_tells_the_two_arrangements_apart_by_whether_the_window_was_asked_started() {
    let with = exit_with_the_answer();
    let after = exit_after_the_answer();
    assert_eq!(
        with.started + 1,
        after.started,
        "the launch took the exit before it asked: {with:?}"
    );
    assert_eq!(with.task, "failed");
    assert_eq!(after.task, "failed");
}

#[tokio::test]
async fn an_answer_and_an_exit_written_together_are_one_read_and_the_exit_is_handled_first() {
    let reference = exit_with_the_answer();
    // The host's exit before its answer, and its answer before its exit:
    // either way both are handled before what the answer woke runs.
    for exit_first in [true, false] {
        scene(async {
            let (mut rig, open, pane) = zeus_opening().await;
            let (exit, answer) = (rig.pieces.host.exit(&pane), answer(&open));
            let bytes = if exit_first {
                [exit, answer].concat()
            } else {
                [answer, exit].concat()
            };
            rig.pieces.host.write(&bytes).await;
            let (task, events, started) = finished(&mut rig).await;
            assert_eq!(started, reference.started, "no `started` for a window gone");
            assert_eq!(task, reference.task);
            assert_eq!(events, reference.events);
        })
        .await;
    }
}

#[tokio::test]
async fn an_answer_and_an_exit_in_two_reads_ready_at_once_are_two_callbacks_the_answers_work_first()
{
    let reference = exit_after_the_answer();
    scene(async {
        let (mut rig, open, pane) = zeus_opening().await;
        // The first read is answers only: the host's answer, and another that
        // nothing waits for, which fills the read to the byte. The exit is the
        // second read, already waiting when the first has been handled.
        let answered = answer(&open);
        let mut bytes = answered.clone();
        bytes.extend(padding(READ_BYTES - answered.len()));
        bytes.extend(rig.pieces.host.exit(&pane));
        rig.pieces.host.write(&bytes).await;
        let (task, events, started) = finished(&mut rig).await;
        assert_eq!(
            started, reference.started,
            "`started` was asked, then the exit came"
        );
        assert_eq!(task, reference.task);
        assert_eq!(events, reference.events);
    })
    .await;
}

#[tokio::test]
async fn a_frame_in_pieces_is_handled_when_it_is_whole_and_a_read_with_no_frame_changes_nothing() {
    let (together, apart) = (exit_with_the_answer(), exit_after_the_answer());
    // The answer's first half, a read that holds no whole frame; then the
    // rest of it with the exit, which is one read again.
    scene(async {
        let (mut rig, open, pane) = zeus_opening().await;
        let answered = answer(&open);
        let (first, rest) = answered.split_at(answered.len() / 2);
        let before = rig.events();
        rig.pieces.host.write(first).await;
        let (_, events, _) = finished(&mut rig).await;
        assert_eq!(events, before, "half a frame woke nothing");
        let exit = rig.pieces.host.exit(&pane);
        rig.pieces.host.write(&[rest, &exit].concat()).await;
        let (task, events, started) = finished(&mut rig).await;
        assert_eq!((started, task), (together.started, together.task));
        assert_eq!(events, together.events);
    })
    .await;
    // The rest of the answer fills a read with answers only, which the first
    // piece had begun, and the exit is the read after it, already waiting.
    scene(async {
        let (mut rig, open, pane) = zeus_opening().await;
        let answered = answer(&open);
        let (first, rest) = answered.split_at(answered.len() / 2);
        rig.pieces.host.write(first).await;
        settle().await;
        let mut bytes = rest.to_vec();
        bytes.extend(padding(READ_BYTES - rest.len()));
        bytes.extend(rig.pieces.host.exit(&pane));
        rig.pieces.host.write(&bytes).await;
        let (task, events, started) = finished(&mut rig).await;
        assert_eq!((started, task), (apart.started, apart.task));
        assert_eq!(events, apart.events);
    })
    .await;
}
