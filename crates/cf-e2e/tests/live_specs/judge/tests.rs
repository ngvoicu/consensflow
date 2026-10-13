use serde_json::json;

use super::*;

const MARKDOWN: &str = "---
id: hello-files
title: Hello files
status: active
created: 2026-10-13
updated: 2026-10-13
priority: medium
tags: [demo]
---

# Hello files

## Overview

Two files, one after the other.

## Phase 1: Hello [in-progress]

- [x] [HELLO-01] Add `hello.txt` containing the word hello
- [ ] [HELLO-02] Check it

## Phase 2: Goodbye [pending]

- [ ] [HELLO-03] Add `goodbye.txt` containing the word goodbye <- current

## Acceptance Criteria

- [ ] Both files are there
";

const HTML: &str = r#"<!doctype html><html><head>
<script type="application/json" id="spec-meta">{"id":"hello-files","title":"Hello files","status":"active"}</script>
</head><body>
<details class="phase" open data-phase="1" data-status="in-progress">
  <summary>Phase 1</summary>
  <ul>
    <li class="task" id="task-HELLO-01" data-task="HELLO-01" data-status="completed"><span class="task__check"></span>Add hello.txt</li>
    <li class="task" id="task-HELLO-02" data-task="HELLO-02" data-status="pending">Check it</li>
  </ul>
</details>
<details class="phase" open data-phase="2" data-status="pending">
  <ul><li class="task" id="task-HELLO-03" data-task="HELLO-03" data-status="pending">Add goodbye.txt</li></ul>
</details>
<ul class="ac-list"><li class="ac-item" id="ac-1" data-ac="1" data-status="pending">Both files</li></ul>
</body></html>"#;

fn code(code: &str, done: bool) -> SpecTask {
    SpecTask {
        code: code.to_owned(),
        done,
    }
}

fn hello() -> Spec {
    Spec {
        id: "hello-files".to_owned(),
        phases: vec![
            Phase {
                number: 1,
                tasks: vec![code("HELLO-01", true), code("HELLO-02", false)],
            },
            Phase {
                number: 2,
                tasks: vec![code("HELLO-03", false)],
            },
        ],
    }
}

#[test]
fn a_markdown_spec_is_its_id_its_phases_and_the_tasks_under_each_with_what_is_checked() {
    assert_eq!(markdown(MARKDOWN).unwrap(), hello());
    assert_eq!(hello().progress(), (1, 3));
    // The acceptance criteria are no task of a phase: they have no code.
    assert_eq!(hello().tasks().count(), 3);
}

#[test]
fn the_tdd_variants_tasks_are_tests_and_implementations_whose_codes_name_the_pair() {
    let tdd = "---\nid: rate-limit\n---\n\n## Phase 1: Limit [in-progress]\n\n- [ ] [TEST-RL-01] Test the limit <- current\n- [x] [IMPL-RL-02] Implement it -> satisfies [TEST-RL-01]\n\n## Phase 2: Report [pending]\n\n- [ ] [TEST-RL-03] Test the report\n";
    let spec = markdown(tdd).unwrap();
    let codes: Vec<&str> = spec.tasks().map(|task| task.code.as_str()).collect();
    assert_eq!(codes, ["TEST-RL-01", "IMPL-RL-02", "TEST-RL-03"]);
    assert_eq!(spec.progress(), (1, 3));
    // A mention of another task on a line is no task of its own.
    assert_eq!(spec.phases[0].tasks.len(), 2);
}

#[test]
fn an_html_spec_is_the_same_spec_read_from_its_meta_its_phases_and_its_tasks() {
    assert_eq!(html(HTML).unwrap(), hello());
    assert_eq!(read("SPEC.html", HTML).unwrap(), hello());
    assert_eq!(read("SPEC.md", MARKDOWN).unwrap(), hello());
}

#[test]
fn a_file_that_is_no_spec_is_refused_in_words() {
    assert_eq!(
        markdown("# Notes\n\n## Phase 1: x [pending]\n").unwrap_err(),
        "the spec has no id in its frontmatter"
    );
    assert_eq!(
        html("<html></html>").unwrap_err(),
        "the spec has no spec-meta"
    );
    assert!(html(r#"<script id="spec-meta">{"title":"x"}</script>"#)
        .unwrap_err()
        .contains("no id"));
    assert!(html(r#"<script id="spec-meta">{broken</script>"#)
        .unwrap_err()
        .contains("no JSON"));
    // A spec with no phases is a spec read, which a run then judges too thin.
    assert!(markdown("---\nid: bare\n---\n").unwrap().phases.is_empty());
}

#[test]
fn a_spec_is_in_the_shape_of_its_variant_by_its_file_and_where_it_is_the_tdd_one_by_its_tasks() {
    let core = hello();
    in_the_shape_of("core", "SPEC.md", &core).unwrap();
    in_the_shape_of("core-html", "SPEC.html", &core).unwrap();
    assert_eq!(
        in_the_shape_of("core-html", "SPEC.md", &core).unwrap_err(),
        "core-html writes SPEC.html, and the chief wrote SPEC.md"
    );
    assert!(in_the_shape_of("core", "SPEC.html", &core).is_err());
    let tdd = Spec {
        id: "x".to_owned(),
        phases: vec![Phase {
            number: 1,
            tasks: vec![code("TEST-X-01", false), code("IMPL-X-02", false)],
        }],
    };
    in_the_shape_of("tdd", "SPEC.md", &tdd).unwrap();
    in_the_shape_of("tdd-html", "SPEC.html", &tdd).unwrap();
    // Plain tasks are no tests with their implementations.
    let failed = in_the_shape_of("tdd", "SPEC.md", &core).unwrap_err();
    assert!(failed.contains("HELLO-01, HELLO-02, HELLO-03"), "{failed}");
    // A test alone is not the pair.
    let alone = Spec {
        phases: vec![Phase {
            number: 1,
            tasks: vec![code("TEST-X-01", false)],
        }],
        ..tdd
    };
    assert!(in_the_shape_of("tdd", "SPEC.md", &alone).is_err());
}

#[test]
fn the_registry_names_a_spec_s_status_and_how_much_of_it_is_done() {
    let registry = "# Spec Registry

| ID | Title | Status | Priority | Progress | Updated |
|----|-------|--------|----------|----------|---------|
| other-spec | Another | paused | low | 0/4 | 2026-10-12 |
| hello-files | Hello files | active | medium | 1/3 (33%) | 2026-10-13 |
";
    assert_eq!(
        registered(registry, "hello-files"),
        Some(Registered {
            status: "active".to_owned(),
            done: 1,
            total: 3
        })
    );
    assert_eq!(registered(registry, "other-spec").unwrap().total, 4);
    assert_eq!(registered(registry, "missing"), None);
    // A row with no progress to read is not a row of it.
    assert_eq!(
        registered("| hello-files | t | active | p | soon | d |", "hello-files"),
        None
    );
}

/// The board as the daemon draws it: cards with the tasks each needs as the
/// number and the state, and no brief.
fn board() -> Value {
    let card = |number: i64, state: &str, requester: &str, title: &str, needs: &[i64]| {
        json!({
            "number": number, "state": state, "requester": requester, "title": title,
            "needs": needs.iter().map(|need| json!({ "number": need, "state": "accepted" })).collect::<Vec<_>>(),
        })
    };
    json!({
        "open": [card(4, "queued", "chief", "Add goodbye.txt", &[3])],
        "lanes": [
            { "participant": { "handle": "chief" }, "tasks": [] },
            { "participant": { "handle": "worker" }, "tasks": [
                card(1, "accepted", "chief", "Look at the folder", &[]),
                card(2, "accepted", "chief", "Add hello.txt", &[1]),
                card(3, "working", "chief", "Check hello.txt", &[2]),
            ] },
        ],
    })
}

fn briefs(number: i64) -> String {
    match number {
        2 => "Spec item HELLO-01: add hello.txt\nReply with exactly: DONE-HELLO-01".to_owned(),
        3 => "From the spec, HELLO-02: check it".to_owned(),
        4 => "HELLO-03 of hello-files: add goodbye.txt".to_owned(),
        _ => "Look at the folder".to_owned(),
    }
}

fn tasks() -> Vec<BoardTask> {
    board_tasks(&board(), briefs)
}

#[test]
fn the_tasks_of_a_board_are_its_open_ones_and_each_lane_s_by_number_with_what_they_need() {
    let tasks = tasks();
    let numbers: Vec<i64> = tasks.iter().map(|task| task.number).collect();
    assert_eq!(numbers, [1, 2, 3, 4]);
    assert_eq!(tasks[3].needs, [3]);
    assert_eq!(tasks[1].state, "accepted");
    assert_eq!(tasks[0].requester, "chief");
    assert_eq!(tasks[1].title, "Add hello.txt");
    assert!(tasks[1].body.contains("HELLO-01"));
    assert!(board_tasks(&json!({}), briefs).is_empty());
}

#[test]
fn a_task_is_the_chief_s_when_its_title_or_its_brief_names_the_spec_task_as_its_own_word() {
    let tasks = tasks();
    let made = from_the_spec(&hello(), &tasks);
    assert_eq!(
        made.iter()
            .map(|(phase, task)| (*phase, task.number))
            .collect::<Vec<_>>(),
        [(1, 2), (1, 3), (2, 4)]
    );
    // HELLO-01 is no part of HELLO-010.
    let mut other = tasks.clone();
    other[1].body = "HELLO-010 only".to_owned();
    other[1].title = "x".to_owned();
    assert!(from_the_spec(&hello(), &other)
        .iter()
        .all(|(_, task)| task.number != 2));
    // A task a member asked for is none of the chief's, and a task called off is none.
    let mut by_member = tasks.clone();
    by_member[1].requester = "worker".to_owned();
    by_member[2].state = "cancelled".to_owned();
    assert_eq!(
        from_the_spec(&hello(), &by_member)
            .iter()
            .map(|(_, task)| task.number)
            .collect::<Vec<_>>(),
        [4]
    );
    // One task that names an item of each phase is a task of both.
    let mut both = tasks;
    both[3].body = "HELLO-03 and HELLO-01".to_owned();
    let both = from_the_spec(&hello(), &both);
    assert!(both
        .iter()
        .any(|(phase, task)| (*phase, task.number) == (1, 4)));
    assert!(both
        .iter()
        .any(|(phase, task)| (*phase, task.number) == (2, 4)));
}

#[test]
fn tasks_that_name_the_spec_s_tasks_and_need_the_phase_before_follow_its_phases() {
    // T-1, the look at the folder, names none of the spec's tasks and is no phase's:
    // that T-2 needs it is no breach.
    assert_eq!(
        follow_the_phases(&hello(), &tasks()).unwrap(),
        "phase 1: T-2 needs [1]; phase 1: T-3 needs [2]; phase 2: T-4 needs [3]"
    );
}

#[test]
fn a_task_of_a_later_phase_may_reach_the_phase_before_it_through_the_tasks_it_needs() {
    let mut tasks = tasks();
    // Two tasks in phase 2: the second needs only the first, which needs phase 1.
    tasks.push(BoardTask {
        number: 5,
        state: "queued".to_owned(),
        requester: "chief".to_owned(),
        title: "HELLO-04".to_owned(),
        body: String::new(),
        needs: vec![4],
    });
    let mut spec = hello();
    spec.phases[1].tasks.push(code("HELLO-04", false));
    follow_the_phases(&spec, &tasks).unwrap();
}

#[test]
fn a_task_of_a_later_phase_that_needs_nothing_of_the_phase_before_it_is_not_in_order() {
    let mut tasks = tasks();
    tasks[3].needs.clear();
    assert_eq!(
        follow_the_phases(&hello(), &tasks).unwrap_err(),
        "T-4 of phase 2 needs []: none is a task of phase 1"
    );
    // Needing only a task that is none of the spec's is needing nothing of the phase.
    tasks[3].needs = vec![1];
    assert!(follow_the_phases(&hello(), &tasks)
        .unwrap_err()
        .contains("none is a task of phase 1"));
}

#[test]
fn a_task_that_needs_a_task_of_a_later_phase_and_a_phase_with_no_task_are_each_said() {
    let mut tasks = tasks();
    tasks[1].needs = vec![4];
    let failed = follow_the_phases(&hello(), &tasks).unwrap_err();
    assert!(
        failed.contains("T-2 of phase 1 needs T-4, a task of the later phase 2"),
        "{failed}"
    );
    // The chief put phase 1 on the board and not phase 2.
    let only_one: Vec<BoardTask> = self::tasks()
        .into_iter()
        .filter(|task| task.number != 4)
        .collect();
    assert_eq!(
        follow_the_phases(&hello(), &only_one).unwrap_err(),
        "no task of the chief's names a task of phase 2 (HELLO-03)"
    );
    // A task a member made is no task of the chief's.
    let mut by_member = self::tasks();
    by_member[3].requester = "worker".to_owned();
    assert!(follow_the_phases(&hello(), &by_member).is_err());
}

#[test]
fn the_spec_shows_what_the_chief_accepted_in_its_tasks_and_its_registry() {
    let mut spec = hello();
    spec.phases[0].tasks[1].done = true;
    let tasks = tasks();
    // T-2 (HELLO-01) is accepted and HELLO-01 is done; T-3 (HELLO-02) is still at work.
    let row = Registered {
        status: "active".to_owned(),
        done: 2,
        total: 3,
    };
    assert_eq!(
        progress_shows(&spec, Some(&row), &tasks).unwrap(),
        "2 of 3 tasks done in the spec, and in the registry"
    );
}

#[test]
fn an_acceptance_the_spec_does_not_show_a_registry_that_disagrees_and_no_acceptance_are_each_said()
{
    let tasks = tasks();
    let row = |done| Registered {
        status: "active".to_owned(),
        done,
        total: 3,
    };
    // HELLO-01 is accepted and the spec shows it undone.
    let mut spec = hello();
    spec.phases[0].tasks[0].done = false;
    let failed = progress_shows(&spec, Some(&row(0)), &tasks).unwrap_err();
    assert_eq!(
        failed,
        "the board accepted HELLO-01, which the spec still shows undone"
    );
    // The spec shows it and the registry has not caught up.
    let failed = progress_shows(&hello(), Some(&row(0)), &tasks).unwrap_err();
    assert_eq!(
        failed,
        "the registry says 0/3 of hello-files and the spec has 1/3"
    );
    let failed = progress_shows(&hello(), None, &tasks).unwrap_err();
    assert_eq!(failed, "the registry has no row for hello-files");
    // Nothing was accepted: nothing is owed, which is no proof of progress.
    let mut idle = tasks;
    for task in &mut idle {
        task.state = "working".to_owned();
    }
    let failed = progress_shows(&hello(), Some(&row(1)), &idle).unwrap_err();
    assert_eq!(failed, "no task of the spec was accepted on the board");
}
