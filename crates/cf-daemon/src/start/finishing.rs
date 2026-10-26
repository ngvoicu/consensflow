//! What the agents' API asks of the engine, which is built after the API (the windows it
//! opens are told where the API is): the question waits for the engine, and is answered
//! that the daemon is starting where it is asked before. It runs on the engine's executor,
//! begun where the request is read, as a page operation does.

use std::cell::OnceCell;
use std::rc::Rc;

use cf_base::refusal::Refusal;
use cf_engine::runtime::begin;
use cf_engine::Dispatcher;

use crate::api::context::Finishing;
use crate::seams::DaemonSpawn;

/// The API's finishing of a task (`cf task finish`): the engine's answer once `engine`
/// holds it.
pub(super) fn of(engine: &Rc<OnceCell<Rc<Dispatcher>>>, spawn: &Rc<DaemonSpawn>) -> Rc<Finishing> {
    let (engine, spawn) = (Rc::clone(engine), Rc::clone(spawn));
    Rc::new(move |project, number, by| {
        let (engine, spawn) = (engine.get().cloned(), Rc::clone(&spawn));
        Box::pin(async move {
            let Some(engine) = engine else {
                return Err(Refusal::with_status(
                    "starting",
                    "the daemon is starting: try again in a moment",
                    503,
                ));
            };
            let begun = begin(&*spawn, async move {
                engine.finish_task(project, number, &by).await
            })
            .await;
            begun.await.map_err(|failed| failed.refusal())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::worked;

    #[tokio::test]
    async fn a_finish_asked_before_the_engine_exists_is_told_the_daemon_is_starting() {
        let engine = Rc::new(OnceCell::new());
        let local = tokio::task::LocalSet::new();
        let refused = local
            .run_until(async {
                let worked = worked();
                of(&engine, &worked.spawn)(1, 1, "chief".to_owned()).await
            })
            .await
            .unwrap_err();
        assert_eq!(
            (refused.code, refused.status, refused.message.as_str()),
            (
                "starting",
                503,
                "the daemon is starting: try again in a moment"
            )
        );
    }
}
