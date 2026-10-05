//! Two operations in one read of the bridge, as Node ran them: each frame's
//! handler begins its engine work where its frame is read, in the order the
//! frames came, and what each did after its first wait goes on, in the order
//! the waits ended, once the read's frames are all handled. The real
//! operations (`session.open` and `session.hide`) over a bridge, with an
//! engine whose work says each of its steps, a turn apart.

use super::scripted::Scripted;
use super::*;

#[tokio::test]
async fn the_work_of_two_operations_in_one_read_interleaves_as_node_interleaved_it() {
    LocalSet::new()
        .run_until(async {
            let engine = Scripted::new();
            let steps = Rc::clone(&engine.steps);
            let rig = rig_over(Env::default(), Some(Rc::new(engine)));
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            // A frame is queued where the request is made: the first two are
            // one read. The third is too long to be in it: it is the next read.
            let timeout = Some(Duration::from_secs(5));
            let first = app.request(
                "session.open",
                json!({ "project": 1, "handle": "a" }),
                timeout,
            );
            let second = app.request(
                "session.hide",
                json!({ "project": 1, "handle": "b" }),
                timeout,
            );
            let padding = "x".repeat(70_000);
            let third = app.request(
                "session.open",
                json!({ "project": 1, "handle": "c", "padding": padding }),
                timeout,
            );
            let (first, second, third) = futures_util::future::join3(first, second, third).await;
            let said = json!({ "ok": true, "project": null });
            for answer in [first, second, third] {
                assert_eq!(answer.unwrap(), said);
            }
            // The first two began where their frames were read, one after the
            // other, and then took their turns together, not one and then the
            // other; and all of that was over before the third was read.
            assert_eq!(
                *steps.borrow(),
                [
                    "open a 0", "hide b 0", "open a 1", "hide b 1", "open a 2", "hide b 2",
                    "open c 0", "open c 1", "open c 2"
                ]
            );
            assert_eq!(
                rig.kicks.get(),
                3,
                "each woke the dispatcher when it was done"
            );
        })
        .await;
}
