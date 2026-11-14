//! The instructions each window the daemon opens starts with (TEST-BDC-11): the
//! board's commands for every role, and for coordinators the staff they choose
//! from, the work tiers and the review rule. Nothing from the old transport.
//! The cases of Node's roles suite, but the one that writes a role's text where
//! each harness loads it, which is the launch's (`roleConfiguration`).

use std::fs;
use std::path::Path;

use cf_base::env::Env;
use cf_engine::roles::{
    role_instructions, with_staff_rules, ProjectSpecs, StaffRow, STAFF_RULES_HEADING,
};
use cf_proto::agents::WorkTier;
use cf_proto::cf_word::{CfWord, Shell};
use cf_proto::specs::Variant;
use regex::Regex;

use super::support::{found, staff_row, MEMBER_BANS, OLD_COMMANDS};

fn zeus() -> StaffRow {
    staff_row("zeus", &["worker", "reviewer"], WorkTier::Standard)
}

/// A role's text with no environment and no word of where its cf is.
fn instructions(role: &str, staff: &[StaffRow]) -> String {
    role_instructions(&Env::default(), role, staff, &CfWord::bare(), None).unwrap()
}

fn teaches_only_the_boards_commands(role: &str) {
    let text = instructions(role, &[zeus()]);
    assert!(
        found(&format!(r"^---\nname: consensflow-{role}\n"), &text),
        "{role}"
    );
    assert!(!found(OLD_COMMANDS, &text));
    assert!(!text.contains("{{"), "every placeholder is filled");
    assert!(
        found(
            r"## Your commands\n\nRun each of these in your shell \(your Bash or terminal tool\)",
            &text
        ),
        "{role} knows the commands are shell commands"
    );
    if role == "chief" {
        for command in [
            "cf task add --tier",
            "cf task add --advice --tier",
            "cf task add --design",
            "cf task add --self",
            "cf task add --review --tier",
            "cf task add --test --tier",
            "cf answer",
            "cf task done",
            "cf note --human",
            "cf staff",
            "cf history",
        ] {
            assert!(text.contains(command), "{role} learns {command}");
        }
        // A question raised from a turn's last words ends with a line that says how to take
        // them as the result (T-1119): the chief's text says what the line is.
        assert!(
            found(
                r#"One ending "\(ConsensFlow: … If they are its result: `cf task finish T-5`\.\)"\s+is a turn's last words: answer it, or finish the task\."#,
                &text
            ),
            "the chief learns the line of a question raised from a turn's end"
        );
        // The chief asks the human in its terminal: no command asks on the board.
        assert!(!text.contains("cf ask"));
        assert!(
            !text.contains("cf task add @"),
            "no agent gives another a task by name"
        );
        assert!(found(
            r"(?i)never read another agent's\s+session files",
            &text
        ));
        assert!(
            found(
                r"\| Member \| Roles \| Work tier \|\n\|---\|---\|---\|\n\| zeus \(Opus · high\) \| worker, reviewer \| Standard work \|",
                &text
            ),
            "the staff table: name (and its model where the name says none), roles, tier"
        );
        assert!(!found(r"tags|PM(?-u:\b)", &text), "no tags, no PM");
        assert!(
            found(r"(?m)^## Your commands$", &text),
            "the command card comes first"
        );
        assert!(
            text.find("## Your commands").unwrap() < text.find("## What you do").unwrap(),
            "card first"
        );
        assert!(found(
            r"## Reviews\n\nNothing is reviewed unless you ask\.",
            &text
        ));
        // The tester is one more role the chief names the tier of, and one line of the reviews' section;
        // what each role gives back is its line of the card (spec new-roles, FD-06).
        assert!(found(
            r#"cf task add --test --tier <tier> "…"\s+a test for a tester: what to run and what to check; results with evidence back, no product code changed"#,
            &text
        ));
        // An advisor and a reviewer change no project file, as their own texts say: each may
        // put a long log in a file outside the project's tracked files (T-1435).
        for said in [
            r#"cf task add --advice --tier <tier> "…"\s+a question for an advisor: findings back, no project file changed"#,
            r#"what to review and what to check; findings back, no project file changed"#,
        ] {
            assert!(found(said, &text), "{said}");
        }
        assert!(!found(r"no file changed|changes no file", &text));
        assert!(found(
            r"accept the review\.\n\nA tester checks work by running it, when the work can be run\.\n\n## The staff",
            &text
        ));
        assert!(text.contains("`cf staff` shows them as they are now"));
        // A chief that left the heredoc's word bare to fit a timestamp in ran
        // every backticked snippet of its brief (poker-lab, 2026-10-03).
        assert!(found(
            r"`<<'BRIEF'`, always; to put a value in, write\s+it out",
            &text
        ));
        assert!(found(
            r"A tier nobody on the staff holds\s+goes to the nearest one somebody does, the next one up first; you are told\s+whose\.",
            &text
        ));
        assert!(text.contains("do not run the command to see the refusal: ask"));
        // The image designer draws the chief's precise orders (Gabriel, 2026-10-21), and a
        // revision goes back to its window.
        assert!(found(
            r"Give it a precise\s+drawing order: the subject and composition, the style, the exact colours and\s+size, transparency when needed, each reference file by path and what it is for,\s+and the output path\. It draws exactly that and does not research or choose\.",
            &text
        ));
        assert!(found(
            r"A\s+revision goes back to its window with `--after`\.",
            &text
        ));
        assert!(!found(r"say what to draw", &text));
        // The chief's model is the human's to change (CHF-22): in the chief's window with
        // its harness's own switch, which a reopen keeps; Switch the chief is for another
        // harness or a fresh conversation.
        assert!(found(
            r"The human changes your model with your harness's own switch in this window, and a\s+reopen keeps it\. Switch the chief, in the app, is for another harness or a fresh\s+conversation, on a model from that harness's list\.",
            &text
        ));
        assert!(!found(r"VERDICT|review policy|cf task review", &text));
    } else {
        assert!(text.contains("cf ask"), "{role} can ask");
        assert!(
            found(r"(?m)^## Your commands$", &text),
            "the command card comes first"
        );
        assert!(text.contains("final message of your turn"));
        assert!(text.contains("Questions go to the chief, never to another member"));
        assert!(
            found(
                r"To wait for the chief's answer, ask with `cf ask`, never in your\s+last words\.",
                &text
            ),
            "{role} is told that a question in its last words is its result"
        );
        assert!(
            !text.contains("the human"),
            "{role} is asked and answers only through the chief"
        );
        assert!(found(
            r"(?i)never read another agent's\s+session files",
            &text
        ));
        for (pattern, why) in MEMBER_BANS {
            assert!(!found(pattern, &text), "{role} {why}");
        }
        if role == "reviewer" {
            assert!(text.contains("delivers them to the chief, who decides"));
        }
        if role == "tester" {
            assert!(text.contains("delivers it to the chief, who decides"));
        }
        if role == "advisor" {
            assert!(text.contains("You advise this project's chief"));
            // An advisor may put a long log in a file outside the project, so its text says it
            // changes no project file, in its description and in its intro (T-1435).
            assert!(
                text.contains("with findings and evidence; change no project file.\n"),
                "{text}"
            );
            assert!(
                found(r"An advice task changes no\s+project file\.", &text),
                "{text}"
            );
            assert!(!found(r"changes? no file", &text));
        }
        if role == "designer" {
            assert!(text.contains("image generation tool"));
            // It draws the order and nothing else (Gabriel, 2026-10-21).
            assert!(found(
                r"a drawing order, which says what\s+to draw, the references to use and where to save the result\. Draw exactly what\s+it says, at the size and path it names, using only the references it names\.",
                &text
            ));
            assert!(found(
                r"Do\s+not research the project or redesign what was asked\.",
                &text
            ));
            assert!(found(
                r#"If something needed to draw it is\s+missing or contradicts itself, or you cannot go on without an answer, ask with\s+`cf ask "…"` rather than guess, and end your turn"#,
                &text
            ));
            // Its result is as it was: the paths, then one line on what each shows.
            assert!(found(
                r"the absolute path\s+of every file you saved, one per line, then one line on what it shows",
                &text
            ));
        } else {
            assert!(
                !text.contains("drawing order"),
                "{role} is not told what the designer draws"
            );
        }
        // Poker-lab, 2026-10-03: a worker's rm -rf "$W/$d" stopped Claude Code
        // for a human, with a countdown, in a window where every permission is
        // granted; after the rule named rm -rf, another's rm -f -- "$R/$b" did.
        assert!(found(
            r#"any removal \(`rm`, `rm -f`,\s+`rmdir`\) whose path a variable makes \(`"\$R/\$b"`\)"#,
            &text
        ));
        assert!(found(
            r#"Write a removal's paths out in full, or as `"\$\{R:\?\}/\$\{b:\?\}"`\."#,
            &text
        ));
        // The owner's Windows machine, 2026-10-15: a worker overwrote a file that held
        // uncommitted work with `git checkout`, and others said they had restored files
        // they had not.
        assert!(found(
            r"Never\s+throw away a change you did not make: no `git checkout` or `git restore` that\s+overwrites a file \(`git restore --staged` of your own change is fine\), and no\s+`git reset --hard`, `git stash` or `git clean` over work that is not committed;",
            &text
        ));
        assert!(found(
            r"Say a file is restored only once you have\s+checked it: its diff, or what it holds\.",
            &text
        ));
    }
}

#[test]
fn teach_the_chief_only_the_board_s_commands() {
    teaches_only_the_boards_commands("chief");
}

#[test]
fn teach_the_advisor_only_the_board_s_commands() {
    teaches_only_the_boards_commands("advisor");
}

#[test]
fn teach_the_worker_only_the_board_s_commands() {
    teaches_only_the_boards_commands("worker");
}

#[test]
fn teach_the_reviewer_only_the_board_s_commands() {
    teaches_only_the_boards_commands("reviewer");
}

#[test]
fn teach_the_tester_only_the_board_s_commands() {
    teaches_only_the_boards_commands("tester");
}

#[test]
fn teach_the_designer_only_the_board_s_commands() {
    teaches_only_the_boards_commands("designer");
}

#[test]
fn every_member_is_told_when_its_result_is_delivered_and_what_to_do_with_background_work() {
    let rules = [
        (
            "wait for the result",
            "Your answer ends your turn: wait for what your result needs.",
        ),
        (
            "delivery and leftovers",
            "Your result is delivered once your turn has ended and nothing you started is still running, so before your last answer end what you left in the background (a wait loop, a watcher).",
        ),
    ];
    let mut missing = Vec::new();
    for (shell, cf) in [
        ("POSIX", CfWord::bare()),
        (
            "PowerShell",
            CfWord::new("C:/Program Files/ConsensFlow/bin/cf.exe", Shell::PowerShell),
        ),
    ] {
        for role in cf_ledger::roles::ids() {
            let text = role_instructions(&Env::default(), role, &[], &cf, None)
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            for (name, rule) in rules {
                if !text.contains(rule) {
                    missing.push(format!("{role} ({shell}): {name}"));
                }
            }
            if text.contains("if you are still waiting on background work, say so in a line") {
                missing.push(format!("{role} ({shell}): still-waiting line"));
            }
        }
    }
    assert!(missing.is_empty(), "missing rules: {}", missing.join(", "));
}

#[test]
fn gives_every_member_one_shared_text_with_its_role_s_own_parts() {
    let shared = [
        "Run each of these in your shell (your Bash or terminal tool).",
        "Do not hand out tasks, launch other agents or type into other windows, and\nnever read another agent's session files: the board is your only channel.",
        "This window is for one task: it opened with the task and closes when the task\nleaves your hands.",
        // A window the chief continues (`--after`) comes back on its own conversation: the text
        // does not tell it that nothing from an earlier task is there (T-1435).
        "A new window starts from nothing; one the chief continues\nhas its earlier conversation. Either way, finish with a result that stands on\nits own.",
        // A turn's end does not end background work; the result waits for it.
        "Your answer ends your turn: wait for what your result needs.",
    ];
    for (role, title) in [
        ("worker", "worker"),
        ("advisor", "advisor"),
        ("reviewer", "reviewer"),
        ("tester", "tester"),
        ("designer", "image designer"),
    ] {
        let text = instructions(role, &[zeus()]);
        for rule in shared {
            assert!(text.contains(rule), "{role} keeps: {rule}");
        }
        assert!(!text.contains("carries over"), "{role}");
        assert!(found(&format!(r"\n# ConsensFlow {title}\n"), &text));
        assert!(found(
            &format!(r"watcher\)\.\s+Keep this {role} role for the whole session\.\n$"),
            &text
        ));
    }
    assert!(found(
        r"cf task get T-3 {15}a task and its whole thread: the work under review, or this one\n",
        &instructions("reviewer", &[])
    ));
    assert!(found(
        r"cf task get T-3 {15}a task and its whole thread: the work under test, or this one\n",
        &instructions("tester", &[])
    ));
    let refused = role_instructions(&Env::default(), "pm", &[], &CfWord::bare(), None).unwrap_err();
    assert_eq!(refused.to_string(), "no role instructions for pm");
}

#[test]
fn tells_every_window_it_never_has_to_wait_for_what_the_board_brings() {
    // A turn spent sleeping is one no result or answer can reach (Devin waited so, 2026-10-02).
    assert!(found(
        r"You never have to poll or wait for a result: ConsensFlow brings each one to\s+you as a message when it is ready",
        &instructions("chief", &[zeus()])
    ));
    for role in cf_ledger::roles::ids() {
        assert!(
            found(
                r"Never wait for an answer\s+in your shell \(no `sleep`",
                &instructions(role, &[zeus()])
            ),
            "{role}"
        );
    }
}

#[test]
fn tells_the_tester_to_run_the_work_report_with_evidence_and_change_no_product_code() {
    let text = instructions("tester", &[zeus()]);
    for pattern in [
        r"(?m)^description: Verify finished work for the chief of a ConsensFlow project by running it, change no product code, and return results with evidence\.$",
        r"You verify finished work for this project's chief by running it\.",
        r"You change no product code: a\s+test or script you write is part of your result, and only where the task asks\s+for one\.",
        r"what you tested and how; each check, passed or failed, with the exact\s+command you ran and what it printed; each bug with the steps to reproduce it;\s+where any test or script you wrote is; and whether the work is ready\.",
    ] {
        assert!(found(pattern, &text), "{pattern}");
    }
    // It runs things: nothing of the reviewer's reading only is left in its text.
    assert!(!found(r"read-only|changes no file", &text));
}

/// Every role of the table is a command of the chief's card (spec new-roles, FD-06): a role
/// whose switch the card does not show is one the chief cannot give work to, and a role with no
/// tier is shown with none.
#[test]
fn shows_the_chief_a_command_for_every_role_of_the_table_with_a_tier_only_where_it_has_them() {
    let text = instructions("chief", &[zeus()]);
    let card = section(&text, "Your commands");
    for role in cf_ledger::roles::all() {
        let shown = match (role.switch, role.tiered) {
            (None, _) => "cf task add --tier <critical|complex|standard|light> \"…\"".to_owned(),
            (Some(switch), true) => format!("cf task add {switch} --tier <tier> \"…\""),
            (Some(switch), false) => format!("cf task add {switch} \"…\""),
        };
        assert!(
            card.contains(&shown),
            "{}: the card does not show {shown}",
            role.id
        );
    }
}

/// What the chief is told of the four roles of spec new-roles and of a member's images (FD-06,
/// FD-07; T-1480): when work goes to each, in the section on roles, and the researcher in What
/// you do.
#[test]
fn tells_the_chief_when_work_goes_to_each_of_the_four_roles_and_how_a_members_images_are_drawn() {
    let text = instructions("chief", &[zeus()]);
    assert_says(
        section(&text, "What you do"),
        "the chief's What you do",
        &["reading of more than a few lookups to the researcher, so that only its answer enters your conversation."],
    );
    assert_says(
        section(&text, "Choose the role, not the member"),
        "the chief's section on roles",
        &[
            "Name the role a task needs, and its tier where its command above takes `--tier`.",
            "Only a role nobody holds is refused, by its name: only the human adds members, so ask them for one.",
            // The architect (A3, A5): a design first, and the briefs point to its files.
            "Work big enough to need a design (a new module, a new API, a change of structure) goes to the architect first; its files stay, and the workers' briefs point to them.",
            "It also reviews a change against its design.",
            // The frontend designer: the purpose, never the look (rule 0); never beside a worker in the same files.
            "How a screen looks, feels and moves is the frontend designer's call: its brief gives the purpose (who uses the screen, what they must be able to do, what exists already), never the look, and a worker's question of look goes to it through you.",
            "Never give it and a worker the same files at once: order the two with `--needs`.",
            "It can also go over a screen a worker built.",
            // The integrator.
            "Where the staff has an integrator, it lands accepted work; a conflict that needs a choice about what the product does comes back to you as a question with both sides.",
            "It makes no tag, release or deploy unless its brief says so.",
            // Rule 5, the image flow.
            "A member that needs images asks you with a drawing order for each: add a `--design` task per order and, when all are done, answer in words, then each file's path and its line from ConsensFlow.",
            "With no image designer, the member makes them itself (SVG, CSS or its harness's image tool), or you ask the human for them, with the orders.",
        ],
    );
}

#[test]
fn tells_the_chief_one_member_runs_as_many_tasks_at_once_as_it_is_given() {
    // Item 2 says to give parallel work together however few members a role has; the staff's
    // section says why: a member runs any number of tasks at once (T-1480 took the second
    // saying of it out of item 2).
    let text = instructions("chief", &[zeus()]);
    assert!(found(
        r"Independent tasks run side by side, each in a fresh window, however few\s+members a role has, so give work that can go in parallel as separate\s+tasks, together\.",
        &text
    ));
    assert!(found(
        r"it runs any number of tasks at once, each in its own\s+window\.",
        &text
    ));
}

/// A chief that read a member as a person who can be busy chained its work through one
/// window with `--after` and let all of it wait behind a window that stuck (Gabriel's field
/// board, T-6 then T-8 then T-9, 2026-11-02): a member is a model, and no task waits for one.
#[test]
fn never_says_a_member_is_free_or_busy() {
    let text = instructions("chief", &[zeus()]);
    let said = Regex::new(r"(?i)\b(free|busy)\b").unwrap().find(&text);
    assert!(
        said.is_none(),
        "the chief's text says a member is free or busy: {:?}",
        said.map(|word| &text[word.start().saturating_sub(60)..word.end() + 60])
    );
}

#[test]
fn tells_the_chief_each_task_gets_a_new_window_of_a_member_of_its_role_and_tier() {
    assert!(found(
        r"Each task gets a new window of a member of that role and tier on this\s+project's staff; you never pick the member\.",
        &instructions("chief", &[zeus()])
    ));
}

#[test]
fn tells_the_chief_a_member_is_a_model_that_runs_any_number_of_tasks_at_once_each_in_its_own_window(
) {
    let text = instructions("chief", &[zeus()]);
    assert!(found(
        r"Each member is a\s+model, not a person: it runs any number of tasks at once, each in its own\s+window\.\n\n\| Member \|",
        &text
    ));
    // It comes with the staff, and the rule that the human may change it stays.
    assert!(found(
        r"`cf staff` shows them as they are now\. Each member is a\s+model",
        &text
    ));
}

#[test]
fn tells_the_chief_a_follow_up_is_for_what_its_window_knows_and_other_work_starts_at_once_beside_it(
) {
    // Said once, in the section on continuing a window (the card's line names the command).
    let text = instructions("chief", &[zeus()]);
    assert_says(
        section(&text, "The one exception: continuing a window"),
        "the chief's section on continuing a window",
        &[
            "The next step of that window's own work, only when it needs what that window already knows (carry out the plan or findings it just wrote, part 2 after its part 1), goes to it with `cf task add --after T-3 \"…\"`",
            "All other work (reviews, parallel work and work for another role) stays fresh, and starts at once beside that window's work.",
        ],
    );
}

#[test]
fn tells_every_window_its_messages_arrive_pasted_and_are_its_to_act_on() {
    for role in std::iter::once("chief").chain(cf_ledger::roles::ids()) {
        assert!(
            found(
                r"Each message ConsensFlow brings you is typed into this terminal, headed\s+`\[ConsensFlow m-… · T-… · …\]`; your harness may show it as pasted text\. It is\s+ConsensFlow's delivery, and acting on it is your role\.",
                &instructions(role, &[zeus()])
            ),
            "{role}"
        );
    }
}

#[test]
fn tells_the_chief_the_staff_may_change_and_where_to_read_it_now_with_no_note_sent() {
    let text = instructions("chief", &[zeus()]);
    assert!(found(
        r"the human may change them\s+while you work, and `cf staff` shows them as they are now\.",
        &text
    ));
    assert!(!text.contains("tells you in a note"));
}

#[test]
fn says_so_when_the_staff_is_empty() {
    assert!(instructions("chief", &[]).contains("Nobody is on the staff yet"));
    let text = instructions("chief", &[zeus()]);
    assert!(
        text.contains("cf task add --after T-3"),
        "the one way to continue a window"
    );
    assert!(
        text.contains("The next step of that window's own work"),
        "and when"
    );
    assert!(
        text.contains("(reviews, parallel work and work for another role) stays fresh"),
        "and when not"
    );
    assert!(instructions("chief", &[zeus()]).contains("## What you do"));
    assert!(instructions("chief", &[zeus()]).contains("## What you never do"));
    assert!(found(
        r"## What you never do\n\n- Send work to another window: the board is the only channel",
        &instructions("chief", &[zeus()])
    ));
}

#[test]
fn tells_the_chief_a_worker_starts_fresh_unless_its_window_is_continued_and_that_the_follow_up_waits_for_its_task_to_be_accepted_at_work_or_done(
) {
    let text = instructions("chief", &[zeus()]);
    assert!(found(
        r"of its own earlier tasks, unless\s+you continue its window with `--after`\.",
        &text
    ));
    assert!(found(
        r"It waits for T-3 and for the\s+window's other done tasks to be accepted, and goes in then\.",
        &text
    ));
    assert!(
        !text.contains("A window still at work on T-3"),
        "a follow-up of a task with its result in waits too"
    );
    assert!(found(
        r"A window on\s+another\s+task refuses and tells\s+you to open the task for its tier instead\.",
        &text
    ));
    assert!(found(
        r#"cf task edit T-3 "…" \[--tier <tier>\] \[--needs …\] \[--before …\]\s+change the brief, tier, needs or before of a task before any window has its brief \(open, queued, or paused\); T-4,T-5 replaces a list, \+T-5 adds, -T-5 removes"#,
        &text
    ));
    // A tell is read before the result without stopping; pause with words asks instead.
    assert!(found(
        r#"cf tell T-3 "…"\s+say this to T-3's window without stopping it: it reads your words before its result is taken, and owes no answer"#,
        &text
    ));
    assert!(found(
        r#"cf task pause T-3 \["…"\]\s+stop a worker's task: its window waits; background work runs on; with words, they go to its window as a question, its answer arrives as a message; then cf task resume T-3 "…" or cf task finish T-3"#,
        &text
    ));
    assert!(found(
        r#"cf task edit\|reopen\|resume T-3 … --title "…"\s+a new title; edit takes it alone, in any state"#,
        &text
    ));
    assert!(
        !text.contains("session the human has deleted"),
        "a deleted session comes back"
    );
    assert!(!found(
        r"keeps its\s+conversation until the human deletes it",
        &text
    ));
}

/// Every role's text, with the staff and the specs a chief can be given, for the `cf` a
/// window is told: the roles a window can have, so a role added later is read too.
fn every_text_for(cf: &CfWord) -> Vec<(String, String, usize)> {
    let specs = ProjectSpecs {
        variant: Variant::Tdd,
        folder: "docs/specs",
        skill: "/home/me/.consensflow/skills/specmint-tdd/SKILL.md",
    };
    let mut texts = Vec::new();
    for role in std::iter::once("chief").chain(cf_ledger::roles::ids()) {
        for specs in [None, Some(&specs)] {
            let text = role_instructions(&Env::default(), role, &[zeus()], cf, specs).unwrap();
            // The commands each starts with: the chief's table and prose, a member's table and
            // the line of its result.
            let least = if role == "chief" { 37 } else { 7 };
            texts.push((
                format!("the {role}'s, specs {}", specs.is_some()),
                text,
                least,
            ));
        }
    }
    texts
}

/// The line of `text` that says the word `cf` on its own, once `word`, which every command
/// of a window starts with, is taken out of it.
fn a_bare_cf(text: &str, word: &str) -> Option<String> {
    let without = text.replace(word, "");
    let bare = Regex::new(r"(?-u:\b)cf(?-u:\b)").unwrap().find(&without)?;
    let from = without[..bare.start()].rfind('\n').map_or(0, |at| at + 1);
    let to = without[bare.end()..]
        .find('\n')
        .map_or(without.len(), |at| bare.end() + at);
    Some(without[from..to].to_owned())
}

/// Every text of every role starts each command with `cf`, and says the word on no other
/// occasion.
fn holds_for(cf: &CfWord) {
    let word = cf.as_str();
    for (which, text, least) in every_text_for(cf) {
        assert!(
            !text.contains("This window's cf"),
            "{which}: the section that asked a window to run it by its path is gone"
        );
        assert!(!text.contains("{{"), "{which}: every placeholder is filled");
        let commands = text.matches(&format!("{word} ")).count();
        assert!(
            commands >= least,
            "{which}: {commands} commands start with {word}"
        );
        assert_eq!(a_bare_cf(&text, word), None, "{which}: a bare cf");
    }
}

/// A window's shell can find another `cf` first on its PATH (a Devin member's login zsh puts
/// the user's `~/.local/bin` first, a Claude Code member's login Git Bash `~/bin`, 2026-10-31;
/// the Cloud Foundry CLI is a `cf` too), so every command a text teaches names this window's
/// own `cf` by its full path, and no text says the word `cf` of a shell's own finding.
#[test]
fn every_command_a_text_teaches_names_this_window_s_cf_by_its_full_path() {
    holds_for(&CfWord::new("/opt/consensflow/bin/cf", Shell::Posix));
}

/// The default install of Windows is `C:/Program Files/ConsensFlow/bin/cf.exe`: a path with a
/// space is one quoted word in bash, and in PowerShell a command only after `&` (T-1049). A
/// path with a shell's own characters, a quote, a backtick, a `$`, is one in each.
#[test]
fn a_cf_a_shell_would_read_is_one_word_in_every_command_as_its_shell_takes_it() {
    for (path, posix, powershell) in [
        (
            "C:/Program Files/ConsensFlow/bin/cf.exe",
            "'C:/Program Files/ConsensFlow/bin/cf.exe'",
            "& 'C:/Program Files/ConsensFlow/bin/cf.exe'",
        ),
        (
            "/Users/me/My Apps/cf `x` $HOME 'q'",
            r"'/Users/me/My Apps/cf `x` $HOME '\''q'\'''",
            "& '/Users/me/My Apps/cf `x` $HOME ''q'''",
        ),
    ] {
        for (shell, word) in [(Shell::Posix, posix), (Shell::PowerShell, powershell)] {
            let cf = CfWord::new(path, shell);
            assert_eq!(cf.as_str(), word);
            holds_for(&cf);
        }
    }
}

#[test]
fn with_no_path_the_commands_say_cf_and_a_path_that_is_empty_is_none() {
    for text in [
        instructions("worker", &[]),
        instructions("chief", &[zeus()]),
        role_instructions(
            &Env::default(),
            "worker",
            &[],
            &CfWord::new("", Shell::Posix),
            None,
        )
        .unwrap(),
    ] {
        assert!(text.contains("\n    cf "), "the commands start with cf");
        assert!(!text.contains("This window's cf"));
        assert!(!text.contains("''"), "no quoted nothing");
    }
}

/// T-1510's review of T-1480: the explicit ban went to pay for the four roles' words, and what
/// was left said it only in part (a brief written for one member was no longer banned). It is
/// back in What you never do (T-1513).
#[test]
fn never_gives_a_task_to_a_member_by_name_nor_writes_one_with_a_member_in_mind() {
    let text = instructions("chief", &[]);
    let never = text
        .split("## What you never do")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("What you never do");
    assert!(
        found(
            r"- Give a task to a member by name, or write one with a member in mind\.",
            never
        ),
        "{never}"
    );
}

#[test]
fn gives_the_chief_s_subagents_if_its_harness_has_them_nothing_beyond_searching_and_reading() {
    assert!(found(
        r"Hand your harness's subagents or task tool, if it has them, any work\s+beyond searching and reading: the board, the human and the staff never see\s+that work",
        &instructions("chief", &[])
    ));
}

/// harness-models removed the saved agent: a member is a harness, a model it lists, an
/// effort and a tier, named by the model. No role text may send a window to pick an agent,
/// a preset or a nickname, or to a command that no longer exists (`cf agent`, `cf catalog`).
#[test]
fn names_no_agent_to_pick_no_preset_no_nickname_and_no_cf_agent_or_catalog_in_any_text() {
    let removed = Regex::new(
        r"(?i)\bpresets?\b|\bnicknames?\b|\b(?:cf|\{\{cf\}\}) +(?:agent|agents|catalog)\b|\bsaved agents?\b|\b(?:pick|choose|name) (?:an|the) agent\b|\bagent (?:to pick|name)\b",
    )
    .unwrap();
    let staff = [zeus()];
    for role in std::iter::once("chief").chain(cf_ledger::roles::ids()) {
        let text = instructions(role, &staff);
        let said = removed.find(&text);
        assert!(
            said.is_none(),
            "the {role}'s text names a removed thing: {:?}",
            said.map(|word| &text[word.start().saturating_sub(60)..word.end() + 60])
        );
    }
    // The files themselves, with their slots unfilled, not only the text a window gets.
    for file in ["chief.md", "staff.md", "shells.md"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../skill/core")
            .join(file);
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            removed.find(&text).is_none(),
            "{file} names a removed thing"
        );
    }
}

#[test]
fn refuses_an_unknown_role() {
    let refused =
        role_instructions(&Env::default(), "king", &[], &CfWord::bare(), None).unwrap_err();
    assert_eq!(refused.to_string(), "no role instructions for king");
}

/// The files under `folder`, in every folder below it.
fn files_under(folder: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn keeps_the_authorization_boundary_in_the_chief_text_ships_no_harness_payload_and_no_personal_name(
) {
    let skill = instructions(
        "chief",
        &[staff_row("zeus", &["worker"], WorkTier::Critical)],
    );
    // The chief changes nothing itself: every change goes on the board, unless the human says otherwise.
    assert!(skill.contains("You do not change the project yourself"));
    // The human works with the chief in its terminal: answered and asked
    // there, whole (a summary on the board once lost a cost table, btb
    // 2026-09-28); nobody is asked on the board.
    assert!(skill.contains("The human works with you here, in this terminal"));
    assert!(skill.contains("Nobody is asked on the board"));
    assert!(skill.contains("whole, not a summary"));
    assert!(!found(
        r"here briefly|answer a worker's question before you see it",
        &skill
    ));
    assert!(skill.contains("however small"));
    assert!(skill.contains("the human tells you to do a change yourself"));
    assert!(skill.contains("never type into another window or launch agents"));
    assert!(skill.contains("only the human gives you work, here in your terminal"));
    assert!(skill.contains("the human never accepts work on the board"));
    // One role text per role, read by the daemon: no host payload carries a second copy.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(
        !root.join("hosts").join("claude").exists(),
        "no claude payload"
    );
    assert!(!root.join("hosts").join("pi").exists(), "no pi payload");
    // The personal name must not appear in anything that ships: the host
    // extensions, the role texts, the page, and what each crate is built from
    // (its `src` and its `data`, the unit tests among them; the tests in its
    // `tests` folder are not built into anything, and say what they like).
    // The Specmint skills (`skill/specmint`) ship as their author wrote them,
    // under the author's licence, which names the author: they are not ours to
    // rewrite.
    let mut shipped = vec![
        root.join("hosts"),
        root.join("skill").join("core"),
        root.join("app").join("ui"),
    ];
    for entry in fs::read_dir(root.join("crates")).unwrap() {
        let krate = entry.unwrap().path();
        shipped.extend(
            ["src", "data"]
                .map(|folder| krate.join(folder))
                .into_iter()
                .filter(|folder| folder.is_dir()),
        );
    }
    for base in shipped {
        for file in files_under(&base) {
            let content =
                String::from_utf8_lossy(&fs::read(&file).unwrap_or_default()).into_owned();
            assert!(!content.contains("Gabriel"), "{}", file.display());
        }
    }
}

#[test]
fn gives_the_chief_only_the_named_card_nothing_of_the_board_and_leaves_the_other_roles_alone() {
    let dir = tempfile::tempdir().unwrap();
    let card = dir.path().join("no-card.md");
    fs::write(&card, "You work in this project for its owner.\n").unwrap();
    let staff = [staff_row("zeus", &["worker"], WorkTier::Standard)];
    let env = Env::from_vars([("CONSENSFLOW_EVAL_CHIEF_CARD", card.as_os_str())]);
    assert_eq!(
        role_instructions(
            &env,
            "chief",
            &staff,
            &CfWord::new("/app/bin/cf", Shell::Posix),
            None
        )
        .unwrap(),
        "You work in this project for its owner.\n",
        "no tiers, no staff, no cf"
    );
    assert!(role_instructions(
        &env,
        "worker",
        &staff,
        &CfWord::new("/app/bin/cf", Shell::Posix),
        None
    )
    .unwrap()
    .contains("/app/bin/cf"));
    assert!(instructions("chief", &staff).contains("You do not change the project yourself"));
}

// The rules of spec `smaller-contexts` (T-1009, SCX-06 to SCX-10): reviews that
// end, fresh windows, targeted checks, briefs that say how a fact is known, and
// results that begin with the outcome. Each sentence is held by a rule below, and
// a plant drops it (`tests/plants/daemon/smaller-contexts.toml`).

/// Whether `text` says `sentence`: its words in order, any white space between
/// two of them (the texts are wrapped).
fn says(text: &str, sentence: &str) -> bool {
    let words: Vec<_> = sentence.split_whitespace().map(regex::escape).collect();
    found(&words.join(r"\s+"), text)
}

/// Fails for each of `sentences` that `text` does not say.
#[track_caller]
fn assert_says(text: &str, whose: &str, sentences: &[&str]) {
    for sentence in sentences {
        assert!(says(text, sentence), "{whose} does not say: {sentence}");
    }
}

/// The part of the chief's text under the heading `## {heading}`, up to the next.
fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let start = text
        .find(&format!("\n## {heading}\n"))
        .unwrap_or_else(|| panic!("no section {heading}"))
        + 1;
    let end = text[start + 3..]
        .find("\n## ")
        .map_or(text.len(), |at| start + 3 + at);
    &text[start..end]
}

#[test]
fn every_member_is_taught_when_and_how_to_tell_without_waiting() {
    for role in cf_ledger::roles::ids() {
        let text = instructions(role, &[zeus()]);
        let commands = section(&text, "Your commands");
        assert_says(
            commands,
            role,
            &[
                "cf tell \"…\" tell whoever gave you the task, without asking or waiting",
                "A tell to your chief is for what it must know before your result: a wrong premise in the brief, a finding that changes another task, a blocker you work around.",
                "Progress is not a tell.",
                "Go on working after a tell.",
                "If you cannot go on without an answer, use `cf ask` and end your turn.",
            ],
        );
        assert_eq!(text.matches("cf tell ").count(), 1, "{role}");
        assert!(!text.contains("cf note --human"), "{role}");
    }
}

#[test]
fn the_chief_is_told_a_members_tell_needs_no_answer() {
    let text = instructions("chief", &[zeus()]);
    assert_says(
        section(&text, "What you do"),
        "the chief",
        &["A tell from a member on a task you gave arrives headed `[… note from @worker]`; it asks no answer, so act on it or not as you judge."],
    );
}

#[test]
fn tells_the_chief_a_blocker_is_evidenced_and_the_rest_does_not_hold_the_work() {
    let reviews = instructions("chief", &[zeus()]);
    let reviews = section(&reviews, "Reviews");
    assert_says(
        reviews,
        "the chief's Reviews",
        &[
            "A finding blocks the work only when it would make it fail (a wrong result, lost data, a break in normal use, a failure in a case that will really happen) and names the requirement, the case, the failure one would see, and the evidence.",
            "Reopen for blockers only.",
            "Whether a blocker belongs in this work at all is a separate question; dropping a capability is an answer to a blocker too.",
            "The rest (naming, style, wording, unlikely cases, nice-to-haves) doesn't hold the work: accept it, and gather what is worth doing into one later task.",
        ],
    );
}

#[test]
fn tells_the_chief_a_second_review_checks_the_fixes_and_not_the_whole_work_again() {
    let text = instructions("chief", &[zeus()]);
    assert_says(
        section(&text, "Reviews"),
        "the chief's Reviews",
        &[
            "A second review checks the fixes: give it the earlier findings by number and the change since.",
            "It says of each whether it is fixed, still open, fixed only in part, or newly broken by a fix, and whether a fix broke what it touches; it doesn't review the whole work again.",
        ],
    );
}

#[test]
fn tells_the_chief_to_change_the_work_after_two_reviews_that_found_blockers_or_to_ask_the_human() {
    let text = instructions("chief", &[zeus()]);
    assert_says(
        section(&text, "Reviews"),
        "the chief's Reviews",
        &[
            "When two reviews of the same work have both found blockers, stop sending it back as it is.",
            "Change the work: simplify it, turn a capability off, split it, start a fresh window with a short, checked handoff, or try another approach.",
            "If none fits, ask the human here.",
            "Accepting work with an open blocker needs the human's yes.",
        ],
    );
    // The earlier rule stays: the chief decides the work and the review, and
    // nothing on the board sends a review back (CORE-27).
    assert_says(
        section(&text, "Reviews"),
        "the chief's Reviews",
        &["Then decide both: reopen the work with what must change, or accept it, and accept the review."],
    );
    assert!(!text.contains("Review round"));
}

#[test]
fn tells_the_chief_to_ask_a_reviewer_for_no_pass_or_fail_first_line() {
    let text = instructions("chief", &[zeus()]);
    assert_says(
        section(&text, "Reviews"),
        "the chief's Reviews",
        &[
            r#"Don't ask a reviewer for a pass-or-fail first line: its own "ready", "ready with notes" or "not ready" stands."#,
        ],
    );
    assert!(!found(r"PUSH AS WRITTEN|FIX FIRST", &text));
}

#[test]
fn tells_the_chief_to_start_a_fresh_window_for_the_next_round_after_about_three_rounds_of_the_same_work(
) {
    let text = instructions("chief", &[zeus()]);
    assert_says(
        section(&text, "The one exception: continuing a window"),
        "the chief's section on continuing a window",
        &[
            "After about three rounds of the same work (reopens, `--after`), a window's conversation is long and every step re-reads it.",
            // A follow-up in a window left for hours wrote 300,000 to 660,000 tokens again
            // (T-1446's measurements): a late one is a new task too.
            "One that finished hours ago reads it all again at its first call.",
            "For the next round, or a late follow-up, add a new task that says where things stand (branch and head, done, left, findings still open), then accept the old one: its open findings go on in the new task.",
            "Accepting releases whatever `--needs` it, so if tasks wait on it, add the new task `--before` them first.",
        ],
    );
}

// What T-1009's cuts took out that the critical review (T-1014, notes 3 and 9) asked back.

#[test]
fn tells_the_chief_it_may_change_the_project_itself_when_the_human_says_so_and_why_no_work_goes_to_another_window(
) {
    let text = instructions("chief", &[zeus()]);
    // The rule and its exception are said once, in What you do (T-1480 took What you never
    // do's second saying of them out).
    assert_says(
        section(&text, "What you do"),
        "the chief's What you do",
        &[
            "You do not change the project yourself: no file edits, no commits or pushes, no builds, releases or deploys.",
            "The one exception: the human tells you to do a change yourself.",
        ],
    );
    assert_says(
        section(&text, "What you never do"),
        "the chief's What you never do",
        &["Send work to another window: the board is the only channel, and only the human gives you work, here in your terminal; never read another agent's session files."],
    );
}

#[test]
fn tells_the_chief_its_own_later_step_comes_back_when_the_task_it_needs_is_accepted_naming_no_task()
{
    let text = instructions("chief", &[zeus()]);
    let what_you_do = section(&text, "What you do");
    assert_says(
        what_you_do,
        "the chief's What you do",
        &["A later step of yours goes on the board with `--self` (above): its brief comes back to this window once the task it needs is accepted."],
    );
    // The command card defines T-3; the sentence stands alone without it.
    assert!(!says(what_you_do, "once T-3 is accepted"));
}

#[test]
fn tells_the_reviewer_a_review_changes_no_project_file_since_a_long_log_goes_in_a_file_of_its_own()
{
    assert_says(
        &instructions("reviewer", &[zeus()]),
        "the reviewer",
        &[
            "A review changes no project file: suggested fixes belong in your findings.",
            "A long log, table or listing goes in a file outside the project's tracked files",
        ],
    );
}

#[test]
fn tells_the_chief_to_keep_its_own_checks_targeted_and_to_read_no_result_it_already_has() {
    let text = instructions("chief", &[zeus()]);
    assert_says(
        section(&text, "What you do"),
        "the chief's What you do",
        &[
            "Keep your own checks targeted (the diff stat, a grep, one test, the lines that changed); a deep check (the whole change, the suites) goes to a reviewer or tester.",
            "A result already in your conversation needs no `cf task get`.",
            "Look at an image yourself only when the decision needs your eyes; otherwise ask the member what it shows.",
        ],
    );
}

#[test]
fn tells_the_chief_to_put_commands_that_go_together_in_one_shell_call() {
    let text = instructions("chief", &[zeus()]);
    assert_says(
        section(&text, "Your commands"),
        "the chief's command card",
        &["Put commands that go together in one shell call, an accept and the next task for example: each call re-reads everything you hold."],
    );
}

#[test]
fn tells_the_chief_to_say_how_it_knows_each_fact_a_brief_has_a_member_act_on() {
    let text = instructions("chief", &[zeus()]);
    assert_says(
        section(&text, "What you do"),
        "the chief's What you do",
        &[
            r#"For each fact a brief states that the member will act on (a version, a commit, "X is on main", a machine's state), say in a few words how you know it: what you ran or read, and when."#,
            "If you have not checked it, say so and ask the member to check it first and tell you if it does not hold.",
        ],
    );
}

#[test]
fn tells_the_reviewer_to_begin_with_whether_the_work_is_ready_and_to_number_each_finding_and_say_if_it_blocks(
) {
    let text = instructions("reviewer", &[zeus()]);
    assert_says(
        &text,
        "the reviewer",
        &[
            "Begin with whether the work is ready (ready, ready with notes, or not ready) and what the chief has to decide, in a few lines.",
            "Then each finding, numbered so a later review can name it, with its evidence, its location and whether it blocks.",
            "A finding blocks only when it would make the work fail, and then it names the requirement, the case and the failure one would see.",
            "The rest is optional.",
            "The work is ready when nothing blocks.",
        ],
    );
    // The reviewer's rules are its own: no other member is asked to number findings.
    for role in cf_ledger::roles::ids()
        .into_iter()
        .filter(|role| *role != "reviewer")
    {
        let text = instructions(role, &[zeus()]);
        assert!(
            !says(&text, "numbered so a later review can name it"),
            "{role}"
        );
    }
}

#[test]
fn tells_the_reviewer_a_second_review_checks_the_earlier_findings_and_what_the_fixes_touch() {
    assert_says(
        &instructions("reviewer", &[zeus()]),
        "the reviewer",
        &[
            "On a second review of the same work, check the earlier findings and what the fixes touch, not the whole work again: say of each finding whether it is fixed, still open, fixed only in part, or newly broken.",
        ],
    );
}

#[test]
fn tells_a_worker_an_advisor_a_reviewer_and_a_tester_to_begin_with_the_outcome_and_what_the_chief_decides(
) {
    for (role, begin) in [
        (
            "worker",
            "Begin with the outcome and what the chief has to decide, in a few lines. Then the details.",
        ),
        (
            "advisor",
            "Begin with the outcome and what the chief has to decide, in a few lines. Then the details.",
        ),
        (
            "tester",
            "Begin with the outcome and what the chief has to decide, in a few lines. Then the details.",
        ),
        // The reviewer's outcome is whether the work is ready (its own rule above).
        (
            "reviewer",
            "Begin with whether the work is ready (ready, ready with notes, or not ready) and what the chief has to decide, in a few lines.",
        ),
    ] {
        assert_says(&instructions(role, &[zeus()]), role, &[begin]);
    }
    // The image designer's result is the files it saved and a line on what they show.
    assert!(!says(
        &instructions("designer", &[zeus()]),
        "what the chief has to decide"
    ));
}

#[test]
fn tells_a_worker_an_advisor_a_reviewer_and_a_tester_to_put_a_long_log_in_a_file_outside_the_tracked_files_and_give_its_path(
) {
    for (role, gives) in [
        ("worker", "and the result gives its path."),
        ("advisor", "and the answer gives its path."),
        ("reviewer", "and the findings give its path."),
        ("tester", "and the result gives its path."),
    ] {
        assert_says(
            &instructions(role, &[zeus()]),
            role,
            &[&format!(
                "A long log, table or listing goes in a file outside the project's tracked files (your scratch folder, or where the brief says), {gives}"
            )],
        );
    }
    assert!(!says(
        &instructions("designer", &[zeus()]),
        "A long log, table or listing"
    ));
}

// The project's rules for its staff (spec smaller-contexts, SCX-14 and SCX-15):
// the human's own text, at the end of every window's text under its own heading,
// a member told to follow them and the chief that every member has them. What
// ConsensFlow says round them keeps the bans of its texts; the rules themselves
// are the human's, and are not held to them.

/// Rules a human might write, saying what ConsensFlow's own member texts never
/// may: they are kept as written.
const HUMANS_RULES: &str = "- Push to origin only, never to upstream.\n- Ask the human before any cf task add of your own.\n\tIndented, as written.";

/// What ConsensFlow says in a text with rules, the rules themselves taken out.
fn framing(text: &str) -> String {
    text.replacen(HUMANS_RULES, "", 1)
}

#[test]
fn tells_the_chief_every_member_has_the_projects_rules_so_a_brief_does_not_repeat_them() {
    let text = instructions("chief", &[zeus()]);
    assert_says(
        section(&text, "What you do"),
        "the chief's What you do",
        &["Every window opens with the project's rules for the staff (at the end of this text, when the project has any): a brief doesn't repeat them, and says only what its task needs."],
    );
}

#[test]
fn gives_each_member_the_projects_rules_for_the_staff_under_their_own_heading_after_its_role() {
    for role in cf_ledger::roles::ids() {
        let role_text = instructions(role, &[zeus()]);
        let text = with_staff_rules(role_text.clone(), role, Some(HUMANS_RULES));
        assert!(
            text.starts_with(&role_text),
            "{role}: the role text comes first, whole"
        );
        assert_eq!(
            &text[role_text.len()..],
            format!(
                "\n{STAFF_RULES_HEADING}\n\nEvery member of this project's staff is given these rules after its role, as you are. Follow them in every task, as you follow your role.\n\n{HUMANS_RULES}\n"
            ),
            "{role}"
        );
        assert_eq!(text.matches(STAFF_RULES_HEADING).count(), 1, "{role}");
        // The member texts' bans hold of what ConsensFlow says, not of the human's rules.
        let said = framing(&text);
        assert!(!said.contains("the human"), "{role}");
        assert!(!said.contains("cf task add"), "{role}");
        assert!(text.contains(HUMANS_RULES), "{role}: the rules as written");
    }
}

#[test]
fn shows_the_chief_the_same_rules_marked_as_what_every_member_already_has() {
    let role_text = instructions("chief", &[zeus()]);
    let text = with_staff_rules(role_text.clone(), "chief", Some(HUMANS_RULES));
    assert!(text.starts_with(&role_text));
    let rules = section(&text, "The project's rules for the staff");
    assert_says(
        rules,
        "the chief's rules for the staff",
        &[
            "Every window of your staff opens with these rules in its instructions, after its role.",
            "They are the human's, written in Project settings: to change one, propose it to the human here.",
        ],
    );
    assert!(rules.ends_with(&format!("\n\n{HUMANS_RULES}\n")));
    assert!(!found(
        r"VERDICT|review policy|cf task review|Review round",
        &framing(&text)
    ));
}

#[test]
fn a_project_with_no_rules_for_its_staff_leaves_every_text_as_it_was_to_the_byte() {
    for role in std::iter::once("chief").chain(cf_ledger::roles::ids()) {
        let role_text = instructions(role, &[zeus()]);
        assert_eq!(
            with_staff_rules(role_text.clone(), role, None),
            role_text,
            "{role}"
        );
        assert!(!role_text.contains(STAFF_RULES_HEADING), "{role}");
    }
}

/// Every role of the table has its text (spec new-roles, FD-04): its title by its name, its
/// own description and intro, and the shared rules; a row added to the table without its
/// parts in `staff.md` fails here, as the engine refuses a missing slot.
#[test]
fn every_role_of_the_table_has_its_own_text_under_its_name() {
    let mut intros = Vec::new();
    let mut descriptions = Vec::new();
    for role in cf_ledger::roles::all() {
        let text = role_instructions(&Env::default(), role.id, &[zeus()], &CfWord::bare(), None)
            .unwrap_or_else(|refused| panic!("{}: {refused}", role.id));
        assert!(
            text.contains(&format!("\n# ConsensFlow {}\n", role.name)),
            "{}: {text}",
            role.id
        );
        assert!(
            text.contains(&format!("Keep this {} role", role.id)),
            "{}",
            role.id
        );
        assert!(
            found(r"Never wait for an answer\s+in your shell", &text),
            "{}",
            role.id
        );
        let intro = text
            .split("\n# ConsensFlow ")
            .nth(1)
            .and_then(|rest| rest.split("\n## Your commands").next())
            .unwrap_or_default()
            .to_owned();
        assert!(
            !intros.contains(&intro),
            "{}: another role's intro",
            role.id
        );
        intros.push(intro);
        // The description its harness's skill lists it by is its own too.
        let description = text
            .lines()
            .find_map(|line| line.strip_prefix("description: "))
            .unwrap_or_default()
            .to_owned();
        assert!(
            !description.is_empty() && !descriptions.contains(&description),
            "{}: no description of its own: {description:?}",
            role.id
        );
        descriptions.push(description);
    }
}

/// The four roles' texts say what their rules give each (spec new-roles): the frontend
/// designer decides the look and asks for images through the chief, the architect writes
/// nothing that runs, the researcher changes no file, and the integrator changes no
/// behaviour and releases only when told so.
#[test]
fn the_four_new_roles_texts_say_their_own_territory() {
    let text = |role: &str| instructions(role, &[zeus()]);
    let frontend = text("frontend");
    for said in [
        r"The look is\s+yours to decide",
        r"Look at the project's screens, styles and components first",
        r"When you need an image, ask the chief with a drawing order",
        r"where\s+it cannot, say it was not looked at",
        r"why you chose\s+what you chose",
    ] {
        assert!(found(said, &frontend), "frontend: {said}");
    }
    let architect = text("architect");
    for said in [
        r"Nothing you\s+write runs",
        r"Read the code as it stands and the project's documents first",
        r"what that rework\s+would cost and buy",
    ] {
        assert!(found(said, &architect), "architect: {said}");
    }
    assert!(found(
        r"A research task changes no file",
        &text("researcher")
    ));
    let integrator = text("integrator");
    for said in [
        r"You change no\s+behaviour",
        r"Tag,\s+release or deploy only where the brief says so in words",
        r"showing both sides",
    ] {
        assert!(found(said, &integrator), "integrator: {said}");
    }
}
