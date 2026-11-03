//! `cf task …`: work put on the board, read from it, and moved along it.

use std::ffi::OsString;
use std::io::Read;

use cf_base::js;
use cf_board::Board;
use serde_json::{json, Map, Value};

use super::lines::{
    a_pool, message_line, numbers, printed_body, task_head, task_line, tasks, waiting_answers,
};
use super::usage::{task_usage, ADD_USAGE};
use super::words::{quoted, require_text, split, task_number, task_numbers, Shape, Split};
use super::{brief, transcript};
use super::{help_of, Failure, Said};

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

/// The flag that gives a task a new title: the brief's first line otherwise.
const TITLE: &str = "--title";

/// The flags of the other commands that move a task: their words may come from a file.
const FILED: Shape = Shape {
    valued: &[brief::FILE],
    ..Shape::TEXT
};

/// The flags of `cf task reopen` and `cf task resume`, which give the window words of their
/// own, and a new title if they like.
const BRIEFED: Shape = Shape {
    valued: &[brief::FILE, TITLE],
    ..Shape::TEXT
};

/// The flags of `cf task edit`: a brief and a title as any other, and the tier, the tasks it
/// needs and the tasks that wait for it, which may stand alone.
const EDIT: Shape = Shape {
    valued: &[brief::FILE, TITLE, "--tier", "--needs", "--before"],
    ..Shape::TEXT
};

/// The flags of `cf task get`, which stand after its task.
const GET: Shape = Shape {
    switches: &["--transcript"],
    valued: &["--last", "--page", "--find"],
    leading: 0,
};

/// The commands that move a task, which stands first among their words.
const MOVES: [&str; 8] = [
    "done", "accept", "cancel", "reopen", "edit", "pause", "resume", "finish",
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
            return Ok(Said::usage(task_usage()));
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
            "unknown task command {}: use add, list, get, done, accept, reopen, edit, cancel, pause, resume or finish",
            quoted(other.unwrap_or_default())
        ))),
    }
}

/// The flags of the task command `verb`, after its task. Every command that
/// moves a task takes words, so every one takes a file of them too.
fn shape_of(verb: &str) -> Shape {
    match verb {
        "get" => GET,
        "edit" => EDIT,
        "reopen" | "resume" => BRIEFED,
        _ => FILED,
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
    let tier = words.value("--tier")?;
    let after = words
        .value("--after")?
        .map(|after| task_number(Some(after)))
        .transpose()?;
    let needs = task_numbers(words.each("--needs"))?;
    let before = task_numbers(words.each("--before"))?;
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
        if let Some(purpose) = words.value("--purpose")? {
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
            "T-{number} is yours; finish it with: {} task done T-{number} \"what you did\".{waits}",
            board.cf()
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
                " (no {} {} is on the staff, so the nearest tier{})",
                js::text(asked),
                js::text(pool),
                held_by(created.value())
            ),
        };
        format!(
            "T-{number} is on the board for {}{nearest}; it starts in a new window of one, and its result arrives in your inbox.{waits}{holds}{gated}",
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

/// The states of a task that is over: nothing is left to do on it, by its
/// member or by the chief. A failed task is not over: the chief reopens it,
/// cancels it or adds it again.
const OVER: [&str; 2] = ["accepted", "cancelled"];

/// The flags of `cf task list`.
const LIST: Shape = Shape {
    switches: &["--all"],
    valued: &[],
    leading: 0,
};

/// Whether a lane is an idle session: it has tasks on the board, and every
/// one is over. The chief's lane never is, and a lane with no task, which the
/// list never prints, is none either.
fn is_idle(lane: &Value, tasks: &[Value]) -> bool {
    js::text(lane.get("role")) != "chief"
        && !tasks.is_empty()
        && tasks
            .iter()
            .all(|task| OVER.contains(&js::text(task.get("state")).as_ref()))
}

/// `data`, the board's answer, with its `lanes` as given and the idle ones
/// under `idle`, which stands right after them.
fn idle_apart(data: Value, lanes: Vec<Value>, idle: Vec<Value>) -> Value {
    let Value::Object(fields) = data else {
        return data;
    };
    let mut apart = Map::new();
    for (key, value) in fields {
        let lanes_key = key == "lanes";
        apart.insert(
            key,
            if lanes_key {
                Value::Array(lanes.clone())
            } else {
                value
            },
        );
        if lanes_key {
            apart.insert("idle".to_owned(), Value::Array(idle.clone()));
        }
    }
    Value::Object(apart)
}

/// `cf task list`: what waits for a member, the other tasks no lane has, then
/// the lanes with something to look at, and how many idle sessions there are
/// besides; `--all` lists those too, as the board always did.
fn board_list(rest: &[String], board: &Board) -> Result<Said, Failure> {
    let words = split(rest, LIST);
    if words.asks_for_help() {
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
    let (mut kept, mut idle) = (Vec::new(), Vec::new());
    for lane in answer.list(answer.value().get("lanes"), "lanes")? {
        let lane_tasks = answer.list(lane.get("tasks"), "tasks")?;
        if !words.on("--all") && is_idle(lane, lane_tasks) {
            idle.push(lane);
            continue;
        }
        kept.push(lane);
        if !lane_tasks.is_empty() {
            lines.push(format!(
                "@{} ({})",
                js::text(lane.get("handle")),
                js::text(lane.get("role"))
            ));
            lines.extend(lane_tasks.iter().map(task_line));
        }
    }
    if !idle.is_empty() {
        lines.push(format!(
            "and {} idle session{} (`{} task list --all`)",
            idle.len(),
            if idle.len() == 1 { "" } else { "s" },
            board.cf()
        ));
    }
    let text = if lines.is_empty() {
        "No tasks yet.".to_string()
    } else {
        lines.join("\n")
    };
    let data = if words.on("--all") {
        answer.into_value()
    } else {
        let (kept, idle) = (
            kept.into_iter().cloned().collect(),
            idle.into_iter().cloned().collect(),
        );
        idle_apart(answer.into_value(), kept, idle)
    };
    Ok(Said::new(data, text))
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
                    printed_body(message, board.cf())
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
    // `cf task edit T-3 --tier light` changes the tier alone: its brief stays.
    let tier = words
        .value("--tier")?
        .filter(|_| action == "edit")
        .map(str::to_owned);
    let needs = links_of("--needs", &words, action)?;
    let before = links_of("--before", &words, action)?;
    let alone = tier.is_some() || needs.is_some() || before.is_some();
    let title = new_title(&words)?;
    let text = brief::brief_of(&words, given, input)?;
    // An edit with a title and nothing else changes the title alone.
    let retitling = action == "edit" && title.is_some() && js::trim(&text).is_empty() && !alone;
    if matches!(action, "done" | "reopen" | "edit" | "resume")
        && js::trim(&text).is_empty()
        && (action != "edit" || !alone)
        && !retitling
    {
        let what = match action {
            "done" => "your result",
            "edit" => "the new brief",
            "resume" => "what to do now",
            _ => "the follow-up",
        };
        return Err(Failure::Usage(match action {
            "edit" => format!(
                "cf task edit T-{number} \"{what}\", or --tier <tier>, --needs T-4 or --before T-9"
            ),
            _ => format!("cf task {action} T-{number} \"{what}\""),
        }));
    }
    // What the words that send a window on say of themselves.
    let sent = if briefed && !text.is_empty() {
        format!("\n{}", brief::sent(&text))
    } else {
        String::new()
    };
    let mut body = Map::new();
    let has_brief = !text.is_empty();
    if has_brief {
        body.insert("body".into(), text.into());
    }
    if let Some(tier) = &tier {
        body.insert("tier".into(), tier.as_str().into());
    }
    if let Some(needs) = &needs {
        body.insert("needs".into(), needs.clone());
    }
    if let Some(before) = &before {
        body.insert("before".into(), before.clone());
    }
    if let Some(title) = &title {
        body.insert("title".into(), title.as_str().into());
    }
    let path = format!("/api/tasks/{number}/{action}");
    let mut answer = board.post(&path, &Value::Object(body))?;
    // What a cancel took with it of the words you told the task's window.
    let dropped = match answer.value().get("dropped") {
        Some(dropped) => went_with_it(answer.list(Some(dropped), "dropped")?),
        None => String::new(),
    };
    let task = answer.take("task")?;
    // A resume's line names no title, so a new one is said.
    let titled = title.as_ref().map_or_else(String::new, |title| {
        format!(" Its title is now {}.", quoted(title))
    });
    let said = match action {
        "pause" => format!(
            "T-{number} is paused: its window stops and its work waits. Resume it with: {} task resume T-{number} \"what to do now\"",
            board.cf()
        ),
        "resume" if task.get("state").and_then(Value::as_str) == Some("open") => format!(
            "T-{number} is back on the board for {}: the window that had it has ended.{titled}",
            a_pool(task.get("pool"), task.get("tier"))
        ),
        "resume" => format!("T-{number} resumes in @{} with your words.{titled}", js::text(task.get("assignee"))),
        "finish" => {
            let count = |field: &str| answer.value().get(field).and_then(Value::as_u64);
            finishing(number, &task, count("jobs"), count("agents"))
        }
        // The chief's own step is accepted with its words: nothing comes back to decide.
        "done" if task.get("state").and_then(Value::as_str) == Some("accepted") => format!(
            "T-{number} is done and accepted: it was your own step, so nothing waits for your decision, and what needed it goes on.{}",
            on_its_way(answer.value())
        ),
        "edit" if retitling => format!(
            "T-{number} has the title {}; its brief is as it was, and the ledger keeps the old title. {}",
            quoted(title.as_deref().unwrap_or_default()),
            task_line(&task)
        ),
        "edit" => edited(
            number,
            &task,
            answer.value(),
            Changed {
                brief: has_brief,
                tier: tier.is_some(),
                needs: needs.is_some(),
                before: before.is_some(),
            },
        ),
        _ => format!("{}{dropped}", task_line(&task)),
    };
    Ok(Said::new(task, format!("{said}{sent}")))
}

/// The members of the tier a task went to when nobody held the one asked, as `: @anubis`.
fn held_by(answered: &Value) -> String {
    answered
        .get("heldBy")
        .and_then(Value::as_array)
        .map(|held| {
            held.iter()
                .map(|handle| format!("@{}", js::text(Some(handle))))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|held| !held.is_empty())
        .map_or_else(String::new, |held| format!(": {held}"))
}

/// What `cf task edit` was given to change.
struct Changed {
    brief: bool,
    tier: bool,
    needs: bool,
    before: bool,
}

/// The task numbers of `list` as `T-3, T-4`, or `nothing`.
fn numbers_said(list: Option<&Value>) -> String {
    // A task's needs are `{ number, state }`; the tasks that wait for it are numbers.
    let numbers: Vec<String> = list
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|item| format!("T-{}", js::text(Some(item.get("number").unwrap_or(item)))))
                .collect()
        })
        .unwrap_or_default();
    if numbers.is_empty() {
        "nothing".to_owned()
    } else {
        numbers.join(", ")
    }
}

/// `--needs` or `--before` read as the list it changes: `T-3,T-4` replaces what the task has,
/// `+T-5,-T-6` adds and removes. Only `cf task edit` takes them.
fn links_of(
    flag: &str,
    words: &super::words::Split,
    action: &str,
) -> Result<Option<Value>, Failure> {
    let Some(word) = words.value(flag)?.filter(|_| action == "edit") else {
        return Ok(None);
    };
    let parts: Vec<&str> = word.split(',').map(js::trim).collect();
    let signed = parts
        .iter()
        .filter(|part| part.starts_with(['+', '-']))
        .count();
    let number = |part: &str| task_number(Some(part.trim_start_matches(['+', '-'])));
    if signed == 0 {
        let set: Result<Vec<u64>, Failure> = parts.iter().map(|part| number(part)).collect();
        return Ok(Some(json!(set?)));
    }
    if signed < parts.len() {
        return Err(Failure::Usage(format!(
            "{flag} takes T-3,T-4 to replace the list, or +T-5,-T-6 to add and remove: not both in one"
        )));
    }
    let (mut add, mut remove) = (Vec::new(), Vec::new());
    for part in parts {
        match part.starts_with('+') {
            true => add.push(number(part)?),
            false => remove.push(number(part)?),
        }
    }
    Ok(Some(json!({ "add": add, "remove": remove })))
}

/// What `cf task edit` says: the brief is the task's now, the ledger keeps the old; the task is
/// for another tier (the nearest one somebody holds, when nobody holds the tier asked, and who
/// holds it); it needs other tasks now, and other tasks wait for it; and no window had it.
fn edited(number: u64, task: &Value, answered: &Value, changed: Changed) -> String {
    let now = a_pool(task.get("pool"), task.get("tier"));
    let nearest = match answered.get("asked") {
        None => String::new(),
        asked => format!(
            " (no {} {} is on the staff, so the nearest tier{})",
            js::text(asked),
            js::text(task.get("pool")),
            held_by(answered)
        ),
    };
    let mut parts = Vec::new();
    if changed.brief {
        parts.push("has your new brief".to_owned());
    }
    if changed.tier {
        parts.push(format!("is for {now}{nearest}"));
    }
    if changed.needs {
        parts.push(format!("needs {}", numbers_said(task.get("needs"))));
    }
    if changed.before {
        parts.push(format!(
            "is waited for by {}",
            numbers_said(answered.get("waiting"))
        ));
    }
    let last = parts.pop().unwrap_or_default();
    let lead = match parts.is_empty() {
        true => last,
        false => format!("{} and {last}", parts.join(", ")),
    };
    let tail = if changed.brief {
        "; no window had the old one, and the ledger keeps it."
    } else if changed.tier {
        "; no window had it."
    } else {
        "."
    };
    format!("T-{number} {lead}{tail} {}", task_line(task))
}

/// The title `--title` gives, if it is given: it takes a title, and the title says something.
fn new_title(words: &Split) -> Result<Option<String>, Failure> {
    if !words.given(TITLE) {
        return Ok(None);
    }
    match words.value(TITLE)? {
        Some(title) if !js::trim(title).is_empty() => Ok(Some(title.to_owned())),
        _ => Err(Failure::Usage(format!(
            "{TITLE} takes the new title: {TITLE} \"…\""
        ))),
    }
}

/// What `cf task finish` says: the result is recorded, and nothing is ended. The `jobs` and
/// `agents` the worker's harness still counted are said to run on until its window closes,
/// or past it where the host cannot tell what is the window's (on Windows a program runs
/// before its Job Object holds it, and on the Mac what left the window's session is not
/// found, so the close does not end all of it).
fn finishing(number: u64, task: &Value, jobs: Option<u64>, agents: Option<u64>) -> String {
    let who = js::text(task.get("assignee"));
    let (jobs, agents) = (jobs.unwrap_or(0), agents.unwrap_or(0));
    let plural = |count: u64, one: &str, many: &str| match count {
        1 => format!("1 {one}"),
        _ => format!("{count} {many}"),
    };
    let jobs_said = plural(jobs, "background job", "background jobs");
    let agents_said = plural(
        agents,
        "background agent or workflow",
        "background agents or workflows",
    );
    let left = match (jobs, agents) {
        (0, 0) => return format!(
            "T-{number} is finished: what @{who} said last is its result, which reaches your inbox. Finishing ends nothing its turn left running."
        ),
        (_, 0) => jobs_said,
        (0, _) => agents_said,
        _ => format!("{jobs_said} and {agents_said}"),
    };
    let they = if jobs + agents == 1 { "it" } else { "they" };
    format!(
        "T-{number} is finished: what @{who} said last is its result, which reaches your inbox. Finishing does not end what its turn left running ({left}): {they} may run on until its window closes, or past it where the host cannot tell what is the window's."
    )
}

/// What the board says of the chief's own step it finished, as a sentence to
/// add: what was still on its way to the chief and was withdrawn (its brief, and
/// any answer or note carried on it), and its brief if it was being typed in,
/// which may still land once. Nothing when the answer names neither.
fn on_its_way(said: &Value) -> String {
    let mut sentences = String::new();
    if let Some(withdrawn) = named(said.get("withdrawn")) {
        sentences.push_str(&format!(
            " Still on its way to you, and now withdrawn: {withdrawn}."
        ));
    }
    if let Some(landing) = named(said.get("landing")) {
        sentences.push_str(&format!(
            " Being typed into your window as you finished: {landing}; it may still land once, and is not sent again."
        ));
    }
    sentences
}

/// The messages in a JSON list as a sentence names them ("the brief m-3, a note
/// m-5 and an answer m-6"); none when there is no list or it is empty.
fn named(messages: Option<&Value>) -> Option<String> {
    let names: Vec<String> = messages?
        .as_array()?
        .iter()
        .map(|message| {
            let kind = match message.get("kind").and_then(Value::as_str) {
                Some("task") => "the brief",
                Some("answer") => "an answer",
                Some("note") => "a note",
                _ => "a message",
            };
            format!("{kind} m-{}", js::text(message.get("id")))
        })
        .collect();
    let (last, rest) = names.split_last()?;
    Some(if rest.is_empty() {
        last.clone()
    } else {
        format!("{} and {last}", rest.join(", "))
    })
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
    use cf_board::scripted::{reply, scripted, Reply, ScriptedApi};

    use super::*;

    fn card(number: u32, state: &str, assignee: Option<&str>) -> Value {
        json!({
            "number": number, "state": state, "assignee": assignee, "requester": "chief",
            "title": format!("Task {number}"), "blockedBy": [], "pool": "worker", "tier": "standard",
        })
    }

    /// What `cf task list words…` said of a board that answered `answer`.
    fn said(words: &[&str], answer: &Value) -> Said {
        let api = scripted(vec![reply(200, answer.clone())]);
        let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
        board_list(&words, &Board::new(Some(&api.url), "tok")).unwrap()
    }

    /// What `cf task list` prints of a board with these open tasks and lanes.
    fn listed(open: &[Value], lanes: &Value) -> String {
        said(&[], &json!({ "open": open, "lanes": lanes })).text
    }

    fn lane(handle: &str, role: &str, tasks: &[Value]) -> Value {
        json!({ "handle": handle, "role": role, "tasks": tasks })
    }

    /// A board with members at work in every state that needs a look, the
    /// chief's lane, a session with nothing on it, and idle sessions: the
    /// ones whose tasks are all over.
    fn a_board_with_idle_sessions() -> Value {
        json!({
            "open": [card(10, "open", None)],
            "lanes": [
                lane("chief", "chief", &[card(1, "accepted", Some("chief"))]),
                lane("zeus", "worker", &[card(2, "accepted", Some("zeus")), card(9, "working", Some("zeus"))]),
                lane("hera", "reviewer", &[card(3, "accepted", Some("hera"))]),
                lane("athena", "worker", &[card(4, "cancelled", Some("athena")), card(5, "accepted", Some("athena"))]),
                lane("ares", "tester", &[card(6, "done", Some("ares"))]),
                lane("eos", "designer", &[]),
                lane("apollo", "worker", &[card(7, "queued", Some("apollo"))]),
                lane("hermes", "worker", &[card(8, "paused", Some("hermes"))]),
                lane("demeter", "worker", &[card(11, "waiting", Some("demeter"))]),
                lane("hades", "advisor", &[card(12, "open", Some("hades"))]),
                lane("hebe", "worker", &[card(13, "failed", Some("hebe"))]),
            ],
        })
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

    /// `cf task list` of `a_board_with_idle_sessions`, every lane with a task
    /// as the board always listed them.
    const WHOLE_LIST: &str = "Waiting for a member
T-10 [open] for a standard worker ← @chief: Task 10
@chief (chief)
T-1 [accepted] @chief ← @chief: Task 1
@zeus (worker)
T-2 [accepted] @zeus ← @chief: Task 2
T-9 [working] @zeus ← @chief: Task 9
@hera (reviewer)
T-3 [accepted] @hera ← @chief: Task 3
@athena (worker)
T-4 [cancelled] @athena ← @chief: Task 4
T-5 [accepted] @athena ← @chief: Task 5
@ares (tester)
T-6 [done] @ares ← @chief: Task 6
@apollo (worker)
T-7 [queued] @apollo ← @chief: Task 7
@hermes (worker)
T-8 [paused] @hermes ← @chief: Task 8
@demeter (worker)
T-11 [waiting] @demeter ← @chief: Task 11
@hades (advisor)
T-12 [open] @hades ← @chief: Task 12
@hebe (worker)
T-13 [failed] @hebe ← @chief: Task 13";

    /// The same without Hera and Athena, whose tasks are all over, and with their line.
    const WITHOUT_THE_IDLE: &str = "Waiting for a member
T-10 [open] for a standard worker ← @chief: Task 10
@chief (chief)
T-1 [accepted] @chief ← @chief: Task 1
@zeus (worker)
T-2 [accepted] @zeus ← @chief: Task 2
T-9 [working] @zeus ← @chief: Task 9
@ares (tester)
T-6 [done] @ares ← @chief: Task 6
@apollo (worker)
T-7 [queued] @apollo ← @chief: Task 7
@hermes (worker)
T-8 [paused] @hermes ← @chief: Task 8
@demeter (worker)
T-11 [waiting] @demeter ← @chief: Task 11
@hades (advisor)
T-12 [open] @hades ← @chief: Task 12
@hebe (worker)
T-13 [failed] @hebe ← @chief: Task 13
and 2 idle sessions (`cf task list --all`)";

    #[test]
    fn the_sessions_with_nothing_to_look_at_are_one_line_and_the_members_with_work_stay_listed() {
        let board = a_board_with_idle_sessions();
        assert_eq!(said(&[], &board).text, WITHOUT_THE_IDLE);
        // The chief's lane stays whatever its tasks say, and a session with
        // nothing on it was never listed: neither is one of the idle.
        let text = said(&[], &board).text;
        assert!(text.contains("@chief (chief)\nT-1 [accepted]"), "{text}");
        assert!(!text.contains("@eos"), "{text}");
    }

    #[test]
    fn a_session_with_one_task_left_to_look_at_stays_listed_whatever_the_rest_say() {
        for (state, shown) in [
            ("open", true),
            ("queued", true),
            ("working", true),
            ("waiting", true),
            ("paused", true),
            ("done", true),
            ("accepted", false),
            ("cancelled", false),
            ("failed", true),
        ] {
            let lanes = json!([lane(
                "zeus",
                "worker",
                &[
                    card(1, "accepted", Some("zeus")),
                    card(2, state, Some("zeus"))
                ]
            )]);
            let text = listed(&[], &lanes);
            assert_eq!(text.contains("@zeus (worker)"), shown, "{state}: {text}");
            assert_eq!(text.contains("idle session"), !shown, "{state}: {text}");
        }
    }

    #[test]
    fn all_lists_every_session_with_a_task_as_the_board_always_did() {
        let board = a_board_with_idle_sessions();
        let all = said(&["--all"], &board);
        assert_eq!(all.text, WHOLE_LIST);
        // And the JSON is the board's answer, as it was.
        assert_eq!(all.data, board);
    }

    #[test]
    fn the_json_holds_the_idle_sessions_under_a_key_of_their_own() {
        let mut board = a_board_with_idle_sessions();
        board["counts"] = json!({ "tasks": 12 });
        let data = said(&[], &board).data;
        let handles = |key: &str| -> Vec<String> {
            data[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|lane| lane["handle"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(
            handles("lanes"),
            ["chief", "zeus", "ares", "eos", "apollo", "hermes", "demeter", "hades", "hebe"]
        );
        // Whole lanes, as the board gave them.
        assert_eq!(handles("idle"), ["hera", "athena"]);
        assert_eq!(data["idle"][0], board["lanes"][2]);
        assert_eq!(data["idle"][1]["tasks"].as_array().unwrap().len(), 2);
        assert_eq!(
            data.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["open", "lanes", "idle", "counts"]
        );
        // A board with none says so in the same key.
        let none = said(&[], &json!({ "open": [], "lanes": [] })).data;
        assert_eq!(none, json!({ "open": [], "lanes": [], "idle": [] }));
    }

    #[test]
    fn one_idle_session_is_said_in_the_singular_and_a_board_of_nothing_else_says_only_that() {
        let lanes = json!([lane(
            "hera",
            "reviewer",
            &[card(3, "accepted", Some("hera"))]
        )]);
        assert_eq!(
            listed(&[], &lanes),
            "and 1 idle session (`cf task list --all`)"
        );
        let lanes = json!([
            lane("hera", "reviewer", &[card(3, "accepted", Some("hera"))]),
            lane("athena", "worker", &[card(4, "cancelled", Some("athena"))]),
        ]);
        assert_eq!(
            listed(&[], &lanes),
            "and 2 idle sessions (`cf task list --all`)"
        );
        assert_eq!(listed(&[], &json!([])), "No tasks yet.");
    }

    /// A board for the window whose `cf` is at `/x/bin/cf`, answering with `replies`.
    fn board_of_the_window(replies: Vec<Reply>) -> (Board, ScriptedApi) {
        let api = scripted(replies);
        let board = Board::new(Some(&api.url), "tok")
            .with_cf(cf_proto::cf_word::CfWord::of_word("/x/bin/cf").unwrap());
        (board, api)
    }

    /// The commands `cf` names in what it prints start with the `cf` of the window (T-1049): a
    /// shell can find another `cf` first, and a chief that reads a hint runs it.
    #[test]
    fn the_hints_cf_prints_start_their_commands_with_the_cf_of_the_window() {
        // The list's hint of the sessions it left out.
        let (board, _api) = board_of_the_window(vec![reply(
            200,
            json!({ "open": [], "lanes": [lane("hera", "reviewer", &[card(3, "accepted", Some("hera"))])] }),
        )]);
        assert_eq!(
            board_list(&[], &board).unwrap().text,
            "and 1 idle session (`/x/bin/cf task list --all`)"
        );
        // The line that says how to finish a step of one's own.
        let (board, _api) = board_of_the_window(vec![reply(
            201,
            json!({
                "task": { "number": 4, "state": "open", "assignee": null, "requester": "chief",
                    "title": "Later", "blockedBy": [], "pool": "chief", "tier": null },
                "message": null, "gated": false,
            }),
        )]);
        let words: Vec<String> = ["--self", "--needs", "T-3", "Later"]
            .map(str::to_owned)
            .to_vec();
        let given: Vec<OsString> = words.iter().map(OsString::from).collect();
        let said = add(&words, &given, &board, &mut std::io::empty()).unwrap();
        assert!(
            said.text
                .contains("finish it with: /x/bin/cf task done T-4 \"what you did\""),
            "{}",
            said.text
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
            "T-3 is on the board for a standard tester; it starts in a new window of one, and its result arrives in your inbox.\nSent: 7 characters, ending \"Run T-2\"."
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

    /// A chief that read of a free member after every task it added took a member
    /// for a person who can be busy, and chained its work through one window (T-1129, T-1134):
    /// whatever the task waits for, the receipt says a member of its role and tier takes it in a
    /// new window, and says nothing of a member free or busy.
    #[test]
    fn a_task_added_is_never_said_to_wait_for_a_free_member_and_goes_to_a_new_window() {
        let task = |pool: &str, tier: Value, blocked: Value| {
            json!({ "task": { "number": 3, "state": "open", "assignee": null, "requester": "chief",
                "title": "T", "blockedBy": blocked, "pool": pool, "tier": tier },
                "message": null, "gated": false })
        };
        let mut gated = task("worker", json!("standard"), json!([]));
        gated["gated"] = json!(true);
        let mut nearest = task("worker", json!("complex"), json!([]));
        nearest["asked"] = json!("critical");
        let cases = [
            (
                vec!["--tier", "standard", "A"],
                task("worker", json!("standard"), json!([])),
            ),
            (
                vec!["--advice", "--tier", "complex", "A"],
                task("advisor", json!("complex"), json!([])),
            ),
            (
                vec!["--review", "--tier", "light", "A"],
                task("reviewer", json!("light"), json!([])),
            ),
            (
                vec!["--test", "--tier", "standard", "A"],
                task("tester", json!("standard"), json!([])),
            ),
            (
                vec!["--design", "A"],
                task("designer", Value::Null, json!([])),
            ),
            (vec!["--tier", "critical", "A"], nearest),
            (
                vec!["--tier", "standard", "--needs", "T-2", "A"],
                task("worker", json!("standard"), json!([2])),
            ),
            (
                vec!["--tier", "standard", "--before", "T-9,T-10", "A"],
                task("worker", json!("standard"), json!([])),
            ),
            (vec!["--tier", "standard", "A"], gated),
        ];
        for (words, answer) in cases {
            let (_, said) = added(&words, &answer);
            let line = said.lines().next().unwrap_or_default();
            for word in line.split(|c: char| !c.is_alphabetic()) {
                let word = word.to_lowercase();
                assert!(
                    word != "free" && word != "busy",
                    "`cf task add {words:?}` says a member is {word}: {line}"
                );
            }
            assert!(
                line.contains(
                    "; it starts in a new window of one, and its result arrives in your inbox."
                ),
                "`cf task add {words:?}` does not say the task gets a new window: {line}"
            );
        }
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
            matches!(&said, Err(Failure::Usage(usage)) if usage == "cf task edit T-2 \"the new brief\", or --tier <tier>, --needs T-4 or --before T-9"),
            "a usage line was expected"
        );
        assert!(api.received().is_empty());
    }

    #[test]
    fn a_tier_alone_is_posted_with_no_brief() {
        let answer = json!({ "task": card(2, "open", None) });
        let api = scripted(vec![reply(200, answer)]);
        let words: Vec<String> = ["edit", "T-2", "--tier", "light"]
            .iter()
            .map(|word| (*word).to_owned())
            .collect();
        let given: Vec<OsString> = words.iter().map(OsString::from).collect();
        let said = command(
            &words,
            &given,
            &Board::new(Some(&api.url), "tok"),
            &mut std::io::empty(),
        )
        .unwrap();
        let posted: Vec<_> = api
            .received()
            .into_iter()
            .map(|got| (got.path.clone(), got.json()))
            .collect();
        assert_eq!(
            posted,
            [(
                "/api/tasks/2/edit".to_owned(),
                Some(json!({ "tier": "light" }))
            )]
        );
        assert!(
            said.text
                .starts_with("T-2 is for a standard worker; no window had it. T-2 [open]"),
            "{}",
            said.text
        );
    }

    #[test]
    fn a_tier_goes_with_a_brief_from_the_words_standard_input_or_a_file_in_one_command() {
        let answer = json!({ "task": card(2, "open", None) });
        let (posted, said) = edited(
            &["T-2", "--tier", "light", "Parse the new way"],
            "",
            &answer,
        );
        assert_eq!(
            posted,
            [(
                "/api/tasks/2/edit".to_owned(),
                Some(json!({ "body": "Parse the new way", "tier": "light" }))
            )]
        );
        assert!(
            said.starts_with(
                "T-2 has your new brief and is for a standard worker; no window had the old one"
            ),
            "{said}"
        );
        // The flag may stand after the words, and the words be standard input.
        let (posted, _) = edited(
            &["T-2", "-", "--tier", "light"],
            "Parse\nthe new way\n",
            &answer,
        );
        assert_eq!(
            posted[0].1,
            Some(json!({ "body": "Parse\nthe new way", "tier": "light" }))
        );
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("brief.md");
        std::fs::write(&file, "From a file\n").unwrap();
        let (posted, _) = edited(
            &["T-2", "--file", file.to_str().unwrap(), "--tier", "complex"],
            "",
            &answer,
        );
        assert_eq!(
            posted[0].1,
            Some(json!({ "body": "From a file", "tier": "complex" }))
        );
    }

    #[test]
    fn a_tier_the_staff_does_not_hold_is_said_with_the_members_of_the_tier_it_went_to() {
        let answer = json!({
            "task": card(2, "open", None),
            "asked": "complex",
            "heldBy": ["zeus", "diana"],
        });
        let (_, said) = edited(&["T-2", "--tier", "complex"], "", &answer);
        assert!(
            said.starts_with(
                "T-2 is for a standard worker (no complex worker is on the staff, so the nearest tier: @zeus, @diana); no window had it."
            ),
            "{said}"
        );
    }

    #[test]
    fn a_task_added_for_a_tier_nobody_holds_says_whose_tier_it_went_to() {
        let answer = json!({
            "task": { "number": 5, "state": "open", "assignee": null, "requester": "chief",
                "title": "Run it", "blockedBy": [], "pool": "tester", "tier": "standard" },
            "message": null,
            "asked": "complex",
            "heldBy": ["anubis"],
            "gated": false,
        });
        let (_, said) = added(&["--test", "--tier", "complex", "Run it"], &answer);
        assert!(
            said.contains("(no complex tester is on the staff, so the nearest tier: @anubis)"),
            "{said}"
        );
    }

    #[test]
    fn a_list_of_needs_replaces_and_a_signed_one_adds_and_removes() {
        let answer = json!({ "task": card(2, "open", None), "waiting": [9, 10] });
        for (words, sent) in [
            (["--needs", "T-3,T-4"], json!([3, 4])),
            (["--needs", "T-3"], json!([3])),
            (["--needs", "+T-5"], json!({ "add": [5], "remove": [] })),
            (["--needs", "-T-5"], json!({ "add": [], "remove": [5] })),
            (
                ["--needs", "+T-5,-T-6"],
                json!({ "add": [5], "remove": [6] }),
            ),
        ] {
            let (posted, said) = edited(&["T-2", words[0], words[1]], "", &answer);
            assert_eq!(
                posted,
                [(
                    "/api/tasks/2/edit".to_owned(),
                    Some(json!({ "needs": sent }))
                )],
                "{words:?}"
            );
            assert!(said.starts_with("T-2 needs nothing. T-2 [open]"), "{said}");
        }
        let (posted, said) = edited(&["T-2", "--before", "-T-9"], "", &answer);
        assert_eq!(
            posted[0].1,
            Some(json!({ "before": { "add": [], "remove": [9] } }))
        );
        assert!(
            said.starts_with("T-2 is waited for by T-9, T-10. T-2 ["),
            "{said}"
        );
    }

    #[test]
    fn the_flags_and_a_brief_go_in_one_command_and_a_list_that_mixes_is_a_usage_failure() {
        let answer = json!({ "task": card(2, "open", None), "waiting": [9] });
        let (posted, said) = edited(
            &[
                "T-2", "--tier", "light", "--needs", "+T-3", "--before", "T-9", "-",
            ],
            "A new brief\n",
            &answer,
        );
        assert_eq!(
            posted[0].1,
            Some(json!({
                "body": "A new brief",
                "tier": "light",
                "needs": { "add": [3], "remove": [] },
                "before": [9],
            }))
        );
        assert!(
            said.starts_with("T-2 has your new brief, is for a standard worker, needs nothing and is waited for by T-9;"),
            "{said}"
        );
        let api = scripted(vec![]);
        for list in ["T-3,+T-4", "T-3,-T-4", "T-x", "+T-"] {
            let words: Vec<String> = ["edit", "T-2", "--needs", list]
                .iter()
                .map(|word| (*word).to_owned())
                .collect();
            let given: Vec<OsString> = words.iter().map(OsString::from).collect();
            let said = command(
                &words,
                &given,
                &Board::new(Some(&api.url), "tok"),
                &mut std::io::empty(),
            );
            assert!(matches!(said, Err(Failure::Usage(_))), "{list}");
        }
        assert!(api.received().is_empty(), "the board is asked nothing");
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

    /// What `cf task finish T-7` says, the board answering with `answer`, and what it asked.
    fn finished(answer: Value) -> (Said, Vec<cf_board::scripted::Received>) {
        let api = scripted(vec![reply(200, answer)]);
        let words = ["finish".to_owned(), "T-7".to_owned()];
        let given: Vec<OsString> = words.iter().map(OsString::from).collect();
        let said = command(
            &words,
            &given,
            &Board::new(Some(&api.url), "tok"),
            &mut std::io::empty(),
        )
        .unwrap();
        (said, api.received())
    }

    #[test]
    fn finish_asks_the_board_to_finish_the_task_with_no_words_and_says_what_follows() {
        let (said, asked) = finished(json!({ "task": card(7, "done", Some("zeus-amber-pine")) }));
        assert_eq!(asked.len(), 1);
        assert_eq!(
            (asked[0].method.as_str(), asked[0].path.as_str()),
            ("POST", "/api/tasks/7/finish")
        );
        assert_eq!(asked[0].json(), Some(json!({})));
        assert_eq!(
            said.text,
            "T-7 is finished: what @zeus-amber-pine said last is its result, which reaches your inbox. Finishing ends nothing its turn left running."
        );
    }

    #[test]
    fn finish_promises_no_ending_and_says_the_jobs_and_agents_its_harness_counted_may_run_on() {
        for (jobs, agents, words) in [
            (1, 0, "(1 background job): it may run on"),
            (2, 0, "(2 background jobs): they may run on"),
            (0, 1, "(1 background agent or workflow): it may run on"),
            (
                1,
                2,
                "(1 background job and 2 background agents or workflows): they may run on",
            ),
        ] {
            let (said, _) = finished(json!({
                "task": card(7, "done", Some("zeus-amber-pine")),
                "jobs": jobs,
                "agents": agents,
            }));
            assert_eq!(
                said.text,
                format!("T-7 is finished: what @zeus-amber-pine said last is its result, which reaches your inbox. Finishing does not end what its turn left running {words} until its window closes, or past it where the host cannot tell what is the window's."),
                "{jobs} {agents}"
            );
        }
    }

    /// What `cf task <words…>` posted (its path and its body) and printed, the board
    /// answering with `answer`; or why `cf` refused it before it asked the board.
    fn moved_by(
        words: &[&str],
        answer: &Value,
    ) -> Result<(Option<(String, Value)>, String), Failure> {
        let api = scripted(vec![reply(200, answer.clone())]);
        let given: Vec<OsString> = words.iter().map(OsString::from).collect();
        let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
        let said = command(
            &words,
            &given,
            &Board::new(Some(&api.url), "tok"),
            &mut std::io::empty(),
        )?;
        let posted = api
            .received()
            .into_iter()
            .next()
            .and_then(|got| got.json().map(|json| (got.path, json)));
        Ok((posted, said.text))
    }

    /// What a command written wrong was told.
    fn wrongly(failure: Failure) -> String {
        match failure {
            Failure::Usage(said) => said,
            other => panic!("not a command written wrong: {other:?}"),
        }
    }

    fn retitled(state: &str, title: &str) -> Value {
        json!({ "task": {
            "number": 3, "state": state, "assignee": "zeus-pale", "requester": "chief",
            "title": title, "blockedBy": [], "pool": null, "tier": null,
        } })
    }

    #[test]
    fn edit_takes_a_title_alone_for_a_task_in_any_state_and_says_the_brief_is_as_it_was() {
        let (posted, said) = moved_by(
            &["edit", "T-3", "--title", "Parser and lexer"],
            &retitled("working", "Parser and lexer"),
        )
        .unwrap();
        assert_eq!(
            posted,
            Some((
                "/api/tasks/3/edit".to_owned(),
                json!({ "title": "Parser and lexer" })
            ))
        );
        assert_eq!(
            said,
            "T-3 has the title \"Parser and lexer\"; its brief is as it was, and the ledger keeps the old title. T-3 [working] @zeus-pale ← @chief: Parser and lexer"
        );
    }

    #[test]
    fn edit_reopen_and_resume_send_a_title_with_their_words_wherever_it_stands() {
        for (verb, state) in [
            ("edit", "open"),
            ("reopen", "working"),
            ("resume", "working"),
        ] {
            for words in [
                [verb, "T-3", "More words", "--title", "New scope"],
                [verb, "T-3", "--title", "New scope", "More words"],
            ] {
                let (posted, said) = moved_by(&words, &retitled(state, "New scope"))
                    .unwrap_or_else(|refused| panic!("{words:?}: {refused:?}"));
                assert_eq!(
                    posted,
                    Some((
                        format!("/api/tasks/3/{verb}"),
                        json!({ "body": "More words", "title": "New scope" })
                    )),
                    "{words:?}"
                );
                assert!(said.contains("New scope"), "{said}");
            }
        }
    }

    #[test]
    fn a_title_that_says_nothing_is_a_command_written_wrong_and_the_board_is_not_asked() {
        for words in [
            vec!["edit", "T-3", "--title"],
            vec!["edit", "T-3", "--title", ""],
            vec!["edit", "T-3", "--title", "  "],
            vec!["reopen", "T-3", "More", "--title", " "],
            vec!["resume", "T-3", "More", "--title"],
        ] {
            let api = scripted(vec![reply(200, retitled("working", "x"))]);
            let given: Vec<OsString> = words.iter().map(OsString::from).collect();
            let owned: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
            let refused = command(
                &owned,
                &given,
                &Board::new(Some(&api.url), "tok"),
                &mut std::io::empty(),
            )
            .err()
            .expect("a refusal");
            assert_eq!(
                wrongly(refused),
                "--title takes the new title: --title \"…\"",
                "{words:?}"
            );
            assert!(api.received().is_empty(), "{words:?}");
        }
    }

    #[test]
    fn a_reopen_and_a_resume_still_ask_for_their_words_when_they_are_given_a_title() {
        for (verb, asked) in [("reopen", "the follow-up"), ("resume", "what to do now")] {
            let refused = moved_by(
                &[verb, "T-3", "--title", "New scope"],
                &retitled("working", "New scope"),
            )
            .unwrap_err();
            assert_eq!(
                wrongly(refused),
                format!("cf task {verb} T-3 \"{asked}\""),
                "{verb}"
            );
        }
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
