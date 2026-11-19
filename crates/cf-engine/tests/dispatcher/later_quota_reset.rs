//! Two windows report different limits on one account.
use super::fixtures::{now, soon, Tiers};
use cf_engine::testing::Context;
use cf_harness::records::Quota;

fn passes(c: &Context, n: usize) {
    for _ in 0..n {
        c.pass().unwrap();
    }
}

fn refuse(c: &Context, window: &str, reset: &str) {
    c.adapter.refuse(
        window,
        Quota::Exhausted {
            at: Some(now(c)),
            resets_at: Some(reset.to_owned()),
        },
    );
}

#[test]
fn a_weekly_limit_reported_after_the_five_hour_limit_does_not_resume_work_early() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    c.give(t.project.id, "zeus", "Another window");
    passes(&c, 5);
    let first = t.assignee(1);
    let second = t.assignee(2);
    assert_ne!(first, second, "two windows of one member");
    c.advance(1000);
    let five_hours = soon(&c, 5);
    refuse(&c, &first, &five_hours);
    passes(&c, 4);
    c.advance(1000);
    let weekly = soon(&c, 72);
    refuse(&c, &second, &weekly);
    passes(&c, 4);
    eprintln!(
        "Two limits: after both refusals zeus is out until {:?} (five-hour {five_hours}, weekly {weekly}); T-1 {} T-2 {}",
        t.out_until("zeus"),
        t.task(1).task.state,
        t.task(2).task.state
    );
    c.advance(5 * 3_600_000);
    passes(&c, 6);
    let states = [t.task(1).task.state, t.task(2).task.state];
    eprintln!(
        "Two limits: just after the five-hour reset: T-1 {} T-2 {}; out until {:?}",
        states[0],
        states[1],
        t.out_until("zeus")
    );
    // The account is still out for the week: a window given its task again is refused at once.
    c.advance(1000);
    for (window, state) in [(&first, &states[0]), (&second, &states[1])] {
        if state == "working" {
            refuse(&c, window, &weekly);
        }
    }
    passes(&c, 6);
    let human = t.notes("human");
    let chief = t.notes("chief");
    eprintln!("Two limits: human notes {human:#?}");
    eprintln!("Two limits: chief notes {chief:#?}");
    eprintln!(
        "Two limits: at the end zeus is out until {:?}; T-1 {} T-2 {}",
        t.out_until("zeus"),
        t.task(1).task.state,
        t.task(2).task.state
    );
    let may_need = human.iter().filter(|n| n.contains("may need you")).count();
    assert_eq!(
        may_need, 0,
        "the human was told the account may need them: {human:#?}"
    );
    assert_eq!(t.out_until("zeus").as_deref(), Some(weekly.as_str()));
    assert_eq!(
        states,
        ["paused", "paused"],
        "neither task resumes at five hours"
    );
}
