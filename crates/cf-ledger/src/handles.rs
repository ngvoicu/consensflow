//! A member's name and handle, made from the model it runs on (spec `harness-models`,
//! HMD-05 and HMD-12): never a nickname. Its name is its model's id as its harness lists it
//! (`claude-opus-5-5`, `gpt-6.1-sol`; Pi's without its provider), its handle that id and
//! the effort in lowercase words joined by `-` (`@claude-opus-5-5-high`). Where another
//! participant of the project has that handle, the harness goes in front
//! (`@devin-claude-opus-5-5-high`), a Pi model's provider before that
//! (`@anthropic-claude-opus-5-5-high`), and where one has that too, a number after (`-2`).
//! A member's windows are named after it, two words more ([`crate::names`]). Migration 0031
//! named the members of the ready-made agents by the name their harness's list gave the
//! model (`@opus-high`, `schema::after`); 0033 renames every member to its id's name
//! ([`name_members_by_id`]), and Edit member does the same for the member it changes
//! ([`rename_member`]). A name a participant leaves is kept (`former_handles`).

use rusqlite::{params, Connection, OptionalExtension};

use crate::model::LedgerError;

/// The most characters of a handle made here: a handle holds 64, and its windows add
/// two words and a number to it.
pub const MEMBER_HANDLE_MAX: usize = 40;

/// What a member's model is called: the model as its harness's list names it (`model_name`:
/// its id; Pi's `provider/id`; Devin's listed id, not the launch id its `model` holds), else
/// its id as the harness takes it, else (a member on its harness's default, as the image
/// designer from before the models were the harnesses' own) the harness's default.
pub fn model_shown(model_name: Option<&str>, model: Option<&str>, harness: Option<&str>) -> String {
    model_name
        .filter(|name| !name.trim().is_empty())
        .or(model)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{} default", harness_label(harness)))
}

/// A participant's model as the page, `cf` and the chief's text show it: its name and,
/// where it has one, its effort after a dot (`Opus · high`).
pub fn model_and_effort(
    model_name: Option<&str>,
    model: Option<&str>,
    harness: Option<&str>,
    effort: Option<&str>,
) -> String {
    let shown = model_shown(model_name, model, harness);
    match effort {
        Some(effort) if !effort.is_empty() => format!("{shown} · {effort}"),
        _ => shown,
    }
}

/// What a member that needs an edit (T-1202) is shown to run on: no model, which is so,
/// never its harness's default, which it does not run on.
pub const NEEDS_AN_EDIT: &str = "no model: needs an edit";

/// What `participant` is shown to run on, as the page, `cf` and the chief's text show it:
/// its model and effort ([`model_and_effort`]), or [`NEEDS_AN_EDIT`].
pub fn runs_on(participant: &crate::ParticipantView) -> String {
    if participant.needs_edit {
        return NEEDS_AN_EDIT.to_owned();
    }
    model_and_effort(
        participant.model_name.as_deref(),
        participant.model.as_deref(),
        participant.harness.as_deref(),
        participant.effort.as_deref(),
    )
}

/// How the page names a harness, by its kind.
pub fn harness_label(harness: Option<&str>) -> &str {
    match harness {
        Some("claude-code") => "Claude Code",
        Some("codex") => "Codex",
        Some("pi") => "Pi",
        Some("devin") => "Devin",
        Some("muse") => "Muse Code",
        Some(other) => other,
        None => "its harness's",
    }
}

/// `words` in lowercase ASCII letters and digits, each run of anything else one `-`,
/// none at either end, cut to [`MEMBER_HANDLE_MAX`] characters at a word's end where it
/// can be. `member` where nothing is left.
fn slug(words: &[&str]) -> String {
    let mut slug = String::new();
    for word in words {
        for part in word
            .to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|part| !part.is_empty())
        {
            if !slug.is_empty() {
                slug.push('-');
            }
            slug.push_str(part);
        }
    }
    if slug.len() > MEMBER_HANDLE_MAX {
        slug.truncate(MEMBER_HANDLE_MAX);
        slug = slug.trim_end_matches('-').to_owned();
    }
    if slug.is_empty() {
        "member".to_owned()
    } else {
        slug
    }
}

/// The handle of a member whose model is shown as `shown` (see [`model_shown`]; for Pi
/// without its `provider`), at `effort`, on `harness` (its kind), where `taken` says which
/// handles the project has: the name and the effort; with the provider in front where
/// that is taken (a Pi model has one); with the harness in front where that is taken; and
/// with a number after where that is too. No handle is longer than [`MEMBER_HANDLE_MAX`],
/// the number included.
pub fn member_handle(
    shown: &str,
    provider: Option<&str>,
    effort: Option<&str>,
    harness: &str,
    mut taken: impl FnMut(&str) -> bool,
) -> String {
    // A name that ends with its effort already (Devin's id for it, where its list was never
    // kept: `claude-opus-5-5-max`) does not say it twice.
    let effort = effort
        .filter(|effort| !slug(&[shown]).ends_with(&format!("-{}", slug(&[effort]))))
        .unwrap_or_default();
    let plain = slug(&[shown, effort]);
    if !taken(&plain) {
        return plain;
    }
    if let Some(provider) = provider {
        let with_provider = slug(&[provider, shown, effort]);
        if !taken(&with_provider) {
            return with_provider;
        }
    }
    let with_harness = slug(&[harness, provider.unwrap_or_default(), shown, effort]);
    if !taken(&with_harness) {
        return with_harness;
    }
    (2..)
        .map(|n| numbered(&with_harness, n))
        .find(|handle| !taken(handle))
        .unwrap_or(with_harness)
}

/// `name` and a number after it, within [`MEMBER_HANDLE_MAX`] characters as a whole: the name
/// is cut to leave room for `-n`, at a word's end where a `-` would be left before the number.
fn numbered(name: &str, n: usize) -> String {
    let number = format!("-{n}");
    let room = MEMBER_HANDLE_MAX.saturating_sub(number.len());
    // A name is ASCII (`slug`): a cut falls between characters.
    let cut = name
        .get(..room.min(name.len()))
        .unwrap_or(name)
        .trim_end_matches('-');
    format!("{cut}{number}")
}

/// A model as its harness takes it, split into the provider that a name leaves out and the id
/// it is made of: a model that holds a `/` is `provider/id` (Pi's; the id may hold a `/` of its
/// own, as an OpenRouter one does), any other is only an id. No other harness's id holds one.
pub fn split_provider(model: &str) -> (Option<&str>, &str) {
    match model.split_once('/') {
        Some((provider, id)) if !provider.is_empty() && !id.is_empty() => (Some(provider), id),
        _ => (None, model),
    }
}

/// The handle of a member on `model` (its id as its harness lists it: [`model_shown`]), at
/// `effort`, on `harness`: [`member_handle`] with a Pi model's provider left out of its name.
pub fn model_handle(
    model: &str,
    effort: Option<&str>,
    harness: &str,
    taken: impl FnMut(&str) -> bool,
) -> String {
    let (provider, id) = split_provider(model);
    member_handle(id, provider, effort, harness, taken)
}

/// Whether `handle`, a member's or one of its windows', holds the model and effort the
/// member runs on, as [`model_handle`] makes it: the name, whatever stands in front of it
/// (a harness, a provider) or after it (a number, a window's two words). A member on its
/// harness's default, or marked as needing an edit, has no model for a name to hold.
pub fn names_its_model(
    handle: &str,
    model_name: Option<&str>,
    model: Option<&str>,
    effort: Option<&str>,
) -> bool {
    let Some(shown) = model_name.filter(|name| !name.trim().is_empty()).or(model) else {
        return false;
    };
    let (_, id) = split_provider(shown);
    let effort = effort
        .filter(|effort| !slug(&[id]).ends_with(&format!("-{}", slug(&[effort]))))
        .unwrap_or_default();
    let name = slug(&[id, effort]);
    format!("-{handle}-").contains(&format!("-{name}-"))
}

/// Whether `project` has a participant, in the staff or gone from it, named `handle`.
pub(crate) fn handle_taken(db: &Connection, project: i64, handle: &str) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS (SELECT 1 FROM participant WHERE project_id = ? AND handle = ?)",
        params![project, handle],
        |row| row.get(0),
    )
}

/// Migration 0031's part SQL cannot do: every member a ready-made agent named (its row
/// keeps the agent's name in `agent`) takes a handle made from its model, in the order
/// the members joined, and each of its windows its member's new handle with the window's
/// own two words after. The chief and the human keep theirs, a member that names no agent
/// was named by its model when it joined, and one that needs an edit (an agent the migration
/// could not resolve) keeps the name it had, its windows theirs.
/// Tasks, messages and every other row point at participants by id: their authors stay.
pub(crate) fn name_members_by_model(db: &Connection) -> Result<(), LedgerError> {
    let members = db
        .prepare(
            "SELECT id, project_id, handle, harness, model, effort, model_name FROM participant
             WHERE member_id IS NULL AND role NOT IN ('human', 'chief') AND agent IS NOT NULL
               AND needs_edit = 0
             ORDER BY project_id, id",
        )?
        .query_map([], |row| {
            Ok(Member {
                id: row.get(0)?,
                project: row.get(1)?,
                handle: row.get(2)?,
                harness: row.get(3)?,
                model: row.get(4)?,
                effort: row.get(5)?,
                model_name: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // Every member's old handle goes first, so a new one may be one an old one was.
    for member in &members {
        db.execute(
            "UPDATE participant SET handle = ? WHERE id = ?",
            params![format!("\u{1}{}", member.id), member.id],
        )?;
    }
    for member in &members {
        let shown = model_shown(
            member.model_name.as_deref(),
            member.model.as_deref(),
            member.harness.as_deref(),
        );
        let handle = member_handle(
            &shown,
            None,
            member.effort.as_deref(),
            member.harness.as_deref().unwrap_or_default(),
            |handle| handle_taken(db, member.project, handle).unwrap_or(true),
        );
        db.execute(
            "UPDATE participant SET handle = ? WHERE id = ?",
            params![handle, member.id],
        )?;
        let windows = db
            .prepare("SELECT id, handle FROM participant WHERE member_id = ? ORDER BY id")?
            .query_map([member.id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, old) in windows {
            let own = old
                .strip_prefix(&member.handle)
                .and_then(|rest| rest.strip_prefix('-'))
                .unwrap_or(&old);
            let base = format!("{handle}-{own}");
            let mut name = base.clone();
            let mut n = 2;
            while handle_taken(db, member.project, &name)? {
                name = format!("{base}-{n}");
                n += 1;
            }
            db.execute(
                "UPDATE participant SET handle = ? WHERE id = ?",
                params![name, id],
            )?;
        }
    }
    Ok(())
}

/// Gives `member` (in `project`) the handle `new` in place of `old`, and each of its
/// windows its member's new handle with the window's own two words after it. What a
/// participant leaves is kept in its `former_handles`, oldest first. Every other row points
/// at a participant by id, so tasks, messages and history keep their authors; a window's
/// token names its participant by id too, so a window at work goes on as it was, under its
/// new name.
pub(crate) fn rename_member(
    db: &Connection,
    project: i64,
    member: i64,
    old: &str,
    new: &str,
) -> Result<(), LedgerError> {
    if old != new {
        keep_former(db, member, old)?;
    }
    db.execute(
        "UPDATE participant SET handle = ? WHERE id = ?",
        params![new, member],
    )?;
    if old == new {
        return Ok(());
    }
    let windows = db
        .prepare("SELECT id, handle FROM participant WHERE member_id = ? ORDER BY id")?
        .query_map([member], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, was) in windows {
        let own = was
            .strip_prefix(old)
            .and_then(|rest| rest.strip_prefix('-'))
            .unwrap_or(&was);
        let base = format!("{new}-{own}");
        let mut name = base.clone();
        let mut n = 2;
        while handle_taken(db, project, &name)? {
            name = format!("{base}-{n}");
            n += 1;
        }
        keep_former(db, id, &was)?;
        db.execute(
            "UPDATE participant SET handle = ? WHERE id = ?",
            params![name, id],
        )?;
    }
    Ok(())
}

/// `handle` joins the names `participant` has left, after the ones it left before.
fn keep_former(db: &Connection, participant: i64, handle: &str) -> Result<(), LedgerError> {
    let kept: String = db.query_row(
        "SELECT former_handles FROM participant WHERE id = ?",
        [participant],
        |row| row.get(0),
    )?;
    let mut kept: Vec<String> = serde_json::from_str(&kept)?;
    if kept.last().map(String::as_str) != Some(handle) {
        kept.push(handle.to_owned());
    }
    db.execute(
        "UPDATE participant SET former_handles = ? WHERE id = ?",
        params![serde_json::to_string(&kept)?, participant],
    )?;
    Ok(())
}

/// How a row's listed model was found ([`listed_model`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listed {
    /// The row holds the id its list names, or nothing says it holds another.
    AsTaken,
    /// The home's list holds the row's launch id (the id that has its effort in it) under
    /// one listed id: that is it.
    FromList,
    /// The list has no such row; the launch id has its effort's word among its words once, and
    /// what is left of it is the model's name its row keeps (its label's words).
    ByLabel,
    /// The list has the launch id under more than one listed id: the launch id stands for it.
    LaunchId,
}

/// The models a harness's list holds, each as its listed id and the launch ids it has by effort.
pub type ModelsList = Vec<(String, Vec<String>)>;

/// The listed id of a row that holds `model` at `effort`, and how it was found. A harness whose
/// launch passes the effort inside the id (Devin's `model_uid`: `swe-2-max-lightning` for the
/// listed `swe-2-lightning` at `max`) is known by its list holding launch ids, not by its name:
/// where `list` (the home's, for the row's harness) holds the row's launch id under one listed
/// id, that is it; under several, the launch id stands for it. Where the list has no such row,
/// the effort's word is taken out of the launch id if it is among its words once and what is
/// left reads as the row's own label does (`label`'s words), the label being the name the list
/// gave the model when the row was made. Any other row is taken as it is.
pub fn listed_model(
    model: &str,
    effort: Option<&str>,
    label: Option<&str>,
    list: &ModelsList,
) -> (String, Listed) {
    let mut found: Vec<&str> = list
        .iter()
        .filter(|(_, launches)| launches.iter().any(|launch| launch == model))
        .map(|(id, _)| id.as_str())
        .collect();
    found.dedup();
    match found.as_slice() {
        [id] => return ((*id).to_owned(), Listed::FromList),
        [] => {}
        _ => return (model.to_owned(), Listed::LaunchId),
    }
    if list.iter().any(|(id, _)| id == model) {
        return (model.to_owned(), Listed::AsTaken);
    }
    if let (Some(effort), Some(label)) = (effort, label) {
        let words: Vec<&str> = model.split('-').collect();
        let at: Vec<usize> = (0..words.len()).filter(|at| words[*at] == effort).collect();
        if let [at] = at.as_slice() {
            let mut rest = words.clone();
            rest.remove(*at);
            let stripped = rest.join("-");
            if !stripped.is_empty() && slug(&[&stripped]) == slug(&[label]) {
                return (stripped, Listed::ByLabel);
            }
        }
    }
    (model.to_owned(), Listed::AsTaken)
}

/// The list the home ledger holds for the harness `kind` (as a row names it), if it holds one.
fn list_of(db: &Connection, kind: &str) -> Result<ModelsList, LedgerError> {
    let Some(harness) = cf_proto::agents::Harness::from_kind(kind) else {
        return Ok(Vec::new());
    };
    let models: Option<String> = db
        .query_row(
            "SELECT models FROM model_list WHERE harness = ?",
            [harness.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(models) = models else {
        return Ok(Vec::new());
    };
    let models: serde_json::Value = serde_json::from_str(&models)?;
    Ok(models
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let id = model.get("id")?.as_str()?.to_owned();
            let launches = model
                .get("launchIds")
                .and_then(serde_json::Value::as_object)
                .into_iter()
                .flat_map(|ids| ids.values())
                .filter_map(|launch| launch.as_str().map(str::to_owned))
                .collect();
            Some((id, launches))
        })
        .collect())
}

/// What migration 0033 did, counted.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Renamed {
    /// Members with a model, whose name is now its model's id and effort.
    pub members: usize,
    /// Chiefs with a model, whose row now says it by its id: a chief's name is its role and stays.
    pub chiefs: usize,
    /// Of them, those whose name changed.
    pub changed: usize,
    /// The windows of those that changed, named after them.
    pub windows: usize,
    /// Rows that hold a launch id, by how their listed id was found.
    pub by_list: usize,
    pub by_label: usize,
    pub by_launch_id: usize,
}

/// Migration 0033's part SQL cannot do: every member with a model (a member of the staff or
/// gone from it) takes the name its model's id and effort make, in the order the members
/// joined, and each of its windows its member's new name with the window's own two words
/// after. A participant's `model_name` becomes the model as its harness's list names it
/// ([`listed_model`]): the chief's too, with its handle as it is (a chief's name is its role),
/// so that no reader shows a chief by a label where every member is shown by its id. A member
/// on its harness's default, and one that needs an edit, keep the name they have until they get
/// a model, and a chief with none has none. Each name a participant leaves is kept in
/// `former_handles`; tasks, messages and every other row point at participants by id, so their
/// authors stay.
pub(crate) fn name_members_by_id(db: &Connection) -> Result<Renamed, LedgerError> {
    let rows = db
        .prepare(
            "SELECT id, project_id, handle, harness, model, effort, model_name, role = 'chief'
             FROM participant
             WHERE member_id IS NULL AND role <> 'human' AND needs_edit = 0
               AND model IS NOT NULL AND harness IS NOT NULL
             ORDER BY project_id, id",
        )?
        .query_map([], |row| {
            Ok((
                Member {
                    id: row.get(0)?,
                    project: row.get(1)?,
                    handle: row.get(2)?,
                    harness: row.get(3)?,
                    model: row.get(4)?,
                    effort: row.get(5)?,
                    model_name: row.get(6)?,
                },
                row.get::<_, bool>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut done = Renamed::default();
    let mut lists: std::collections::HashMap<String, ModelsList> = std::collections::HashMap::new();
    // Every member's old handle goes first, so a new one may be one an old one was. A chief
    // keeps its handle.
    for (member, chief) in &rows {
        if !chief {
            db.execute(
                "UPDATE participant SET handle = ? WHERE id = ?",
                params![format!("\u{1}{}", member.id), member.id],
            )?;
        }
    }
    for (member, chief) in &rows {
        let harness = member.harness.as_deref().unwrap_or_default();
        let model = member.model.as_deref().unwrap_or_default();
        let list = match lists.get(harness) {
            Some(list) => list,
            None => lists
                .entry(harness.to_owned())
                .or_insert(list_of(db, harness)?),
        };
        let (listed, how) = listed_model(
            model,
            member.effort.as_deref(),
            member.model_name.as_deref(),
            list,
        );
        match how {
            Listed::FromList => done.by_list += 1,
            Listed::ByLabel => done.by_label += 1,
            Listed::LaunchId => done.by_launch_id += 1,
            Listed::AsTaken => {}
        }
        db.execute(
            "UPDATE participant SET model_name = ? WHERE id = ? OR member_id = ?",
            params![listed, member.id, member.id],
        )?;
        if *chief {
            done.chiefs += 1;
            continue;
        }
        let handle = model_handle(&listed, member.effort.as_deref(), harness, |handle| {
            handle_taken(db, member.project, handle).unwrap_or(true)
        });
        done.members += 1;
        if handle != member.handle {
            done.changed += 1;
            let windows: i64 = db.query_row(
                "SELECT COUNT(*) FROM participant WHERE member_id = ?",
                [member.id],
                |row| row.get(0),
            )?;
            done.windows += usize::try_from(windows).unwrap_or_default();
        }
        rename_member(db, member.project, member.id, &member.handle, &handle)?;
    }
    Ok(done)
}

/// A member as the migration names it.
struct Member {
    id: i64,
    project: i64,
    handle: String,
    harness: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    model_name: Option<String>,
}

#[cfg(test)]
mod tests;
