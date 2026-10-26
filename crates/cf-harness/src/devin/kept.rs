//! What ConsensFlow keeps of a Devin launch whose folder goes, until a start has
//! settled what was on its way into a window (gap 5 of the Devin arrival gaps,
//! 2026-10-18).
//!
//! The record reader takes a user's message for the first message ConsensFlow gave a
//! launch behind `/bypass` only where it is, whole, that launch's prompt file, and only
//! for a session the launch's wire log named (`answer::behind_bypass`): a human's own
//! `/bypass` text is never a delivery. Both files are in the launch's folder, which a
//! start sweeps before it settles what was on its way, and a window's exit removes
//! before it reads the record for the paste it had on its way (a chief's handoff given
//! again behind `/bypass`). So a first message Devin stored with the command kept was
//! read as stored, given back to its queue, and given again. So the removal keeps, for
//! a launch that gave such a prompt, the prompt and the sessions its log named, in
//! `integrations/devin-kept/<launch>.json`, where no sweep reaches; the reader reads
//! them beside the launches' own; and they go once a start has settled what was on its
//! way ([`forget_kept`]). A settle that fails leaves them for the next start's.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use cf_base::file::is_missing;
use cf_base::path;
use serde_json::{json, Value};

use super::adapter::BYPASS;
use crate::shared::record::find::entries;

/// The folder under `integrations` a start keeps what it swept of Devin's launches in:
/// beside Devin's own, which holds launches' folders and nothing else (`wires`).
pub(crate) const FOLDER: &str = "devin-kept";

/// What a kept file is named by: its launch's id and this.
const KIND: &str = ".json";

/// What the launch folder `launch`, named `name`, proves of its first message, kept in
/// `kept`: the prompt ConsensFlow gave it behind `/bypass`, and the sessions its wire log
/// names. Nothing where it gave none so, or its log names none, as Devin stored nothing
/// of it then. Written whole: a start that stops halfway leaves no file half written.
pub(crate) fn keep(launch: &Path, name: &str, kept: &Path) -> io::Result<()> {
    let Some(prompt) = behind_bypass(&launch.join("prompt.txt"))? else {
        return Ok(());
    };
    let sessions = sessions(&launch.join("wire.jsonl"))?;
    if sessions.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(kept)?;
    let partial = kept.join(format!("{name}{KIND}.partial"));
    fs::write(
        &partial,
        json!({ "sessions": sessions, "prompt": prompt }).to_string(),
    )?;
    fs::rename(&partial, kept.join(format!("{name}{KIND}")))
}

/// What the prompt file `file` (`install::prepare_prompt`) gave its launch behind
/// `/bypass`: none where it has none, or one that is not behind the command.
pub(super) fn behind_bypass(file: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(file) {
        Ok(prompt) => Ok(prompt
            .strip_prefix(BYPASS)
            .and_then(|behind| behind.strip_prefix(' '))
            .map(str::to_owned)),
        Err(error) if is_missing(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The sessions a wire log names: none where there is no log. A line torn or not JSON
/// names none.
fn sessions(log: &Path) -> io::Result<BTreeSet<String>> {
    let file = match fs::File::open(log) {
        Ok(file) => file,
        Err(error) if is_missing(&error) => return Ok(BTreeSet::new()),
        Err(error) => return Err(error),
    };
    let mut named = BTreeSet::new();
    let mut lines = BufReader::new(file);
    let mut line = Vec::new();
    while lines.read_until(b'\n', &mut line)? > 0 {
        if line.windows(11).any(|said| said == b"\"sessionId\"") {
            if let Some(session) = serde_json::from_slice::<Value>(&line)
                .ok()
                .and_then(|event| event.get("sessionId")?.as_str().map(str::to_owned))
            {
                named.insert(session);
            }
        }
        line.clear();
    }
    Ok(named)
}

/// What was kept, by its file's name, each read once: a file is written whole, once,
/// and never again ([`keep`]). A file that is not one written so proves nothing.
#[derive(Default)]
pub(super) struct KeptFiles(BTreeMap<String, Option<Kept>>);

/// One launch's proof: the sessions its log named, and the prompt behind `/bypass`.
struct Kept {
    sessions: Vec<String>,
    prompt: String,
}

impl KeptFiles {
    /// Reads the folder `kept` again: a file new since the last read is read, and one
    /// gone is forgotten. None is where nothing is kept.
    pub(super) fn read(&mut self, kept: &str) -> Result<(), String> {
        let names: BTreeSet<String> = match entries(Path::new(kept)) {
            Ok(found) => found
                .into_iter()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(KIND))
                .collect(),
            Err(error) if is_missing(&error) => BTreeSet::new(),
            Err(error) => return Err(format!("{kept}: {error}")),
        };
        self.0.retain(|name, _| names.contains(name));
        for name in names {
            if self.0.contains_key(&name) {
                continue;
            }
            let file = path::join(&[kept, &name]);
            let said = match fs::read(&file) {
                Ok(said) => said,
                Err(error) if is_missing(&error) => continue,
                Err(error) => return Err(format!("{file}: {error}")),
            };
            self.0.insert(name, Kept::of(&said));
        }
        Ok(())
    }

    /// The prompts kept for `session`, each as it was behind the command, in the order
    /// of their files' names.
    pub(super) fn for_session(&self, session: &str) -> Vec<String> {
        self.0
            .values()
            .flatten()
            .filter(|kept| kept.sessions.iter().any(|named| named == session))
            .map(|kept| kept.prompt.clone())
            .collect()
    }
}

impl Kept {
    /// What a file [`keep`] wrote says: none for any other.
    fn of(said: &[u8]) -> Option<Self> {
        let said: Value = serde_json::from_slice(said).ok()?;
        Some(Self {
            sessions: said
                .get("sessions")?
                .as_array()?
                .iter()
                .map(|named| named.as_str().map(str::to_owned))
                .collect::<Option<_>>()?,
            prompt: said.get("prompt")?.as_str()?.to_owned(),
        })
    }
}

/// What was kept goes, once a start has settled what was on its way: each file on its
/// own, so one that cannot go stays for the next start's settle and the rest go. Why
/// each that stayed did, by its file.
pub fn forget_kept(home: &str) -> Vec<String> {
    let kept = path::join(&[home, "integrations", FOLDER]);
    let found = match entries(Path::new(&kept)) {
        Ok(found) => found,
        Err(error) if is_missing(&error) => return Vec::new(),
        Err(error) => return vec![format!("{kept}: {error}")],
    };
    let mut stayed = Vec::new();
    for entry in found {
        let file = entry.path();
        match fs::remove_file(&file) {
            Err(error) if !is_missing(&error) => {
                stayed.push(format!("{}: {error}", file.display()));
            }
            _ => {}
        }
    }
    stayed
}
