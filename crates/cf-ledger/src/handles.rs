//! A member's name and handle, made from the model it runs on (spec `harness-models`,
//! HMD-05): never a nickname. Its name is what its harness's list calls the model
//! (`Opus`, `GPT-6-Astra`), its handle that name and the effort in lowercase words joined
//! by `-` (`@opus-high`). Where another participant of the project has that handle, the
//! harness goes in front (`@devin-claude-opus-5-5-high`), and where one has that too, a
//! number after (`-2`). A member's windows are named after it, two words more
//! ([`crate::names`]). The migration that took the members of the ready-made agents to
//! their models names them with the same rule (`schema::after`, 0031).

use rusqlite::{params, Connection};

use crate::model::LedgerError;

/// The most characters of a handle made here: a handle holds 64, and its windows add
/// two words and a number to it.
pub const MEMBER_HANDLE_MAX: usize = 40;

/// What a member's model is called: the name its harness's list gave it, else its id as
/// the harness takes it, else (a member on its harness's default, as the image designer
/// from before the models were the harnesses' own) the harness's default.
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

/// The handle of a member whose model is shown as `shown` (see [`model_shown`]), at
/// `effort`, on `harness` (its kind), where `taken` says which handles the project has:
/// the name and the effort; with the harness in front where that is taken; and with a
/// number after where that is too.
pub fn member_handle(
    shown: &str,
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
    let with_harness = slug(&[harness, shown, effort]);
    if !taken(&with_harness) {
        return with_harness;
    }
    (2..)
        .map(|n| format!("{with_harness}-{n}"))
        .find(|handle| !taken(handle))
        .unwrap_or(with_harness)
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
