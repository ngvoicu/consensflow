//! `cf task …`: work put on the board, read from it, and moved along it.

use std::ffi::OsString;
use std::io::Read;

use cf_base::js;
use cf_board::Board;
use serde_json::{json, Map, Value};

use super::lines::{a_pool, message_line, numbers, task_head, task_line, tasks, waiting_answers};
use super::usage::{task_usage, ADD_USAGE};
use super::words::{quoted, require_text, split, task_number, task_numbers, Shape};
use super::{brief, transcript};
use super::{help_of, text_of, Failure, Said};

/// The flags of `cf task add`.
const ADD: Shape = Shape {
    switches: &["--self", "--advice", "--review", "--test", "--design"],
    valued: &[
        "--tier",
        "--purpose",
        "--after",
        "--needs",
        "--before",
        brief::FILE,
    ],
    leading: 0,
};

/// The flags of `cf task reopen` and `cf task resume`, which give the window
/// words of their own.
const BRIEFED: Shape = Shape {
    valued: &[brief::FILE],
    ..Shape::TEXT
};

/// The flags of `cf task get`, which stand after its task.
const GET: Shape = Shape {
    switches: &["--transcript"],
    valued: &["--last", "--page", "--find"],
    leading: 0,
};

/// The commands that move a task, which stands first among their words.
const MOVES: [&str; 7] = [
    "done", "accept", "cancel", "reopen", "edit", "pause", "resume",
];

/// `cf task` with `words`, the last of those `given` spells as the system gave them.
pub fn command(
    words: &[String],
    given: &[OsString],
    board: &Board,
    input: &mut dyn Read,
) -> Result<Said, Failure> {
    let (action, rest) = match words.split_first() {
        Some((action, rest)) => (Some(action.as_str()), rest),
        None => (None, words),
    };
    match action {
        Some("help" | "--help" | "-h") => {
            let usage = task_usage();
            return Ok(Said::new(json!({ "usage": usage }), usage));
        }
        Some("add") => return add(rest, given, board, input),
        Some("list") | None => return board_list(rest, board),
        Some(_) => {}
    }
    // Asked for before the task is looked for in the words: `cf task get
    // --help` has no task to name.
    if let Some(verb) = action.filter(|action| *action == "get" || MOVES.contains(action)) {
        let shape = Shape {
            leading: 1,
            ..shape_of(verb)
        };
        if split(rest, shape).asks_for_help() {
            return Ok(help_of(&["task", verb]));
        }
    }
    let number = task_number(rest.first().map(String::as_str))?;
    match action {
        Some("get") => get(number, rest.get(1..).unwrap_or_default(), board),
        Some(action) if MOVES.contains(&action) => moved(
            action,
            number,
            rest.get(1..).unwrap_or_default(),
            given,
            board,
            input,
        ),
        other => Err(Failure::Usage(format!(
            "unknown task command {}: use add, list, get, done, accept, reopen, edit, cancel, pause or resume",
            quoted(other.unwrap_or_default())
        ))),
    }
}

/// The flags of the task command `verb`, after its task.
fn shape_of(verb: &str) -> Shape {
    match verb {
        "get" => GET,
        "reopen" | "resume" | "edit" => BRIEFED,
        _ => Shape::TEXT,
    }
}

fn add(
    rest: &[String],
    given: &[OsString],
    board: &Board,
    input: &mut dyn Read,
) -> Result<Said, Failure> {
    let words = split(rest, ADD);
    if words.asks_for_help() {
        return Ok(help_of(&["task", "add"]));
    }
    let tier = words.value("--tier");
    let after = words
        .value("--after")
        .map(|after| task_number(Some(after)))
        .transpose()?;
    let needs = task_numbers(words.value("--needs"))?;
    let before = task_numbers(words.value("--before"))?;
    let mine = words.on("--self");
    // Your own work needs no board while you are at it; on the board it is a
    // wake-up: the brief comes back to this window when what it waits for is accepted.
    if mine && needs.is_none() && before.is_none() {
        return Err(Failure::Usage(
            "you are already at it: do it now, or give it --needs T-3 to be woken when T-3 is accepted"
                .into(),
        ));
    }
    if tier.is_none() && after.is_none() && !mine && !words.on("--design") {
        return Err(Failure::Usage(ADD_USAGE.into()));
    }
    let brief = require_text(brief::brief_of(&words, given, input)?, ADD_USAGE)?;
    let sent = brief::sent(&brief);

    let mut body = Map::new();
    if mine {
        body.insert("self".into(), true.into());
    } else if let Some(after) = after {
        body.insert("after".into(), after.into());
    } else if words.on("--design") {
        body.insert("design".into(), true.into());
    } else {
        if let Some(tier) = tier {
            body.insert("tier".into(), tier.into());
        }
        if words.on("--advice") {
            body.insert("advice".into(), true.into());
        }
        if words.on("--review") {
            body.insert("review".into(), true.into());
        }
        if words.on("--test") {
            body.insert("test".into(), true.into());
        }
        if let Some(purpose) = words.value("--purpose") {
            body.insert("purpose".into(), purpose.into());
        }
    }
    if let Some(needs) = &needs {
        body.insert("needs".into(), json!(needs));
    }
    if let Some(before) = &before {
        body.insert("before".into(), json!(before));
    }
    body.insert("body".into(), brief.into());

    let created = board.post("/api/tasks", &Value::Object(body))?;
    let task = created.part("task")?;
    let number = js::text(task.get("number"));
    // With human approval required, nothing moves until the human passes it on.
    let gated = if js::truthy(created.value().get("gated")) && !mine {
        " The human approves each message before it moves."
    } else {
        ""
    };
    let blocked = created.list(task.get("blockedBy"), "blockedBy")?;
    let waits = match blocked.len() {
        0 => String::new(),
        1 => format!(" It waits until {} is accepted.", tasks(numbers(blocked))),
        _ => format!(" It waits until {} are accepted.", tasks(numbers(blocked))),
    };
    let holds = match &before {
        None => String::new(),
        Some(before) => {
            let verb = if before.len() == 1 { "waits" } else { "wait" };
            format!(" {} {verb} for it.", tasks(before))
        }
    };
    let text = if mine {
        format!(
            "T-{number} is yours; finish it with: cf task done T-{number} \"what you did\".{waits}"
        )
    } else if let Some(after) = after {
        format!(
            "T-{number} continues in @{}, the window that did T-{after}; its result arrives in your inbox.{gated}{waits}",
            js::text(task.get("assignee"))
        )
    } else {
        let pool = task.get("pool");
        let tier = match task.get("tier") {
            None | Some(Value::Null) => tier.map(Value::from),
            Some(tier) => Some(tier.clone()),
        };
        let nearest = match created.value().get("asked") {
            None => String::new(),
            asked => format!(
                " (no {} {} is on the staff, so the nearest tier)",
                js::text(asked),
                js::text(pool)
            ),
        };
        format!(
            "T-{number} is on the board for {}{nearest}; the first free one gets it, and its result arrives in your inbox.{waits}{holds}{gated}",
            a_pool(pool, tier.as_ref())
        )
    };
    Ok(Said::new(created.into_value(), format!("{text}\n{sent}")))
}

/// Whether a task no lane has waits for a member to take it: a task paused or
/// over before any member had it, or of a member who left the staff, does not.
fn waits_for_a_member(task: &Value) -> bool {
    js::text(task.get("state")) == "open" && matches!(task.get("assignee"), Some(Value::Null))
}

/// `cf task list`: what waits for a member, the other tasks no lane has, then
/// each lane's.
fn board_list(rest: &[String], board: &Board) -> Result<Said, Failure> {
    if split(rest, Shape::TEXT).asks_for_help() {
        return Ok(help_of(&["task", "list"]));
    }
    let answer = board.get("/api/tasks")?;
    let mut lines = Vec::new();
    let (waiting, apart): (Vec<&Value>, Vec<&Value>) = answer
        .list(answer.value().get("open"), "open")?
        .iter()
        .partition(|task| waits_for_a_member(task));
    for (heading, group) in [("Waiting for a member", waiting), ("With no member", apart)] {
        if !group.is_empty() {
            lines.push(heading.to_string());
            lines.extend(group.into_iter().map(task_line));
        }
    }
    for lane in answer.list(answer.value().get("lanes"), "lanes")? {
        let lane_tasks = answer.list(lane.get("tasks"), "tasks")?;
        if !lane_tasks.is_empty() {
            lines.push(format!(
                "@{} ({})",
                js::text(lane.get("handle")),
                js::text(lane.get("role"))
            ));
            lines.extend(lane_tasks.iter().map(task_line));
        }
    }
    let text = if lines.is_empty() {
        "No tasks yet.".to_string()
    } else {
        lines.join("\n")
    };
    Ok(Said::new(answer.into_value(), text))
}

/// The brief `cf task get` prints under the header, or nothing. A window that was
/// given the task holds its brief in its own task message in the thread
/// (`delivery_body` wrote it there), so it is not printed twice. A task no window
/// holds (open, blocked, or taken back to the board, whose brief may have grown
/// since its old message) gets its body here, as the board keeps it. A brief the
/// board withholds at the human's gate reads `""` and prints nothing.
fn brief_under_head<'a>(task: &'a Value, messages: &[Value]) -> Option<&'a str> {
    let brief = match task.get("body").and_then(Value::as_str) {
        Some(brief) if !brief.is_empty() => brief,
        _ => return None,
    };
    let window = task.get("assignee").and_then(Value::as_str);
    let carried = window.is_some_and(|window| {
        messages.iter().any(|message| {
            message.get("kind").and_then(Value::as_str) == Some("task")
                && message.get("recipient").and_then(Value::as_str) == Some(window)
        })
    });
    (!carried).then_some(brief)
}

/// `cf task get T-n`: the task with its brief (`brief_under_head`) and its whole
/// thread, and said of each answer in it still waiting to be pasted that it was
/// written whole; or, with `--transcript`, what its window did, which says of no
/// answer that it was written: the thread is not what was asked for there (its
/// text does not print it, and what its JSON carries of it is along the way).
/// What is wrong with the command is said before the board is asked anything.
fn get(number: u64, rest: &[String], board: &Board) -> Result<Said, Failure> {
    let words = split(rest, GET);
    let asked = if words.on("--transcript") {
        Some(transcript::asked(&words)?)
    } else {
        None
    };
    let path = format!("/api/tasks/{number}");
    let mut answer = board.get(&path)?;
    let task = answer.take("task")?;
    let Some(asked) = asked else {
        let messages = answer.list(task.get("messages"), "messages")?;
        let thread = messages
            .iter()
            .map(|message| {
                format!(
                    "{}\n{}",
                    message_line(message),
                    js::text(message.get("body"))
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let body = match brief_under_head(&task, messages) {
            Some(brief) if thread.is_empty() => brief.to_owned(),
            Some(brief) => format!("{brief}\n\n{thread}"),
            None => thread,
        };
        let text = format!("{}\n\n{body}", task_head(&task));
        let answers = waiting_answers(messages);
        return Ok(Said::new(task, text).having_written("task", answers));
    };
    let head = task_head(&task);
    transcript::read(number, task, head, &asked, board)
}

fn moved(
    action: &str,
    number: u64,
    rest: &[String],
    given: &[OsString],
    board: &Board,
    input: &mut dyn Read,
) -> Result<Said, Failure> {
    let briefed = matches!(action, "reopen" | "resume" | "edit");
    let words = split(rest, shape_of(action));
    let text = if briefed {
        brief::brief_of(&words, given, input)?
    } else {
        text_of(words.text, input)?
    };
    if matches!(action, "done" | "reopen" | "edit" | "resume") && js::trim(&text).is_empty() {
        let what = match action {
            "done" => "your result",
            "edit" => "the new brief",
            "resume" => "what to do now",
            _ => "the follow-up",
        };
        return Err(Failure::Usage(format!(
            "cf task {action} T-{number} \"{what}\""
        )));
    }
    // What the words that send a window on say of themselves.
    let sent = if briefed {
        format!("\n{}", brief::sent(&text))
    } else {
        String::new()
    };
    let mut body = Map::new();
    if !text.is_empty() {
        body.insert("body".into(), text.into());
    }
    let path = format!("/api/tasks/{number}/{action}");
    let mut answer = board.post(&path, &Value::Object(body))?;
    // What a cancel took with it of the words you told the task's window.
    let dropped = match answer.value().get("dropped") {
        Some(dropped) => went_with_it(answer.list(Some(dropped), "dropped")?),
        None => String::new(),
    };
    let task = answer.take("task")?;
    let said = match action {
        "pause" => format!(
            "T-{number} is paused: its window stops and its work waits. Resume it with: cf task resume T-{number} \"what to do now\""
        ),
        "resume" if task.get("state").and_then(Value::as_str) == Some("open") => format!(
            "T-{number} is back on the board for {}: the window that had it has ended.",
            a_pool(task.get("pool"), task.get("tier"))
        ),
        "resume" => format!("T-{number} resumes in @{} with your words.", js::text(task.get("assignee"))),
        // The chief's own step is accepted with its words: nothing comes back to decide.
        "done" if task.get("state").and_then(Value::as_str) == Some("accepted") => format!(
            "T-{number} is done and accepted: it was your own step, so nothing waits for your decision, and what needed it goes on."
        ),
        "edit" => format!(
            "T-{number} has your new brief; no window had the old one, and the ledger keeps it. {}",
            task_line(&task)
        ),
        _ => format!("{}{dropped}", task_line(&task)),
    };
    Ok(Said::new(task, format!("{said}{sent}")))
}

/// What a cancel says of the messages `ids`, the words you told the task's
/// window that it never had: nothing when there are none.
fn went_with_it(ids: &[Value]) -> String {
    let ids: Vec<String> = ids
        .iter()
        .map(|id| format!("m-{}", js::text(Some(id))))
        .collect();
    match ids.as_slice() {
        [] => String::new(),
        [one] => format!("\nWhat you told its window, {one}, went with it: it never had it."),
        several => format!(
            "\nWhat you told its window, {}, went with it: it never had them.",
            several.join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use cf_board::scripted::{reply, scripted};

    use super::*;

    fn card(number: u32, state: &str, assignee: Option<&str>) -> Value {
        json!({
            "number": number, "state": state, "assignee": assignee, "requester": "chief",
            "title": format!("Task {number}"), "blockedBy": [], "pool": "worker", "tier": "standard",
        })
    }

    /// What `cf task list` prints of a board with these open tasks and lanes.
    fn listed(open: &[Value], lanes: &Value) -> String {
        let api = scripted(vec![reply(200, json!({ "open": open, "lanes": lanes }))]);
        board_list(&[], &Board::new(Some(&api.url), "tok"))
            .unwrap()
            .text
    }

    #[test]
    fn what_waits_for_a_member_is_listed_apart_from_the_other_tasks_no_lane_has() {
        let lanes = json!([{ "handle": "zeus", "role": "worker", "tasks": [card(1, "done", Some("zeus"))] }]);
        assert_eq!(
            listed(
                &[
                    card(2, "cancelled", None),
                    card(3, "paused", None),
                    card(4, "failed", None),
                    card(5, "open", None),
                    card(9, "cancelled", Some("athena")),
                    card(13, "open", Some("athena")),
                ],
                &lanes,
            ),
            [
                "Waiting for a member",
                "T-5 [open] for a standard worker ← @chief: Task 5",
                "With no member",
                "T-2 [cancelled] for a standard worker ← @chief: Task 2",
                "T-3 [paused] for a standard worker ← @chief: Task 3",
                "T-4 [failed] for a standard worker ← @chief: Task 4",
                "T-9 [cancelled] @athena ← @chief: Task 9",
                "T-13 [open] @athena ← @chief: Task 13",
                "@zeus (worker)",
                "T-1 [done] @zeus ← @chief: Task 1",
            ]
            .join("\n")
        );
    }

    /// What `cf task add words…` posted and printed, the board answering with `answer`.
    fn added(words: &[&str], answer: &Value) -> (Option<Value>, String) {
        let api = scripted(vec![reply(201, answer.clone())]);
        let given: Vec<OsString> = words.iter().map(OsString::from).collect();
        let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
        let said = add(
            &words,
            &given,
            &Board::new(Some(&api.url), "tok"),
            &mut std::io::empty(),
        )
        .unwrap();
        let posted = api.received().into_iter().next().and_then(|got| got.json());
        (posted, said.text)
    }

    #[test]
    fn a_test_is_asked_for_a_tester_of_a_tier_and_says_whom_it_waits_for() {
        let answer = json!({
            "task": { "number": 3, "state": "open", "assignee": null, "requester": "chief",
                "title": "Run T-2", "blockedBy": [], "pool": "tester", "tier": "standard" },
            "message": null, "gated": false,
        });
        let (posted, said) = added(&["--test", "--tier", "standard", "Run T-2"], &answer);
        assert_eq!(
            posted,
            Some(json!({ "tier": "standard", "test": true, "body": "Run T-2" }))
        );
        assert_eq!(
            said,
            "T-3 is on the board for a standard tester; the first free one gets it, and its result arrives in your inbox.\nSent: 7 characters, ending \"Run T-2\"."
        );
        // Beside the others, and with none of them it is a plain task.
        let (posted, _) = added(
            &["--tier", "light", "--review", "--test", "Check it"],
            &answer,
        );
        assert_eq!(
            posted,
            Some(json!({ "tier": "light", "review": true, "test": true, "body": "Check it" })),
            "the flags are the board's to rank"
        );
        let (posted, _) = added(&["--tier", "light", "Write it"], &answer);
        assert_eq!(posted, Some(json!({ "tier": "light", "body": "Write it" })));
    }

    /// What `cf task edit words…` posted and printed, the board answering with `answer`.
    fn edited(
        words: &[&str],
        input: &str,
        answer: &Value,
    ) -> (Vec<(String, Option<Value>)>, String) {
        let api = scripted(vec![reply(200, answer.clone())]);
        let words: Vec<String> = [&["edit"], words]
            .concat()
            .iter()
            .map(|word| (*word).to_owned())
            .collect();
        let given: Vec<OsString> = words.iter().map(OsString::from).collect();
        let said = command(
            &words,
            &given,
            &Board::new(Some(&api.url), "tok"),
            &mut input.as_bytes(),
        )
        .unwrap();
        let posted = api
            .received()
            .into_iter()
            .map(|got| (got.path.clone(), got.json()))
            .collect();
        (posted, said.text)
    }

    #[test]
    fn a_new_brief_is_posted_to_the_task_and_said_to_be_the_tasks_now() {
        let answer = json!({ "task": card(2, "open", Some("zeus-pale")) });
        let (posted, said) = edited(&["T-2", "Parse the new way"], "", &answer);
        assert_eq!(
            posted,
            [(
                "/api/tasks/2/edit".to_owned(),
                Some(json!({ "body": "Parse the new way" }))
            )]
        );
        // A new brief is words that give a window work: the reply says how they end (B6).
        assert_eq!(
            said,
            "T-2 has your new brief; no window had the old one, and the ledger keeps it. T-2 [open] @zeus-pale ← @chief: Task 2\nSent: 17 characters, ending \"Parse the new way\"."
        );
        // Or on the standard input, as a heredoc gives it: its last newline is its own.
        let (posted, _) = edited(&["T-2", "-"], "Parse\nthe new way\n", &answer);
        assert_eq!(posted[0].1, Some(json!({ "body": "Parse\nthe new way" })));
    }

    /// The merge of main's `cf task edit` with B6 (T-340): a new brief is read from a
    /// file as every brief is, where a text-only `edit` posted `--file brief.md` as
    /// the brief itself.
    #[test]
    fn a_new_brief_is_read_from_a_file() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("brief.md");
        std::fs::write(&file, "Parse\nthe new way\n").unwrap();
        let answer = json!({ "task": card(2, "open", Some("zeus-pale")) });
        let name = file.to_str().unwrap();
        let (posted, said) = edited(&["T-2", "--file", name], "", &answer);
        assert_eq!(
            posted,
            [(
                "/api/tasks/2/edit".to_owned(),
                Some(json!({ "body": "Parse\nthe new way" }))
            )]
        );
        assert!(
            said.ends_with("\nSent: 17 characters, ending \"Parse the new way\"."),
            "{said}"
        );
    }

    #[test]
    fn a_new_brief_is_asked_for_before_the_board_is_asked_anything() {
        let api = scripted(vec![]);
        let words = ["edit".to_owned(), "T-2".to_owned()];
        let given: Vec<OsString> = words.iter().map(OsString::from).collect();
        let said = command(
            &words,
            &given,
            &Board::new(Some(&api.url), "tok"),
            &mut std::io::empty(),
        );
        assert!(
            matches!(&said, Err(Failure::Usage(usage)) if usage == "cf task edit T-2 \"the new brief\""),
            "a usage line was expected"
        );
        assert!(api.received().is_empty());
    }

    #[test]
    fn a_transcript_is_asked_for_as_many_items_as_the_chief_wants() {
        let task = json!({ "task": card(3, "working", Some("zeus-pale")) });
        let transcript = json!({ "items": [], "total": 0 });
        let api = scripted(vec![reply(200, task), reply(200, transcript)]);
        let words = [
            "get".to_owned(),
            "T-3".to_owned(),
            "--transcript".to_owned(),
            "--last".to_owned(),
            "30".to_owned(),
        ];
        let given: Vec<OsString> = words.iter().map(OsString::from).collect();
        command(
            &words,
            &given,
            &Board::new(Some(&api.url), "tok"),
            &mut std::io::empty(),
        )
        .unwrap();
        let paths: Vec<String> = api.received().into_iter().map(|got| got.path).collect();
        assert_eq!(paths, ["/api/tasks/3", "/api/tasks/3/transcript?last=30"]);
    }

    #[test]
    fn a_follow_up_behind_a_window_at_work_says_what_it_waits_for() {
        let answer = json!({
            "task": { "number": 5, "state": "open", "assignee": "zeus-pale", "requester": "chief",
                "title": "Tests", "blockedBy": [3], "pool": null, "tier": null },
            "message": null, "gated": false,
        });
        let (posted, said) = added(&["--after", "T-3", "Tests"], &answer);
        assert_eq!(posted, Some(json!({ "after": 3, "body": "Tests" })));
        assert_eq!(
            said,
            "T-5 continues in @zeus-pale, the window that did T-3; its result arrives in your inbox. It waits until T-3 is accepted.\nSent: 5 characters, ending \"Tests\"."
        );
    }

    #[test]
    fn a_heading_with_no_task_under_it_is_not_printed() {
        let lanes = json!([]);
        assert_eq!(
            listed(&[card(5, "open", None)], &lanes),
            "Waiting for a member\nT-5 [open] for a standard worker ← @chief: Task 5"
        );
        assert_eq!(
            listed(&[card(2, "paused", None)], &lanes),
            "With no member\nT-2 [paused] for a standard worker ← @chief: Task 2"
        );
        assert_eq!(listed(&[], &lanes), "No tasks yet.");
    }
}
