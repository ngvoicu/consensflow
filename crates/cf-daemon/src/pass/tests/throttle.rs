//! The throttle on the paused clock: an event told at most once in a wait,
//! from the first call, whatever came during it.

use std::cell::Cell;

use tokio::time::sleep;

use super::*;

fn counted() -> (Rc<Cell<u32>>, impl Fn() + 'static) {
    let seen = Rc::new(Cell::new(0));
    let count = Rc::clone(&seen);
    (seen, move || count.set(count.get() + 1))
}

#[tokio::test(start_paused = true)]
async fn the_throttle_tells_once_a_wait_after_the_first_call_whatever_came_between() {
    scene(async {
        let (told, work) = counted();
        let throttled = throttle(Duration::from_millis(100), work);
        throttled();
        sleep(Duration::from_millis(50)).await;
        throttled();
        sleep(Duration::from_millis(49)).await;
        throttled();
        turns().await;
        assert_eq!(told.get(), 0, "nothing before the wait is over");
        sleep(Duration::from_millis(1)).await;
        turns().await;
        assert_eq!(
            told.get(),
            1,
            "at 100 ms from the first, not postponed by the others"
        );
        sleep(Duration::from_secs(5)).await;
        assert_eq!(
            told.get(),
            1,
            "the calls that came during the wait were that event"
        );
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn the_next_call_after_the_wait_begins_another_wait() {
    scene(async {
        let (told, work) = counted();
        let throttled = throttle(Duration::from_millis(100), work);
        throttled();
        sleep(Duration::from_millis(100)).await;
        turns().await;
        assert_eq!(told.get(), 1);
        sleep(Duration::from_millis(300)).await;
        throttled();
        sleep(Duration::from_millis(99)).await;
        turns().await;
        assert_eq!(told.get(), 1);
        sleep(Duration::from_millis(1)).await;
        turns().await;
        assert_eq!(told.get(), 2);
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn each_throttle_has_its_own_wait() {
    scene(async {
        let (core, work_core) = counted();
        let (transcript, work_transcript) = counted();
        let first = throttle(Duration::from_millis(100), work_core);
        let second = throttle(Duration::from_millis(100), work_transcript);
        first();
        sleep(Duration::from_millis(60)).await;
        second();
        sleep(Duration::from_millis(40)).await;
        turns().await;
        assert_eq!((core.get(), transcript.get()), (1, 0));
        sleep(Duration::from_millis(60)).await;
        turns().await;
        assert_eq!((core.get(), transcript.get()), (1, 1));
    })
    .await;
}
