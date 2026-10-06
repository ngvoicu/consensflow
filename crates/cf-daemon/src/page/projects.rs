//! The projects: listed, opened, resumed, closed, deleted and gated.

use cf_base::js;
use cf_ledger::model::parse_gate;
use cf_ledger::{NewChief, NewMember, NewProject};
use serde_json::{json, Map, Value};

use super::agents::{chief_on, last_staff_now, membership, saved};
use super::body::{one, Body, Fields, Said};
use super::Page;

/// `projects.list`.
pub(super) async fn list(page: &Page) -> Result<Fields, Said> {
    let projects = page.ledger.borrow().projects()?;
    one("projects", projects)
}

/// `project.open`: the chief on the saved agent given; the staff given, as the
/// roster has those agents now, else the last staff.
pub(super) async fn open(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let directory = body.get("directory");
    let name = match body.get("name") {
        None | Some(Value::Null) => json!(basename(directory)?),
        Some(name) => name.clone(),
    };
    let agents = saved(&page.env)?;
    let roster = agents.roster();
    let chief = chief_on(&roster, body.get("agent"))?;
    let staff = match body.get("staff") {
        None => last_staff_now(&page.ledger.borrow(), &roster)?,
        Some(Value::Array(members)) => members
            .iter()
            .map(|member| {
                let given = Body::new(member);
                Ok(membership(&roster, given.get("agent"))?.with_roles(given.get("roles")))
            })
            .collect::<Result<Vec<_>, Said>>()?,
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
        json!({ "harness": chief.harness, "agent": chief.agent }),
    );
    if let Some(gate) = body.get("gate") {
        request.insert("gate".to_owned(), gate.clone());
    }
    request.insert("staff".to_owned(), Value::Array(staff));
    let project = page.engine.open_project(new_project(request)?).await?;
    one("project", project)
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

/// The request as the dispatcher takes it. Where the page sent what the ledger
/// takes (text for the folder and the name, a flag or none for the gate, roles
/// that are a list of words), it is typed as sent and left to the dispatcher
/// and then the ledger to refuse in the order Node's did: a harness no adapter
/// opens is refused before a role that does not fit. Where it did not, the
/// ledger's own words for it, which are the same words.
fn new_project(request: Map<String, Value>) -> Result<NewProject, Said> {
    if let Some(typed) = typed(&request) {
        return Ok(typed);
    }
    Ok(NewProject::from_json(&Value::Object(request))?)
}

/// The request typed as sent, or none where something is not of the type the
/// ledger takes.
fn typed(request: &Map<String, Value>) -> Option<NewProject> {
    let chief = request.get("chief")?;
    let staff = request.get("staff")?.as_array()?;
    Some(NewProject {
        directory: request.get("directory")?.as_str()?.to_owned(),
        name: request.get("name")?.as_str()?.to_owned(),
        chief: NewChief {
            harness: chief.get("harness")?.as_str()?.to_owned(),
            agent: Some(chief.get("agent")?.as_str()?.to_owned()),
        },
        staff: staff.iter().map(member).collect::<Option<Vec<_>>>()?,
        gate: match request.get("gate") {
            None => false,
            Some(Value::Bool(gate)) => *gate,
            Some(_) => return None,
        },
    })
}

/// A member as the page-made value says it, none when its roles are not a
/// list of words.
fn member(member: &Value) -> Option<NewMember> {
    let roles = member
        .get("roles")?
        .as_array()?
        .iter()
        .map(|role| role.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    Some(NewMember {
        agent: member.get("agent")?.as_str()?.to_owned(),
        harness: member.get("harness")?.as_str()?.to_owned(),
        designer: member.get("designer")?.as_bool()?,
        roles,
        tier: member.get("tier")?.as_str()?.to_owned(),
    })
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

/// The last name of a path, as `basename` of Node's `path.js` writes it for
/// `win32` or for `posix`: its trailing separators and a Windows drive's `C:`
/// not counted.
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
