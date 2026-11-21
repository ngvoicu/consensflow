//! The router (`architecture/staff-slots.md`, section 4): a task goes to its slot, else to the
//! nearest held slot of its role, the next one up before the next one down, but never into
//! Critical, which takes only a task added or edited there with its purpose. A task waits only
//! when no slot of its role is held outside Critical, and is told once the way on. This holds
//! for every role that has tiers: a task still waiting when the staff changes under it goes the
//! same way. Node's dispatcher had no such rule, so no trace holds these.

use cf_engine::testing::Context;
use cf_ledger::{NewTask, ProjectView};
use serde_json::{json, Value};

/// The tiers, from the one asked first to the one asked last.
const TIERS: [&str; 4] = ["critical", "complex", "standard", "light"];

/// The roles whose tasks have a tier, with the pool each is asked for.
const POOLS: [&str; 4] = ["worker", "advisor", "reviewer", "tester"];

/// Every role that has tiers, from the registry: the roles with a Critical slot hold work at
/// Critical, the three that write code do not.
fn tiered_roles() -> Vec<&'static cf_ledger::roles::Role> {
    cf_ledger::roles::all().iter().filter(|role| role.tiered()).collect()
}

/// A project at /work/app: apollo its chief, and `staff` as `(agent, role, tier)`.
fn project_of(context: &Context, staff: &[(&str, &str, &str)]) -> ProjectView {
    let members: Vec<Value> = staff
        .iter()
        .map(|(agent, role, tier)| {
            json!({ "agent": agent, "harness": "claude-code", "roles": [role], "tier": tier })
        })
        .collect();
    context
        .open_project_overflowing(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "model": "apollo" },
            "staff": members,
        }))
        .unwrap()
        .expect("the project")
}

/// The chief opens a task for `tier` of `pool`; the work of a critical tier names its purpose
/// (and `pool` must have a Critical slot to be asked there).
fn open(context: &Context, project: &ProjectView, pool: &str, tier: &str) -> i64 {
    context
        .create_task(
            project.id,
            NewTask {
                from: "chief".to_owned(),
                pool: Some(pool.to_owned()),
                tier: Some(tier.to_owned()),
                purpose: (tier == "critical").then(|| "hard-problem".to_owned()),
                body: "Do the work".to_owned(),
                ..NewTask::default()
            },
        )
        .task
        .number
}

/// The notes the chief was written that say a task's tier moved.
fn retiered(context: &Context, project: &ProjectView) -> Vec<String> {
    let chief = context.id(project.id, "chief");
    let mut said: Vec<String> = context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.kind == "note" && message.body.contains(" asked for a "))
        .map(|message| message.body)
        .collect();
    said.reverse();
    said
}

/// The notes the chief was written that say a task waits.
fn waits(context: &Context, project: &ProjectView) -> Vec<String> {
    let chief = context.id(project.id, "chief");
    let mut said: Vec<String> = context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.kind == "note" && message.body.contains(" waits for "))
        .map(|message| message.body)
        .collect();
    said.reverse();
    said
}

#[test]
fn a_task_goes_to_its_slot_else_the_nearest_held_one_never_into_critical_for_every_role() {
    for role in tiered_roles() {
        let pool = role.id;
        for held in role.tiers {
            for asked in role.tiers.iter().filter(|asked| *asked != held) {
                let context = Context::new();
                let project = project_of(&context, &[("zeus", pool, held)]);
                let number = open(&context, &project, pool, asked);
                for _ in 0..3 {
                    context.pass().unwrap();
                }
                let task = context.task(project.id, number).task;
                // Where the one slot held is Critical, no task falls into it: a task asked
                // there with its purpose is its own, and every other waits where it asked.
                let into_critical = *held == "critical";
                if into_critical && *asked != "critical" {
                    assert_eq!(
                        (task.tier.as_deref(), task.assignee.is_some(), task.state.as_str()),
                        (Some(*asked), false, "open"),
                        "a {asked} {pool} task waits: only the critical slot is held, which takes only critical work"
                    );
                    assert_eq!(
                        waits(&context, &project).len(),
                        1,
                        "told once the way on: a {asked} {pool} task"
                    );
                } else {
                    assert_eq!(
                        (task.tier.as_deref(), task.assignee.is_some()),
                        (Some(*held), true),
                        "a {asked} {pool} task goes to the {held} {pool} the staff has"
                    );
                    assert_eq!(
                        waits(&context, &project),
                        Vec::<String>::new(),
                        "a {asked} {pool} task is told nothing of a wait when the {held} one is free"
                    );
                    // The add's answer told whose slot it fell to (`asked`, `heldBy`): the
                    // pass writes no second note.
                    assert_eq!(retiered(&context, &project).len(), 0);
                }
            }
        }
    }
}

#[test]
fn a_critical_task_with_its_purpose_with_nobody_at_critical_falls_to_the_nearest_tier_and_keeps_its_purpose()
{
    let context = Context::new();
    let project = project_of(
        &context,
        &[("zeus", "advisor", "light"), ("hera", "advisor", "standard")],
    );
    let number = open(&context, &project, "advisor", "critical");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let task = context.task(project.id, number).task;
    assert_eq!(
        (task.tier.as_deref(), task.purpose.as_deref(), task.assignee.is_some()),
        (Some("standard"), Some("hard-problem"), true),
        "the nearest tier held, never the light one past it"
    );
}

#[test]
fn only_critical_held_waits_a_task_and_says_the_way_on_once_and_the_same_task_goes_once_a_slot_is_filled() {
    let context = Context::new();
    let project = project_of(&context, &[("zeus", "reviewer", "critical")]);
    let number = open(&context, &project, "reviewer", "standard");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(
        waits(&context, &project),
        [format!(
            "T-{number} waits for a standard reviewer: no reviewer slot is held but the critical one, which takes only critical work. To go on: if it is critical work, cf task edit T-{number} --tier critical --purpose <purpose> (@zeus holds it); else ask the human for a reviewer, in your terminal."
        )],
        "told once, in the registry's words"
    );
    // A member of a slot it may take comes: the same task goes to it.
    context
        .ledger
        .borrow_mut()
        .add_member(
            project.id,
            &cf_ledger::NewMember {
                model: None,
                effort: None,
                model_name: Some("hera".to_owned()),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["reviewer".to_owned()],
                tier: "light".to_owned(),
            },
        )
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let task = context.task(project.id, number).task;
    assert_eq!(
        (task.tier.as_deref(), task.assignee.is_some()),
        (Some("light"), true),
        "the same task, now at the nearest held slot outside Critical"
    );
}

#[test]
fn the_edit_that_sends_it_to_critical_with_its_purpose_gives_it_to_the_critical_holder() {
    let context = Context::new();
    let project = project_of(&context, &[("zeus", "reviewer", "critical")]);
    let number = open(&context, &project, "reviewer", "standard");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert!(context.task(project.id, number).task.assignee.is_none());
    context
        .ledger
        .borrow_mut()
        .edit_task_with(
            project.id,
            number,
            "chief",
            &cf_ledger::TaskEdit {
                tier: Some("critical".to_owned()),
                purpose: Some("critical-review".to_owned()),
                ..cf_ledger::TaskEdit::default()
            },
        )
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let task = context.task(project.id, number).task;
    assert_eq!(
        (task.tier.as_deref(), task.purpose.as_deref(), task.assignee.is_some()),
        (Some("critical"), Some("critical-review"), true)
    );
}

#[test]
fn a_task_asked_at_critical_that_lost_its_purpose_is_never_given_to_the_critical_holder() {
    // A task from before, at Critical with no purpose (main's fall put it there), behind a need,
    // beside a Critical advisor alone: it may not take that slot, waits, and goes to a
    // standard advisor once one joins.
    let context = Context::new();
    let project = project_of(&context, &[("zeus", "advisor", "critical")]);
    let first = open(&context, &project, "advisor", "critical");
    let behind = open(&context, &project, "advisor", "light");
    context
        .ledger
        .borrow()
        .sqlite()
        .execute(
            "UPDATE task SET tier = 'critical', purpose = NULL WHERE number = ?",
            [behind],
        )
        .unwrap();
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let kept = context.task(project.id, behind).task;
    assert_eq!(
        (kept.tier.as_deref(), kept.assignee.is_some(), kept.state.as_str()),
        (Some("critical"), false, "open")
    );
    assert!(context.task(project.id, first).task.assignee.is_some());
    assert_eq!(waits(&context, &project).len(), 1, "told once");
    context
        .ledger
        .borrow_mut()
        .add_member(
            project.id,
            &cf_ledger::NewMember {
                model: None,
                effort: None,
                model_name: Some("hera".to_owned()),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["advisor".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let moved = context.task(project.id, behind).task;
    assert_eq!(
        (moved.tier.as_deref(), moved.assignee.is_some()),
        (Some("standard"), true),
        "the same task goes to the standard advisor once one joins"
    );
}

#[test]
fn in_a_slot_three_hold_all_three_take_work() {
    let context = Context::new();
    let project = project_of(
        &context,
        &[
            ("zeus", "worker", "standard"),
            ("hera", "worker", "standard"),
            ("ares", "worker", "standard"),
        ],
    );
    let numbers: Vec<i64> = (0..3)
        .map(|_| open(&context, &project, "worker", "standard"))
        .collect();
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let mut takers: Vec<String> = numbers
        .iter()
        .map(|number| {
            let task = context.task(project.id, *number).task;
            task.assignee.expect("taken")
        })
        .map(|window| window.split('-').next().unwrap_or_default().to_owned())
        .collect();
    takers.sort();
    takers.dedup();
    assert_eq!(takers.len(), 3, "each member took one: {takers:?}");
}

#[test]
fn a_task_open_when_the_last_member_of_its_tier_moves_to_another_tier_goes_to_the_nearest() {
    for pool in POOLS {
        let context = Context::new();
        let project = project_of(&context, &[("zeus", pool, "complex")]);
        let number = open(&context, &project, pool, "complex");
        // The human moves zeus to another tier in Edit member: the tiers no longer follow a
        // saved agent's (harness-models).
        let zeus = project
            .participants
            .iter()
            .find(|participant| participant.role != "chief" && participant.role != "human")
            .expect("the member")
            .handle
            .clone();
        context
            .ledger
            .borrow_mut()
            .edit_member(
                project.id,
                &zeus,
                &cf_ledger::MemberEdit {
                    harness: "claude-code".to_owned(),
                    model: None,
                    effort: None,
                    model_name: Some("zeus".to_owned()),
                    tier: "standard".to_owned(),
                },
            )
            .unwrap();
        context.pass().unwrap();
        context.pass().unwrap();
        let task = context.task(project.id, number).task;
        assert_eq!(
            (task.tier.as_deref(), task.assignee.is_some()),
            (Some("standard"), true),
            "the {pool} task waits for no tier nobody holds"
        );
        assert_eq!(waits(&context, &project), Vec::<String>::new());
    }
}

#[test]
fn a_task_open_when_the_last_member_of_its_tier_leaves_goes_to_the_nearest() {
    for pool in POOLS {
        let context = Context::new();
        let project = project_of(
            &context,
            &[("zeus", pool, "complex"), ("hera", pool, "standard")],
        );
        let number = open(&context, &project, pool, "complex");
        context
            .ledger
            .borrow_mut()
            .remove_member(project.id, "zeus")
            .unwrap();
        context.pass().unwrap();
        context.pass().unwrap();
        let task = context.task(project.id, number).task;
        assert_eq!(
            (task.tier.as_deref(), task.assignee.is_some()),
            (Some("standard"), true),
            "the {pool} task waits for no tier nobody holds"
        );
        assert_eq!(waits(&context, &project), Vec::<String>::new());
    }
}

#[test]
fn the_requester_is_told_once_which_tier_took_a_task_that_lost_the_one_it_asked() {
    let context = Context::new();
    let project = project_of(
        &context,
        &[
            ("zeus", "tester", "complex"),
            ("anubis", "tester", "standard"),
        ],
    );
    let number = open(&context, &project, "tester", "complex");
    context
        .ledger
        .borrow_mut()
        .remove_member(project.id, "zeus")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(
        context.task(project.id, number).task.assignee.is_some(),
        "the standard tester took it"
    );
    assert_eq!(
        retiered(&context, &project),
        ["T-1 asked for a complex tester; none is on the staff, so it goes to the standard tier: @anubis."],
        "told once, with the member of the tier that took it"
    );
}

#[test]
fn a_task_for_a_role_nobody_holds_any_more_waits_and_says_so() {
    for pool in POOLS {
        let context = Context::new();
        let project = project_of(&context, &[("zeus", pool, "standard")]);
        let number = open(&context, &project, pool, "standard");
        context
            .ledger
            .borrow_mut()
            .remove_member(project.id, "zeus")
            .unwrap();
        for _ in 0..3 {
            context.pass().unwrap();
        }
        assert_eq!(
            waits(&context, &project),
            [format!(
                "T-{number} waits for a free standard {pool}: no {pool} is on the staff (ask the human for one, in your terminal)."
            )],
            "a {pool} task says why it waits, once"
        );
    }
}

#[test]
fn the_nearest_tier_is_the_next_one_up_before_the_next_one_down_and_never_critical() {
    // (held, asked, goes to): a tie goes up, and the nearer one wins over the one above;
    // Critical is never a destination.
    for (held, asked, goes_to) in [
        (["complex", "light"], "standard", "complex"),
        (["complex", "light"], "critical", "complex"),
        (["standard", "light"], "complex", "standard"),
        (["critical", "light"], "standard", "light"),
        (["critical", "complex"], "light", "complex"),
        (["standard", "light"], "critical", "standard"),
    ] {
        let context = Context::new();
        let project = project_of(
            &context,
            &[("zeus", "advisor", held[0]), ("hera", "advisor", held[1])],
        );
        let number = open(&context, &project, "advisor", asked);
        for _ in 0..2 {
            context.pass().unwrap();
        }
        let task = context.task(project.id, number).task;
        assert_eq!(
            task.tier.as_deref(),
            Some(goes_to),
            "{asked} with {held:?} held goes to {goes_to}"
        );
    }
}

#[test]
fn a_task_is_never_refused_for_want_of_a_member_for_every_pool() {
    for pool in ["worker", "advisor", "reviewer", "tester", "designer"] {
        let context = Context::new();
        // A staff that holds every role but this one.
        let others: Vec<(&str, &str, &str)> = [
            ("w", "worker"),
            ("a", "advisor"),
            ("r", "reviewer"),
            ("t", "tester"),
        ]
        .into_iter()
        .filter(|(_, role)| *role != pool)
        .map(|(agent, role)| (agent, role, "standard"))
        .collect();
        let project = project_of(&context, &others);
        let made = context
            .ledger
            .borrow_mut()
            .create_task(
                project.id,
                &NewTask {
                    from: "chief".to_owned(),
                    pool: Some(pool.to_owned()),
                    tier: (pool != "designer").then(|| "standard".to_owned()),
                    body: "Do the work".to_owned(),
                    ..NewTask::default()
                },
            )
            .expect("a task is not refused for want of a member");
        assert_eq!(made.task.state, "open", "{pool}");
        for _ in 0..3 {
            context.pass().unwrap();
        }
        let role = if pool == "designer" { "image designer" } else { pool };
        let tier = if pool == "designer" { "" } else { "standard " };
        assert_eq!(
            waits(&context, &project),
            [format!(
                "T-1 waits for a free {tier}{role}: no {role} is on the staff (ask the human for one, in your terminal)."
            )],
            "{pool}: told once the way on"
        );
    }
}

#[test]
fn a_task_for_a_tier_somebody_holds_is_never_moved_though_that_member_is_out_of_quota() {
    let context = Context::new();
    let project = project_of(
        &context,
        &[
            ("zeus", "worker", "complex"),
            ("hera", "worker", "standard"),
        ],
    );
    let number = open(&context, &project, "worker", "complex");
    let until = crate::fixtures::soon(&context, 3);
    let zeus = context.id(project.id, "zeus");
    context
        .ledger
        .borrow_mut()
        .mark_out(zeus, &until, "out of quota")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let task = context.task(project.id, number).task;
    assert_eq!(
        (task.tier.as_deref(), task.assignee.is_some()),
        (Some("complex"), false),
        "it waits for zeus, who holds the tier, and hera's is another"
    );
    assert_eq!(retiered(&context, &project), Vec::<String>::new());
    assert_eq!(
        waits(&context, &project),
        [format!(
            "T-{number} waits for a free complex worker: @zeus is out of quota until {until}. If the quota was renewed early, the human presses Try again now on @zeus."
        )],
        "and says why"
    );
}
