//! The projects: listed, opened, resumed, closed, deleted, gated, renamed and
//! described, given the rules for their staff, and told the size their members'
//! windows compact at.

use cf_base::js;
use cf_ledger::model::{parse_gate, parse_meeting};
use cf_ledger::{
    parse_advisor_tool, parse_compaction, parse_description, parse_name, parse_prompt_suggestion,
    parse_staff_rules, NewMember, NewProject,
};
use serde_json::{json, Map, Value};

use super::body::{one, Body, Fields, Said};
use super::chief::chief_asked;
use super::staff::picked_member;
use super::Page;

/// `projects.list`.
pub(super) async fn list(page: &Page) -> Result<Fields, Said> {
    let projects = page.ledger.borrow().projects()?;
    one("projects", projects)
}

/// `project.open`: the chief on the harness, model and effort given (`chief_asked`); the
/// staff given, each member a harness, a model its harness lists, an effort and a tier, as
/// Add member takes one, else the last staff, but its members on a harness not installed
/// here. The gate, the wake-up calls
/// (`wakeUp`, as `wakeup.set` takes them), the specs (`specs`, as
/// `project.specs` takes them), the rules for the staff (`rules`, as `project.rules`
/// takes them) and the compaction (`compaction`, as `project.compaction` takes it) are
/// the human's word for the project, kept in the write that makes it, so the chief's
/// first launch has them; one the ledger refuses refuses the whole start, in the words
/// the Project settings dialog shows for it, and no project is made. A `compaction` that
/// is left out starts the project on the ledger's default size, and `null` on each
/// harness's own point: what is passed to the ledger is what the page sent, `null` too.
pub(super) async fn open(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let directory = body.get("directory");
    let name = match body.get("name") {
        None | Some(Value::Null) => json!(basename(directory)?),
        Some(name) => name.clone(),
    };
    let chief = chief_asked(page, body, false).await?;
    let staff = match body.get("staff") {
        None => last_staff(page)?,
        Some(Value::Array(members)) => {
            let mut staff = Vec::new();
            for member in members {
                let mut choice = member.clone();
                if member.get("backupOf").is_some_and(|value| !value.is_null()) {
                    let fields = choice
                        .as_object_mut()
                        .ok_or_else(|| Said::from("a staff row is an object"))?;
                    fields.insert("roles".to_owned(), json!(["worker"]));
                    fields.insert("tier".to_owned(), json!("light"));
                }
                let picked = picked_member(page, Body::new(&choice), false).await?;
                let mut row = member_json(&picked);
                for key in ["id", "backupOf", "vacationSince"] {
                    if let Some(value) = member.get(key) {
                        row[key] = value.clone();
                    }
                }
                staff.push(row);
            }
            staff
        }
        // `staff.map` of what is no list.
        Some(other) => {
            return Err(Said(format!(
                "staff is a list of members, not {}",
                js::text(Some(other))
            )))
        }
    };
    let mut request = Map::new();
    if let Some(directory) = directory {
        request.insert("directory".to_owned(), directory.clone());
    }
    request.insert("name".to_owned(), name);
    request.insert(
        "chief".to_owned(),
        json!({
            "harness": chief.harness,
            "model": chief.model,
            "effort": chief.effort,
            "modelName": chief.model_name,
        }),
    );
    for key in [
        "gate",
        "wakeUp",
        "specs",
        "rules",
        "compaction",
        "advisorTool",
        "promptSuggestion",
    ] {
        if let Some(said) = body.get(key) {
            request.insert(key.to_owned(), said.clone());
        }
    }
    request.insert("staff".to_owned(), Value::Array(staff));
    let project = page
        .engine
        .open_project_with_staff(new_project(request.clone())?, Value::Object(request))
        .await?;
    one("project", project)
}

/// A member as the ledger reads one from JSON (`NewMember::from_json`).
fn member_json(member: &NewMember) -> Value {
    json!({
        "harness": member.harness,
        "model": member.model,
        "effort": member.effort,
        "modelName": member.model_name,
        "designer": member.designer,
        "roles": member.roles,
        "tier": member.tier,
    })
}

/// The last project's staff for a new one, each member on a harness installed here, as the
/// ledger reads it from JSON: what New project starts with where the page names no staff.
fn last_staff(page: &Page) -> Result<Vec<Value>, Said> {
    let missing = cf_harness::detect::missing_harnesses(&page.env);
    let staff: Vec<_> = page
        .ledger
        .borrow()
        .last_staff()?
        .into_iter()
        .filter(|member| {
            member
                .harness
                .as_deref()
                .and_then(cf_proto::agents::Harness::from_kind)
                .is_some_and(|harness| !missing.contains(&harness))
        })
        .collect();
    let owners: std::collections::HashSet<i64> = staff
        .iter()
        .filter(|member| member.backup_of.is_none())
        .filter_map(|member| member.id)
        .collect();
    staff
        .into_iter()
        .filter(|member| member.backup_of.is_none_or(|id| owners.contains(&id)))
        .map(|member| serde_json::to_value(member).map_err(Said::from))
        .collect()
}

/// `project.resume`.
pub(super) async fn resume(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let project = page.engine.resume_project(body.whole("project")?).await?;
    one("project", project)
}

/// `project.close`.
pub(super) async fn close(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let project = page.engine.close_project(body.whole("project")?).await?;
    one("project", project)
}

/// `project.delete`.
pub(super) async fn delete(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let project = page.engine.delete_project(body.whole("project")?).await?;
    one("project", project)
}

/// `project.gate`: whether the human approves each message, checked before the
/// project is looked for.
pub(super) async fn gate(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let gate = parse_gate(body.get("gate"))?;
    let project = page
        .ledger
        .borrow_mut()
        .set_gate(body.whole("project")?, gate)?;
    one("project", project)
}

/// `project.meeting`: the human starts (`on`) or stops a meeting with the project's
/// chief, the flag checked before the project is looked for. While one is on, what
/// is queued for the chief waits; a closed project's is refused. The dispatcher is
/// woken, so what a Stop lets go goes at once, paced.
pub(super) async fn meeting(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let on = parse_meeting(body.get("on"))?;
    let project = page
        .ledger
        .borrow_mut()
        .set_meeting(body.whole("project")?, on)?;
    one("project", project)
}

/// `project.rename`: the name the human gives the project, which has the rules a
/// name has when a project is opened and is checked before the project is looked
/// for. The folder the project is in stays as it was.
pub(super) async fn rename(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let name = parse_name(body.get("name"))?;
    let project = page
        .ledger
        .borrow_mut()
        .rename_project(body.whole("project")?, &name)?;
    one("project", project)
}

/// `project.describe`: the line the human writes under the project's name, one
/// line of plain text of at most 200 characters, checked before the project is
/// looked for. A `description` that is left out, `null` or blank takes the line
/// away. The line is the human's alone: no agent is told it.
pub(super) async fn describe(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let description = parse_description(body.get("description"))?;
    let project = page
        .ledger
        .borrow_mut()
        .describe_project(body.whole("project")?, description.as_deref())?;
    one("project", project)
}

/// `project.rules`: the rules the human writes for the project's staff, plain
/// text of at most 8,000 bytes and 150 lines, checked before the project is
/// looked for. `rules` that are left out, `null` or blank take them away. Only
/// the page writes them: no route of the agents' API does, so no `cf` command,
/// the chief's or a member's, changes them. Every window of the project is given
/// them when it next starts (the engine's `roles::with_staff_rules`), and a chief whose
/// window is open is told at its next rest, in a note (the engine's `role_news`).
pub(super) async fn rules(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let rules = parse_staff_rules(body.get("rules"))?;
    let project = page
        .ledger
        .borrow_mut()
        .set_staff_rules(body.whole("project")?, rules.as_deref())?;
    one("project", project)
}

/// `project.compaction`: the size, in tokens, at which the human has the project's
/// member windows compact, whatever their harness, from 250,000 to 1,000,000, checked
/// before the project is looked for. `compaction` that is left out or `null` takes the
/// choice away: each harness's own point. Only the page chooses it: no route of the
/// agents' API does, so no `cf` command, the chief's or a member's, changes it.
/// Each member window of the project is given it when it next starts (the
/// harness's launch); the chief's never is.
pub(super) async fn compaction(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let size = parse_compaction(body.get("compaction"))?;
    let project = page
        .ledger
        .borrow_mut()
        .set_compaction(body.whole("project")?, size)?;
    one("project", project)
}

/// `project.advisor_tool`: whether the human has the project's members run with Claude
/// Code's own advisor tool: a flag, checked before the project is looked for. Off is every
/// project's until it is ticked. Only the page chooses it: no route of the agents' API does,
/// so no `cf` command, the chief's or a member's, changes it. Each member window of the
/// project is given the switch when it next starts (the Claude adapter's launch); an open
/// window keeps what it began with, and the chief's never is given it.
pub(super) async fn advisor_tool(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let on = parse_advisor_tool(body.get("advisorTool"))?;
    let project = page
        .ledger
        .borrow_mut()
        .set_advisor_tool(body.whole("project")?, on)?;
    one("project", project)
}

/// `project.prompt_suggestion`: whether the human leaves Claude Code's prompt suggestion on in
/// the project's windows: a flag, checked before the project is looked for. Off is every
/// project's until it is ticked. Only the page chooses it: no route of the agents' API does, so
/// no `cf` command, the chief's or a member's, changes it. Each Claude Code window of the
/// project, the chief's as each member's, is given the switch when it next starts (the Claude
/// adapter's launch, fresh or resumed); an open window keeps what it began with.
pub(super) async fn prompt_suggestion(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let on = parse_prompt_suggestion(body.get("promptSuggestion"))?;
    let project = page
        .ledger
        .borrow_mut()
        .set_prompt_suggestion(body.whole("project")?, on)?;
    one("project", project)
}

/// Only the person's page can switch how the chief works.
pub(super) async fn own_hands(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let on = cf_ledger::parse_own_hands(body.get("ownHands"))?;
    let project = page
        .ledger
        .borrow_mut()
        .set_own_hands(body.whole("project")?, on)?;
    one("project", project)
}

/// The request as the dispatcher takes it: read by the ledger (`NewProject::from_json`), whose
/// words refuse what it cannot read. The page has already picked the chief and every member
/// (`chief_asked`, `picked_member`), so the request holds what the ledger takes: a member with
/// its harness, model, effort, the model's name, its roles and its tier.
fn new_project(mut request: Map<String, Value>) -> Result<NewProject, Said> {
    // A copied backup holds its owner's roles, including an image designer's, but
    // its own model need not be an image agent. The engine needs its harness for
    // validation; the ledger checks and reconnects the original source rows.
    if let Some(Value::Array(staff)) = request.get_mut("staff") {
        for row in staff {
            if row.get("backupOf").is_some_and(|value| !value.is_null()) {
                if let Some(fields) = row.as_object_mut() {
                    let designer = fields.get("designer") == Some(&Value::Bool(true));
                    fields.insert(
                        "roles".to_owned(),
                        json!([if designer { "designer" } else { "worker" }]),
                    );
                    fields.insert("tier".to_owned(), json!("light"));
                }
            }
        }
    }
    Ok(NewProject::from_json(&Value::Object(request))?)
}

/// `path.basename(path)` of the system this is built for: the name Node gives a
/// project made of a folder.
fn basename(directory: Option<&Value>) -> Result<String, Said> {
    let Some(Value::String(path)) = directory else {
        return Err(Said(format!(
            "The \"path\" argument must be of type string. Received {}",
            received(directory)
        )));
    };
    Ok(last_name(path, cfg!(windows)))
}

/// The last name of a path, as Node's `path.basename` writes it for `win32` or
/// for `posix`: its trailing separators and a Windows drive's `C:` not counted.
fn last_name(path: &str, windows: bool) -> String {
    let bytes = path.as_bytes();
    let separator = |byte: u8| byte == b'/' || (windows && byte == b'\\');
    let mut start = 0;
    if windows && bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        start = 2;
    }
    let mut end = None;
    let mut matched_separator = true;
    for at in (start..bytes.len()).rev() {
        if separator(bytes[at]) {
            if !matched_separator {
                start = at + 1;
                break;
            }
        } else if end.is_none() {
            matched_separator = false;
            end = Some(at + 1);
        }
    }
    // Every cut is next to a separator or an end: between two characters.
    end.and_then(|end| path.get(start..end))
        .unwrap_or_default()
        .to_owned()
}

/// What Node's `ERR_INVALID_ARG_TYPE` says it was given.
fn received(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_owned(),
        Some(Value::Null) => "null".to_owned(),
        Some(Value::Bool(flag)) => format!("type boolean ({flag})"),
        Some(Value::Number(_)) => format!("type number ({})", js::text(value)),
        Some(Value::Array(_)) => "an instance of Array".to_owned(),
        Some(Value::Object(_)) => "an instance of Object".to_owned(),
        Some(Value::String(text)) => format!("type string ('{text}')"),
    }
}

#[cfg(test)]
mod tests {
    use cf_ledger::{NewChief, COMPACTION_DEFAULT};

    use super::*;

    /// What Node v26.8.1's `path.posix.basename` and `path.win32.basename` said
    /// of each path: the name a folder gives its project, on either system.
    const NODE_SAYS: [(&str, &str, &str); 26] = [
        ("/work/app", "app", "app"),
        ("/work/app/", "app", "app"),
        ("/work/app//", "app", "app"),
        ("app", "app", "app"),
        ("/", "", ""),
        ("", "", ""),
        (r"C:\work\app", r"C:\work\app", "app"),
        (r"C:\work\app\", r"C:\work\app\", "app"),
        ("C:/work/app", "app", "app"),
        ("C:", "C:", ""),
        ("C:app", "C:app", "app"),
        (r"C:\", r"C:\", ""),
        (r"\\server\share\dir", r"\\server\share\dir", "dir"),
        (r"a\b", r"a\b", "b"),
        (r"C:a\b", r"C:a\b", "b"),
        ("./x/", "x", "x"),
        ("..", "..", ".."),
        ("日本語", "日本語", "日本語"),
        ("/work/日本語/", "日本語", "日本語"),
        ("c:", "c:", ""),
        (r"c:\", r"c:\", ""),
        ("x:y:z", "x:y:z", "y:z"),
        ("//", "", ""),
        ("a//b//", "b", "b"),
        (r"\", r"\", ""),
        (r"D:\a b\c d", r"D:\a b\c d", "c d"),
    ];

    #[test]
    fn a_project_is_named_after_the_last_name_of_its_folder_as_node_names_it_on_either_system() {
        for (path, posix, win32) in NODE_SAYS {
            assert_eq!(last_name(path, false), posix, "{path:?} on posix");
            assert_eq!(last_name(path, true), win32, "{path:?} on win32");
            let here = if cfg!(windows) { win32 } else { posix };
            assert_eq!(basename(Some(&json!(path))).unwrap(), here, "{path:?}");
        }
    }

    /// The request `open` builds: the chief as `chief_asked` answers, the staff as `member_json`
    /// says each member (a member has no `agent`, only its harness, model, effort, the model's
    /// name, its roles and its tier).
    fn opened(chief_model: Value, staff: Vec<Value>) -> Map<String, Value> {
        let Value::Object(request) = json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "model": chief_model, "effort": "high" },
            "staff": staff,
        }) else {
            unreachable!("an object")
        };
        request
    }

    fn astra() -> Value {
        json!({
            "harness": "codex",
            "model": "gpt-6-astra",
            "effort": "high",
            "modelName": "GPT-6 Astra",
            "designer": false,
            "roles": ["worker"],
            "tier": "light",
        })
    }

    fn astra_member() -> NewMember {
        NewMember {
            harness: "codex".to_owned(),
            model: Some("gpt-6-astra".to_owned()),
            effort: Some("high".to_owned()),
            model_name: Some("GPT-6 Astra".to_owned()),
            designer: false,
            roles: vec!["worker".to_owned()],
            tier: "light".to_owned(),
        }
    }

    fn chief_on(model: Option<&str>) -> NewChief {
        NewChief {
            harness: "claude-code".to_owned(),
            model: model.map(str::to_owned),
            effort: Some("high".to_owned()),
            model_name: None,
        }
    }

    /// What the page sends is read as the ledger reads it, whatever the staff and whether or not the
    /// chief has a model: a member with its model and effort, no member, a chief with no model.
    #[test]
    fn a_project_to_open_is_read_as_the_ledger_reads_it() {
        for (what, request, chief, staff) in [
            (
                "a member",
                opened(json!("claude-opus-5-5"), vec![astra()]),
                chief_on(Some("claude-opus-5-5")),
                vec![astra_member()],
            ),
            (
                "no member, a chief with a model",
                opened(json!("claude-opus-5-5"), Vec::new()),
                chief_on(Some("claude-opus-5-5")),
                Vec::new(),
            ),
            (
                "a member, a chief with no model",
                opened(Value::Null, vec![astra()]),
                chief_on(None),
                vec![astra_member()],
            ),
            (
                "no member, a chief with no model",
                opened(Value::Null, Vec::new()),
                chief_on(None),
                Vec::new(),
            ),
        ] {
            let read =
                new_project(request.clone()).unwrap_or_else(|said| panic!("{what}: {said:?}"));
            assert_eq!(
                read,
                NewProject {
                    directory: "/work/app".to_owned(),
                    name: "app".to_owned(),
                    chief,
                    staff,
                    gate: false,
                    wake_up: None,
                    specs: None,
                    staff_rules: None,
                    // The request says nothing of it: the size a new project starts on (T-1400).
                    compaction: Some(COMPACTION_DEFAULT),
                    advisor_tool: false,
                    prompt_suggestion: false,
                },
                "{what}"
            );
            assert_eq!(
                read,
                NewProject::from_json(&Value::Object(request)).unwrap(),
                "{what}"
            );
        }
    }

    /// A member is read from the keys a member has now. One that also carries an `agent` (the
    /// name a member had before it was named by its model) is the same member: its model, its
    /// effort and the model's name are not dropped for the old name (T-1335, item 3).
    #[test]
    fn a_member_with_an_agent_key_keeps_its_model_and_effort() {
        let mut old = astra();
        old["agent"] = json!("old-name");
        let read = new_project(opened(json!("claude-opus-5-5"), vec![old])).unwrap();
        assert_eq!(read.staff, vec![astra_member()]);
    }

    #[test]
    fn a_folder_that_is_no_text_is_refused_in_nodes_words() {
        for (given, received) in [
            (None, "undefined"),
            (Some(json!(null)), "null"),
            (Some(json!(5)), "type number (5)"),
            (Some(json!(true)), "type boolean (true)"),
            (Some(json!([1])), "an instance of Array"),
            (Some(json!({})), "an instance of Object"),
        ] {
            assert_eq!(
                basename(given.as_ref()).unwrap_err().0,
                format!("The \"path\" argument must be of type string. Received {received}")
            );
        }
    }
}
