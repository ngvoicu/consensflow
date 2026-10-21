//! The four things a member is asked to do once its window was opened again, each
//! with what proves it did it, and the way a message gets into the window as the
//! daemon gets it in: pasted, with Enter once the window has drawn it; if the
//! window's record does not show it in ten seconds and the window has printed
//! nothing for three, Enter once more (whatever the window shows: "a quiet dialog
//! may take it", the daemon's own words); and if the record has not shown it in a
//! minute, pasted again. (`crates/cf-engine/src/arrival.rs`: the daemon pastes again
//! at the window's next rest, after a minute of rest; here the window's rest is not
//! waited for, and a window that is held is exactly the one that has none.)
//!
//! What proves a call ran is what it leaves where a model's word does not reach: a
//! file the shell wrote, a file the writing tool made, a file the editing tool
//! changed, and a line in the log of the stand-in `cf` that the shell ran.

use std::path::Path;
use std::time::Duration;

use cf_e2e::devin_store::Message;
use cf_e2e::live::members::squash;
use cf_e2e::live::{ask as harness, calls};
use cf_e2e::{files, Result};
use serde_json::Value;

use crate::again::Again;
use crate::drive::wire_of;
use crate::outcome::Outcome;
use crate::scene::{Scene, QUIET_MS};
use crate::tools::{called, tools_after, Called};
use crate::turn::Turn;
use crate::words::{delivery, marker_of};

/// The task every message of the check is about.
pub const TASK: u32 = 1;

/// How long an ask has to be answered.
pub const ASKED: Duration = Duration::from_secs(180);

/// How long after a paste the daemon presses Enter once more for a message its
/// record has not shown, until when, and when it pastes the message again.
const ENTER_AGAIN: Duration = Duration::from_secs(10);
const REPASTE: Duration = Duration::from_secs(60);

/// The key Enter sends.
const ENTER: u8 = 13;

/// What a member is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Write a file with the shell.
    Shell,
    /// Make a file with the tool that writes files.
    Write,
    /// Change a file with the tool that edits them.
    Edit,
    /// Run `cf ask`, with the shell.
    Cf,
}

/// Where the asks leave what proves them: the window's folder, the folder of the
/// stand-in `cf`, and the tag that makes this run's words its own.
#[derive(Debug, Clone, Copy)]
pub struct Place<'a> {
    pub folder: &'a Path,
    pub calls: &'a Path,
    pub tag: &'a str,
}

/// What proves a call ran, or that it did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proof {
    pub ran: bool,
    pub saw: String,
}

impl Ask {
    pub const ALL: [Ask; 4] = [Ask::Shell, Ask::Write, Ask::Edit, Ask::Cf];

    pub const fn name(self) -> &'static str {
        match self {
            Ask::Shell => "shell",
            Ask::Write => "write",
            Ask::Edit => "edit",
            Ask::Cf => "cf",
        }
    }

    /// The words of the ask, which name no permission, so that nothing in them is
    /// mistaken for a dialog's.
    pub fn body(self, tag: &str) -> String {
        match self {
            Ask::Shell => format!(
                "Run this exact command with your shell tool: echo ran-{tag} > shell-{tag}.txt\nThen reply with exactly one line: SHELL_DONE"
            ),
            Ask::Write => format!(
                "Create the file write-{tag}.txt with your file-writing tool (not your shell), holding exactly: wrote-{tag}\nThen reply with exactly one line: WRITE_DONE"
            ),
            Ask::Edit => format!(
                "With your file-editing tool (not your shell), change the word beta to edited-{tag} in the file edit-{tag}.txt\nThen reply with exactly one line: EDIT_DONE"
            ),
            Ask::Cf => format!(
                "Run this exact command with your shell tool: cf ask which-{tag}\nThen reply with exactly one line: CF_DONE"
            ),
        }
    }

    /// What the ask needs in the folder: the file the edit changes.
    pub fn prepare(self, place: &Place) -> Result<()> {
        if self == Ask::Edit {
            files::write(
                &place.folder.join(format!("edit-{}.txt", place.tag)),
                "alpha beta gamma\n",
            )?;
        }
        Ok(())
    }

    /// Whether the call ran, by what it left; and for `cf`, by what Devin's record
    /// (`messages`) has of it where the stand-in has no line: a shell that reads
    /// the user's profile again may find another `cf` first, which ran the call all
    /// the same.
    pub fn proof(self, place: &Place, messages: &[Message]) -> Proof {
        let tag = place.tag;
        match self {
            Ask::Shell => holds(
                place.folder,
                &format!("shell-{tag}.txt"),
                &format!("ran-{tag}"),
            ),
            Ask::Write => holds(
                place.folder,
                &format!("write-{tag}.txt"),
                &format!("wrote-{tag}"),
            ),
            Ask::Edit => holds(
                place.folder,
                &format!("edit-{tag}.txt"),
                &format!("edited-{tag}"),
            ),
            Ask::Cf => {
                let wanted = format!("ask which-{tag}");
                let found = calls::calls(place.calls)
                    .into_iter()
                    .any(|call| call.words == wanted);
                if found {
                    return Proof {
                        ran: true,
                        saw: format!("the stand-in cf was run with \"{wanted}\""),
                    };
                }
                match called(messages, &format!("which-{tag}")) {
                    Some(Called::Answered(said)) => Proof {
                        ran: true,
                        saw: format!(
                            "the stand-in cf has no line of it, but Devin's record has the call answered ({said}): another cf was found first by its shell"
                        ),
                    },
                    _ => Proof {
                        ran: false,
                        saw: format!("the stand-in cf was never run with \"{wanted}\""),
                    },
                }
            }
        }
    }
}

/// Whether the file `name` of `folder` holds `wanted`.
pub fn holds(folder: &Path, name: &str, wanted: &str) -> Proof {
    match files::read_string(&folder.join(name)) {
        Ok(text) if text.contains(wanted) => Proof {
            ran: true,
            saw: format!("{name} holds \"{wanted}\""),
        },
        Ok(text) => Proof {
            ran: false,
            saw: format!("{name} holds \"{}\", not \"{wanted}\"", squash(&text, 60)),
        },
        Err(_) => Proof {
            ran: false,
            saw: format!("{name} is not there"),
        },
    }
}

/// Whether the daemon would press Enter once more by now, for a message its record
/// has not shown: after ten seconds, once, and no more once it pastes again.
fn enter_due(waited: Duration, entered: bool) -> bool {
    !entered && waited > ENTER_AGAIN && waited <= REPASTE
}

/// Whether the window is one the daemon presses that Enter into: the pane host
/// answered, nothing is typed in it and unsent, and it has printed nothing for a while.
fn enter_ready(snapshot: Option<&Value>) -> bool {
    snapshot.is_some_and(|snapshot| {
        snapshot["ok"] == true
            && snapshot["unsent"].as_str().is_none_or(str::is_empty)
            && snapshot["outputQuietMs"]
                .as_f64()
                .is_some_and(|quiet| quiet >= QUIET_MS)
    })
}

/// Gives the window the message `number`, whose text as the window takes it is
/// `text`, as the daemon gives one to a window that is open, and waits up to `within`
/// for its turn to end: how it went. `between` is done at each look, for what a case
/// does to the window besides.
pub fn deliver(
    scene: &mut Scene,
    number: u32,
    text: &str,
    within: Duration,
    mut between: impl FnMut(&mut Scene) -> Result<()>,
) -> Result<Turn> {
    let pasted = format!("paste m-{number} ({} chars)", text.chars().count());
    scene.paste(text, &pasted)?;
    let (mut entered, mut again) = (false, false);
    scene.wait_turn(&marker_of(number), within, |scene, waited, arrived| {
        between(scene)?;
        if arrived {
            return Ok(());
        }
        if enter_due(waited, entered) && enter_ready(scene.snapshot().as_ref()) {
            scene.press(&[ENTER], format!("Enter again for m-{number}"))?;
            entered = true;
        }
        if waited >= REPASTE && !again {
            scene.paste(text, format!("paste m-{number} again"))?;
            again = true;
        }
        Ok(())
    })
}

/// Asks the window the `ask` as message `number` ([`deliver`]): what came of it. A
/// turn that does not end is the result, not a failure, and the window's screen is in
/// it. A window that goes on with the task it was cut in, and runs the long command
/// again in the turn of the ask, is let run it to its end, and its turn goes on to the
/// ask ([`Again`]); it is said to have.
pub fn put(scene: &mut Scene, place: &Place, ask: Ask, number: u32) -> Result<Outcome> {
    ask.prepare(place)?;
    let body = delivery(number, TASK, "note", "@chief", &ask.body(place.tag));
    let text = scene.window.given(&body)?;
    let (keys, seen, said) = (
        scene.trail.len(),
        scene.watch.sightings.len(),
        scene.watch.marks.len(),
    );
    let mut again = Again::default();
    let turn = deliver(scene, number, &text, ASKED, |scene| again.look(scene))?;
    let messages = scene.watch.messages(scene.window);
    // The daemon reads a paste its TUI put to its agent by the wire's prompt whose text
    // begins with the whole paste: Devin's own prompt of it must (the reviews of
    // 2026-10-18, which made the rule that strict).
    let took = match wire_of(scene.window.argv()).filter(|_| turn.arrived) {
        Some(wire) => Some(harness::devin_took(&wire, &text)?),
        None => None,
    };
    Ok(Outcome {
        ask,
        number,
        turn,
        proof: ask.proof(place, &messages),
        tools: tools_after(&messages, &marker_of(number)),
        sightings: scene.watch.sightings[seen..].to_vec(),
        keys: scene.trail.since(keys),
        states: scene.watch.marks.since(said),
        again: again.said(),
        took,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn place<'a>(folder: &'a Path, calls: &'a Path) -> Place<'a> {
        Place {
            folder,
            calls,
            tag: "ab12",
        }
    }

    #[test]
    fn each_ask_names_the_tool_it_is_to_use_and_what_it_leaves_and_no_word_of_a_permission() {
        let said: Vec<String> = Ask::ALL.iter().map(|ask| ask.body("ab12")).collect();
        assert_eq!(
            said[0],
            "Run this exact command with your shell tool: echo ran-ab12 > shell-ab12.txt\nThen reply with exactly one line: SHELL_DONE"
        );
        assert!(
            said[1].contains("file-writing tool (not your shell)")
                && said[1].contains("wrote-ab12")
        );
        assert!(
            said[2].contains("file-editing tool (not your shell)")
                && said[2].contains("edit-ab12.txt")
        );
        assert!(said[3].contains("cf ask which-ab12"));
        for body in &said {
            let lower = body.to_lowercase();
            for word in [
                "permission",
                "allow",
                "approv",
                "skip",
                "reject",
                "deny",
                "denied",
            ] {
                assert!(!lower.contains(word), "{word} is in {body}");
            }
        }
        let names: Vec<&str> = Ask::ALL.iter().map(|ask| ask.name()).collect();
        assert_eq!(names, ["shell", "write", "edit", "cf"]);
    }

    #[test]
    fn a_file_proves_the_call_that_made_it_and_a_file_that_is_not_there_or_not_changed_proves_none()
    {
        let folder = tempfile::tempdir().unwrap();
        let calls = tempfile::tempdir().unwrap();
        let place = place(folder.path(), calls.path());
        assert_eq!(
            Ask::Shell.proof(&place, &[]),
            Proof {
                ran: false,
                saw: "shell-ab12.txt is not there".to_owned()
            }
        );
        files::write(&folder.path().join("shell-ab12.txt"), "ran-ab12\n").unwrap();
        assert_eq!(
            Ask::Shell.proof(&place, &[]),
            Proof {
                ran: true,
                saw: "shell-ab12.txt holds \"ran-ab12\"".to_owned()
            }
        );
        // The edit's file is made by the ask's preparation, and is not changed yet.
        Ask::Edit.prepare(&place).unwrap();
        assert_eq!(
            Ask::Edit.proof(&place, &[]),
            Proof {
                ran: false,
                saw: "edit-ab12.txt holds \"alpha beta gamma\", not \"edited-ab12\"".to_owned()
            }
        );
        files::write(
            &folder.path().join("edit-ab12.txt"),
            "alpha edited-ab12 gamma\n",
        )
        .unwrap();
        assert!(Ask::Edit.proof(&place, &[]).ran);
        // Nothing but the edit's file is prepared.
        Ask::Write.prepare(&place).unwrap();
        assert!(!folder.path().join("write-ab12.txt").exists());
        files::write(&folder.path().join("write-ab12.txt"), "wrote-ab12").unwrap();
        assert!(Ask::Write.proof(&place, &[]).ran);
    }

    /// A call of the shell and what answered it, as Devin's record has them.
    fn recorded(command: &str, answer: &str) -> Vec<Message> {
        let message = |id: &str, role: &str, raw: Value| {
            let mut raw = raw;
            raw["message_id"] = json!(id);
            raw["role"] = json!(role);
            Message {
                id: id.to_owned(),
                role: role.to_owned(),
                text: String::new(),
                raw,
            }
        };
        vec![
            message(
                "a",
                "assistant",
                json!({ "tool_calls": [{ "id": "c1", "name": "run_command",
                                         "arguments": { "command": command } }] }),
            ),
            message(
                "t",
                "tool",
                json!({ "tool_call_id": "c1", "content": answer }),
            ),
        ]
    }

    #[test]
    fn the_stand_in_cf_proves_a_call_by_its_log_and_a_call_of_other_words_proves_none() {
        let folder = tempfile::tempdir().unwrap();
        let calls = tempfile::tempdir().unwrap();
        let place = place(folder.path(), calls.path());
        assert!(!Ask::Cf.proof(&place, &[]).ran);
        files::write(
            &calls.path().join(calls::LOG),
            "5 inbox\n6 ask something else\n",
        )
        .unwrap();
        assert_eq!(
            Ask::Cf.proof(&place, &[]).saw,
            "the stand-in cf was never run with \"ask which-ab12\""
        );
        files::write(
            &calls.path().join(calls::LOG),
            "5 inbox\n7 ask which-ab12\n",
        )
        .unwrap();
        assert_eq!(
            Ask::Cf.proof(&place, &[]),
            Proof {
                ran: true,
                saw: "the stand-in cf was run with \"ask which-ab12\"".to_owned()
            }
        );
    }

    #[test]
    fn a_cf_the_shell_found_before_the_stand_in_is_a_call_that_ran_and_a_call_that_was_refused_is_not(
    ) {
        let folder = tempfile::tempdir().unwrap();
        let calls = tempfile::tempdir().unwrap();
        let place = place(folder.path(), calls.path());
        // The stand-in has no line, and Devin's record has the call answered.
        let ran = recorded("cf ask which-ab12", "no daemon: not in a window");
        let proof = Ask::Cf.proof(&place, &ran);
        assert!(proof.ran, "{proof:?}");
        assert!(
            proof
                .saw
                .contains("another cf was found first by its shell")
                && proof.saw.contains("no daemon: not in a window"),
            "{proof:?}"
        );
        // A call that was refused, or never answered, or of another word, ran nothing.
        for (messages, said) in [
            (
                recorded(
                    "cf ask which-ab12",
                    "Tool execution was rejected: User skipped this tool call",
                ),
                "refused",
            ),
            (recorded("cf ask which-ffff", "answered"), "another call"),
            (Vec::new(), "none"),
        ] {
            let proof = Ask::Cf.proof(&place, &messages);
            assert!(!proof.ran, "{said}: {proof:?}");
        }
    }

    #[test]
    fn the_daemon_presses_enter_once_more_after_ten_seconds_into_a_quiet_window_with_nothing_unsent(
    ) {
        let secs = Duration::from_secs;
        assert!(
            !enter_due(secs(10), false),
            "not yet: ten seconds, not past them"
        );
        assert!(enter_due(secs(11), false));
        assert!(!enter_due(secs(11), true), "once");
        assert!(
            !enter_due(secs(61), false),
            "a minute on it is pasted again"
        );
        let quiet = json!({ "ok": true, "outputQuietMs": 3500, "unsent": "" });
        assert!(enter_ready(Some(&quiet)));
        assert!(enter_ready(Some(
            &json!({ "ok": true, "outputQuietMs": 3000, "unsent": null })
        )));
        // Printing, never printed, text typed and unsent, or no answer: left be.
        for held in [
            json!({ "ok": true, "outputQuietMs": 2900, "unsent": "" }),
            json!({ "ok": true, "outputQuietMs": null, "unsent": "" }),
            json!({ "ok": true, "outputQuietMs": 9000, "unsent": "typed" }),
            json!({ "ok": false, "outputQuietMs": 9000, "unsent": "" }),
        ] {
            assert!(!enter_ready(Some(&held)), "{held}");
        }
        assert!(!enter_ready(None));
    }
}
