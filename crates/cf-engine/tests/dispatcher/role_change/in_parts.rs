//! T-1513 (review T-1510): a role whose note `cf inbox read` cannot print within the bound
//! a role is read whole at (`cf_base::role_read`, 29,000 bytes and 500 lines) is printed in
//! parts, each within it, and is told once every part was printed. Where it starts: specs on, the most rules for the staff and a staff of 8 of
//! two roles a row (6 since T-1513 put the ban on a task for a member by name back). The
//! review's case is the chief's real text, which the fake role texts carry here as words a
//! release added to them. The window's `cf` is read the way `cf` reads it: each part
//! printed as `cf` prints it (`printed_part`), said read only where it is read whole, which
//! is all `cf` says to the board.

use cf_base::env::Env;
use cf_base::role_read::{printed_part, role_read_whole};
use cf_engine::roles::{role_instructions, with_staff_rules, ProjectSpecs, StaffRow};
use cf_engine::testing::Context;
use cf_ledger::{Read, STAFF_RULES_MAX_BYTES, STAFF_RULES_MAX_LINES};
use cf_proto::agents::WorkTier;
use cf_proto::cf_word::{CfWord, Shell};
use cf_proto::specs::Variant;

use super::{codex, project_of, recorded, relaunch, replacements, said_to_the_chief};
use crate::chiefs::to_human;

/// The most rules for the staff the ledger takes: every byte and every line.
fn the_most_rules() -> String {
    let lines = STAFF_RULES_MAX_LINES;
    let each = (STAFF_RULES_MAX_BYTES - (lines - 1)) / lines;
    let mut rules = vec!["a".repeat(each); lines].join("\n");
    rules += &"a".repeat(STAFF_RULES_MAX_BYTES - rules.len());
    rules
}

/// The windows of `context` start their commands with the `cf` of a Mac's app, by its whole
/// path, as a real window's text does: the sizes the review measured.
fn on_a_mac(context: &Context) {
    context.roles.cf.replace(CfWord::new(
        "/Applications/ConsensFlow.app/Contents/Resources/bin/cf",
        Shell::Posix,
    ));
}

/// The chief's real text with specs on, the most rules for the staff and a staff of `rows`
/// who each hold two roles, in the window whose `cf` is the test's.
fn the_reviews_role(context: &Context, rows: usize) -> String {
    let staff: Vec<StaffRow> = (0..rows)
        .map(|at| StaffRow {
            name: format!("gpt-6-1-sol-xhigh-{at}"),
            model: None,
            roles: vec!["worker".to_owned(), "reviewer".to_owned()],
            work_tier: Some(WorkTier::Standard),
        })
        .collect();
    let specs = ProjectSpecs {
        variant: Variant::TddHtml,
        folder: "docs/specs",
        skill: "/Users/someone/.consensflow/integrations/specmint/codex/tdd-html/SKILL.md",
    };
    let cf = context.roles.cf.borrow().clone();
    let text = role_instructions(&Env::default(), "chief", &staff, &cf, Some(&specs)).unwrap();
    with_staff_rules(text, "chief", Some(&the_most_rules()))
}

/// The parts `cf inbox read` prints the note `id` in, read now.
fn parts_of(context: &Context, id: i64) -> usize {
    let note = context.message(id);
    let cf = context.roles.cf.borrow().clone();
    printed_part("m", &cf.in_text(&note.body), 1, &cf.to_string(), id)
        .unwrap()
        .1
}

/// The chief's window runs `cf inbox read m-N --part K` for each part of `order` (the first
/// is 1): what `cf` prints of it, and what it says to the board where that is read whole.
/// How many messages the ledger received.
fn read_like_cf(context: &Context, project: i64, id: i64, order: &[usize]) -> usize {
    let chief = context.id(project, "chief");
    let cf = context.roles.cf.borrow().clone();
    let mut received = 0;
    for &part in order {
        let note = context.message(id);
        let line = format!("m-{id} [{}] note from ConsensFlow", note.state);
        let (text, of) =
            printed_part(&line, &cf.in_text(&note.body), part, &cf.to_string(), id).unwrap();
        let printed = format!("{text}\n");
        assert!(
            role_read_whole(&printed),
            "part {part} of {of} is printed past the bound: {} bytes, {} lines",
            printed.len(),
            printed.matches('\n').count()
        );
        let mut ledger = context.ledger.borrow_mut();
        received += if of == 1 {
            ledger.receive_read(chief, &[id], Read::Inbox, &cf)
        } else {
            ledger.receive_read_part(chief, id, part, of, Read::Inbox, &cf)
        }
        .unwrap();
    }
    received
}

/// The notes ConsensFlow wrote to the human that say a role was not read.
fn not_read(context: &Context, project: i64) -> Vec<String> {
    to_human(context, project)
        .into_iter()
        .filter(|body| body.contains("did not read its new role"))
        .collect()
}

/// A release that rewrote the chief's text to the review's, at a staff of `rows`, and a
/// Resume: the note that says it, delivered (`confirmed`) or on its way.
fn the_reviews_role_told(context: &Context, project: i64, rows: usize, confirmed: bool) -> i64 {
    let chief = context.id(project, "chief");
    on_a_mac(context);
    let role = the_reviews_role(context, rows);
    context.roles.rewritten.replace(format!("\n\n{role}"));
    relaunch(context, project);
    context.pass().unwrap();
    if confirmed {
        context.pass().unwrap();
    }
    let notes = replacements(context, chief);
    assert_eq!(notes.len(), 1);
    let id = notes[0].id;
    assert_eq!(
        context.message(id).state,
        if confirmed { "delivered" } else { "delivering" }
    );
    id
}

/// The chief answers, and the human resumes again: how many replacements the chief was
/// given in all.
fn resumed_again(context: &Context, project: i64) -> usize {
    context.codex.answer("chief", "Understood.");
    context.pass().unwrap();
    relaunch(context, project);
    context.pass().unwrap();
    context.pass().unwrap();
    replacements(context, context.id(project, "chief")).len()
}

#[test]
fn the_reviews_role_read_in_every_part_is_told_and_said_nothing_of_at_the_next_resume() {
    for rows in [8, 12, 20] {
        let context = codex();
        let project = project_of(&context, "codex");
        let chief = context.id(project.id, "chief");
        let id = the_reviews_role_told(&context, project.id, rows, true);
        let of = parts_of(&context, id);
        assert!(of > 1, "{rows} rows: over the bound, in parts");
        let order: Vec<usize> = (1..=of).collect();
        let received = read_like_cf(&context, project.id, id, &order);
        assert_eq!(
            resumed_again(&context, project.id),
            1,
            "{rows} rows: the role is what it read: nothing is said again"
        );
        assert!(not_read(&context, project.id).is_empty(), "{rows} rows");
        assert_eq!(received, 1, "{rows} rows");
        assert_eq!(context.message(id).state, "read", "{rows} rows");
        assert!(
            recorded(&context, chief)
                .is_some_and(|told| told.ends_with(&the_reviews_role(&context, rows))),
            "{rows} rows: the role it read is what it was told"
        );
        context.close();
    }
}

#[test]
fn the_opening_of_a_role_read_in_parts_says_how_many_and_the_command_that_prints_the_first() {
    let context = codex();
    let project = project_of(&context, "codex");
    let id = the_reviews_role_told(&context, project.id, 12, true);
    let of = parts_of(&context, id);
    let said = said_to_the_chief(&context);
    assert_eq!(said.len(), 1);
    assert!(
        said[0].contains(&format!(
            "prints them in {of} parts: the first, which ends with the command that prints the next."
        )),
        "{}",
        &said[0][..900]
    );
    assert!(
        said[0].ends_with(&format!(
            "read all of it with: {} inbox read m-{id})",
            context.roles.cf.borrow()
        )),
        "{}",
        &said[0][said[0].len() - 200..]
    );
    context.close();
}

#[test]
fn parts_read_while_the_paste_is_on_its_way_tell_the_role_once_it_is_confirmed_and_only_all_of_them(
) {
    // Every part read before the daemon saw the paste arrive; the first before, the rest
    // after; and the first alone, which tells nothing: the role is said again at the next
    // Resume, and the human is told.
    for (before, after, told) in [
        (vec![1, 2], vec![], true),
        (vec![1], vec![2], true),
        (vec![1], vec![], false),
    ] {
        let context = codex();
        let project = project_of(&context, "codex");
        let chief = context.id(project.id, "chief");
        let id = the_reviews_role_told(&context, project.id, 12, false);
        assert_eq!(parts_of(&context, id), 2);
        assert_eq!(read_like_cf(&context, project.id, id, &before), 0);
        assert_eq!(context.message(id).state, "delivering");
        context.pass().unwrap();
        context.pass().unwrap();
        read_like_cf(&context, project.id, id, &after);
        let case = format!("read {before:?} then {after:?}");
        assert_eq!(
            context.message(id).state,
            if told { "read" } else { "delivered" },
            "{case}"
        );
        assert_eq!(
            recorded(&context, chief)
                .is_some_and(|text| text.ends_with(&the_reviews_role(&context, 12))),
            told,
            "{case}"
        );
        assert_eq!(
            resumed_again(&context, project.id),
            if told { 1 } else { 2 },
            "{case}"
        );
        assert_eq!(
            not_read(&context, project.id).len(),
            usize::from(!told),
            "{case}"
        );
        context.close();
    }
}

/// T-1513 (review T-1516): the role is read once every part of it was printed, in any
/// order: a part printed twice is no more read, and the first part printed after the last
/// reads it as the last after the first would.
#[test]
fn parts_read_out_of_order_or_twice_tell_the_role_once_every_part_was_printed() {
    let context = codex();
    let project = project_of(&context, "codex");
    let id = the_reviews_role_told(&context, project.id, 20, true);
    assert_eq!(parts_of(&context, id), 2);
    assert_eq!(
        read_like_cf(&context, project.id, id, &[2, 2]),
        0,
        "the last first, twice"
    );
    assert_eq!(context.message(id).state, "delivered");
    assert_eq!(
        read_like_cf(&context, project.id, id, &[1]),
        1,
        "then the first"
    );
    assert_eq!(context.message(id).state, "read");
    assert_eq!(
        read_like_cf(&context, project.id, id, &[1, 2]),
        0,
        "read once"
    );
    assert_eq!(resumed_again(&context, project.id), 1);
    context.close();
}

/// Muse Code's `/clear` begins a session no hook greets: the engine writes it its role, and
/// a chief's over the bound is read in parts and told once the last is read, as a resumed
/// Codex thread's is.
#[test]
fn a_session_no_hook_greeted_is_told_a_role_over_the_bound_once_it_read_every_part() {
    let context = Context::new();
    on_a_mac(&context);
    let role = the_reviews_role(&context, 12);
    context.roles.rewritten.replace(format!("\n\n{role}"));
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context.adapter.switch_to("chief", "native-cleared");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let note = context
        .inbox(chief)
        .into_iter()
        .find(|message| message.body.contains("began without your ConsensFlow role"))
        .expect("the greeting");
    assert_eq!(context.message(note.id).state, "delivered");
    let of = parts_of(&context, note.id);
    assert!(of > 1);
    assert!(note.body.contains(&format!("prints them in {of} parts")));
    let order: Vec<usize> = (1..=of).collect();
    assert_eq!(read_like_cf(&context, project.id, note.id, &order), 1);
    assert!(recorded(&context, chief).is_some_and(|told| told.ends_with(&role)));
    assert!(to_human(&context, project.id).is_empty());
}
