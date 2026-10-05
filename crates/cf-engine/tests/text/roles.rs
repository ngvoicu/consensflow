//! The instructions each window the daemon opens starts with (TEST-BDC-11):
//! the board's commands for every role, and for coordinators the staff they
//! choose from, the work tiers and the review rule. Nothing from the old
//! transport. The cases of `tests/core-roles.test.mjs`, but the one that
//! writes a role's text where each harness loads it, which is the launch's
//! (`roleConfiguration`).

use std::fs;
use std::path::Path;

use cf_base::env::Env;
use cf_catalog::WorkTier;
use cf_engine::roles::{role_instructions, StaffRow};
use regex::Regex;

use super::support::staff_row;

/// The commands of the transport before the board's: no text may teach them.
const OLD_COMMANDS: &str =
    r"(?-u:\b)cf (run|say|attach|read|results|projects|chief (send|read))(?-u:\b)";

fn zeus() -> StaffRow {
    staff_row("zeus", &["worker", "reviewer"], WorkTier::Standard)
}

/// A role's text with no environment and no word of where its cf is.
fn instructions(role: &str, staff: &[StaffRow]) -> String {
    role_instructions(&Env::default(), role, staff, None).unwrap()
}

/// Whether `pattern` (a JavaScript pattern written as a Rust one) matches `text`.
fn found(pattern: &str, text: &str) -> bool {
    Regex::new(pattern).unwrap().is_match(text)
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
            "cf answer",
            "cf task done",
            "cf note --human",
            "cf staff",
            "cf history",
        ] {
            assert!(text.contains(command), "{role} learns {command}");
        }
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
                r"\| Member \| Roles \| Work tier \|\n\|---\|---\|---\|\n\| zeus \| worker, reviewer \| Standard work \|",
                &text
            ),
            "the staff table: name, roles, tier"
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
        assert!(text.contains("`cf staff` shows them as they are now"));
        // A chief that left the heredoc's word bare to fit a timestamp in ran
        // every backticked snippet of its brief (poker-lab, 2026-10-03).
        assert!(found(
            r"`<<'BRIEF'`, always; to put a value in, write\s+it out",
            &text
        ));
        assert!(found(
            r"A tier nobody on the staff holds\s+goes to the nearest one somebody does",
            &text
        ));
        assert!(text.contains("do not run the command to see the refusal: ask"));
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
            !text.contains("the human"),
            "{role} is asked and answers only through the chief"
        );
        assert!(found(
            r"(?i)never read another agent's\s+session files",
            &text
        ));
        assert!(!text.contains("cf task add"));
        assert!(
            !found(r"PM(?-u:\b)|coordinator", &text),
            "{role} answers to the chief"
        );
        if role == "reviewer" {
            assert!(text.contains("delivers them to the chief, who decides"));
        }
        assert!(
            !found(r"VERDICT|Review round", &text),
            "a review is a task: no verdict line"
        );
        if role == "advisor" {
            assert!(text.contains("You advise this project's chief"));
        }
        if role == "designer" {
            assert!(text.contains("image generation tool"));
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
fn teach_the_designer_only_the_board_s_commands() {
    teaches_only_the_boards_commands("designer");
}

#[test]
fn gives_every_member_one_shared_text_with_its_role_s_own_parts() {
    let shared = [
        "Run each of these in your shell (your Bash or terminal tool).",
        "Do not hand out tasks, launch other agents or type into other windows, and\nnever read another agent's session files: the board is your only channel.",
        "This window is for one task: it opened with the task and closes when the task\nleaves your hands.",
    ];
    for (role, title) in [
        ("worker", "worker"),
        ("advisor", "advisor"),
        ("reviewer", "reviewer"),
        ("designer", "image designer"),
    ] {
        let text = instructions(role, &[zeus()]);
        for rule in shared {
            assert!(text.contains(rule), "{role} keeps: {rule}");
        }
        assert!(found(&format!(r"\n# ConsensFlow {title}\n"), &text));
        assert!(found(
            &format!(r"Keep this\n{role} role for the whole session\.\n$"),
            &text
        ));
    }
    assert!(found(
        r"cf task get T-3 {15}a task and its whole thread: the work under review, or this one\n",
        &instructions("reviewer", &[])
    ));
    let refused = role_instructions(&Env::default(), "pm", &[], None).unwrap_err();
    assert_eq!(refused.to_string(), "no role instructions for pm");
}

#[test]
fn tells_every_window_it_never_has_to_wait_for_what_the_board_brings() {
    // A turn spent sleeping is one no result or answer can reach (Devin waited so, 2026-10-02).
    assert!(found(
        r"You never have to poll or wait for a result: ConsensFlow brings each one to\s+you as a message when it is ready",
        &instructions("chief", &[zeus()])
    ));
    for role in ["worker", "advisor", "reviewer", "designer"] {
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
fn tells_the_chief_one_member_runs_as_many_tasks_at_once_as_it_is_given() {
    assert!(found(
        r"however few\s+members a tier has: one worker runs as many tasks at once as you give it,\s+and so does one advisor or one reviewer",
        &instructions("chief", &[zeus()])
    ));
}

#[test]
fn tells_every_window_its_messages_arrive_pasted_and_are_its_to_act_on() {
    for role in ["chief", "worker", "advisor", "reviewer", "designer"] {
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
        text.contains("the next step of that window's own work"),
        "and when"
    );
    assert!(
        text.contains("reviews, parallel work and work for another role stay fresh"),
        "and when not"
    );
    assert!(instructions("chief", &[zeus()]).contains("## What you do"));
    assert!(instructions("chief", &[zeus()]).contains("## What you never do"));
    assert!(found(
        r"## What you never do\n\n- Change the project yourself: edit a file, commit, push, build or deploy",
        &instructions("chief", &[zeus()])
    ));
}

#[test]
fn tells_the_chief_a_worker_starts_fresh_unless_its_window_is_continued_and_that_only_a_busy_window_refuses(
) {
    let text = instructions("chief", &[zeus()]);
    assert!(found(
        r"of its own earlier tasks, unless\s+you continue its window with `--after`\.",
        &text
    ));
    assert!(found(
        r"A window still busy refuses\s+and tells you to open the task for its tier instead\.",
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

#[test]
fn names_this_window_s_cf_by_its_full_path_for_a_shell_that_finds_another_cf_first() {
    for role in ["chief", "worker", "advisor", "reviewer", "designer"] {
        let text =
            role_instructions(&Env::default(), role, &[], Some("/opt/consensflow/bin/cf")).unwrap();
        assert!(
            found(
                r"## This window's cf\n\nHere `cf` is /opt/consensflow/bin/cf\. If `cf` says a command is unknown, or answers as another program, another `cf` comes first on this shell's PATH: run /opt/consensflow/bin/cf instead\.\n$",
                &text
            ),
            "{role}"
        );
    }
    assert!(
        !instructions("worker", &[]).contains("This window's cf"),
        "only when the daemon says"
    );
}

#[test]
fn gives_the_chief_s_subagents_if_its_harness_has_them_nothing_beyond_searching_and_reading() {
    assert!(found(
        r"Hand your harness's subagents or task tool, if it has them, any work\s+beyond searching and reading: the board, the human and the staff never see\s+that work",
        &instructions("chief", &[])
    ));
}

#[test]
fn refuses_an_unknown_role() {
    let refused = role_instructions(&Env::default(), "king", &[], None).unwrap_err();
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
    // The personal name must not appear in anything that ships.
    for base in ["hosts", "bin", "src", "skill"] {
        for file in files_under(&root.join(base)) {
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
        role_instructions(&env, "chief", &staff, Some("/app/bin/cf")).unwrap(),
        "You work in this project for its owner.\n",
        "no tiers, no staff, no cf"
    );
    assert!(
        role_instructions(&env, "worker", &staff, Some("/app/bin/cf"))
            .unwrap()
            .contains("/app/bin/cf")
    );
    assert!(instructions("chief", &staff).contains("You do not change the project yourself"));
}
