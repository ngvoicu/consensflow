//! An agents file that cannot be read: nobody counts as free for new work and
//! nobody's agent as gone, so no work is taken back for a typo.

use cf_engine::testing::Context;

use crate::fixtures::{assert_match, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["an agents file that cannot be read"];

#[test]
fn gives_out_no_new_work_and_takes_none_back_until_it_can_and_every_window_goes_on() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.roster.broken.set(true);
    tiers.open_body("Write the docs");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "open"],
        "nothing taken back, nothing given out"
    );
    assert_match(
        tiers.notes("chief").last().unwrap(),
        r"^T-2 waits for a free standard worker: @zeus's agent cannot be read \(your agents file needs fixing: see Agents\)",
    );
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done", "the windows went on");
    context.roster.broken.set(false);
    context.pass().unwrap();
    assert_ne!(tiers.task(2).task.assignee, None);
    held_to(
        context.close(),
        SUITES,
        "gives out no new work and takes none back until it can, and every window goes on",
    );
}
