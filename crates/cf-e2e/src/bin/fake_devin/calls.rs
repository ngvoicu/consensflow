//! A turn's calls of its tools, each asked of the window's permission mode first
//! ([`Permission::call`]): one that runs is the tool's, and one that does not is said
//! where Devin says it, on the wire (the call failed, with Devin's mark where it was
//! refused for its permission), in the store (what the call came back with) and on the
//! screen. The tool a line `FILE <name> <words>` asks for makes the file `name` in the
//! window's folder, once the turn has worked, which proves the call ran on every
//! platform: Devin's own write and shell are what B1's calls were.

use std::io::Write;
use std::path::Path;

use crate::permission::{Call, Permission, CANCELED, DIALOG, SKIPPED};
use crate::said;
use crate::store::Store;
use crate::wire::Wire;

/// What the tool that makes a file writes in it.
pub const MADE: &str = "made by the turn's tool\n";

/// Where a turn says what it does, and the call it is at: the parts of a window a call
/// is said in.
pub struct Said<'a, W: Write> {
    pub permission: &'a Permission,
    pub wire: &'a mut Wire,
    pub store: &'a mut Store,
    pub screen: &'a mut W,
    /// The turn the call is of, and the call's own name.
    pub client: &'a str,
    pub call: String,
}

impl<W: Write> Said<'_, W> {
    fn show(&mut self, text: &str) -> Result<(), String> {
        self.screen.write_all(text.as_bytes()).map_err(said)?;
        self.screen.flush().map_err(said)
    }

    /// Asks the mode whether the call of `what` runs, a prompt file's turn where
    /// `prompt_file`: what became of it, said where Devin says it where it does not run.
    pub fn ask(&mut self, what: &str, prompt_file: bool) -> Result<Call, String> {
        let mut drawn = Ok(());
        let call = {
            let screen = &mut *self.screen;
            self.permission.call(prompt_file, || {
                drawn = screen
                    .write_all(format!("⏺ {what}\n{DIALOG}").as_bytes())
                    .and_then(|()| screen.flush())
                    .map_err(said);
            })
        };
        drawn?;
        let (words, rejected) = match call {
            Call::Runs => return Ok(call),
            Call::Skipped => (SKIPPED, true),
            Call::Cancelled => (CANCELED, false),
        };
        self.wire
            .call_failed(&self.call, words, rejected, self.client)
            .map_err(said)?;
        self.store.tool(words)?;
        self.show(&format!("⏺ {what}\n✗ {words}\n"))?;
        Ok(call)
    }

    /// The tool that makes the file `name` in `folder`, once the mode let its call run.
    pub fn make(&mut self, folder: &Path, name: &str) -> Result<(), String> {
        let what = format!("write {name}");
        self.wire.called(&what, self.client).map_err(said)?;
        std::fs::write(folder.join(name), MADE).map_err(said)?;
        self.store
            .tool(&format!("File created successfully at: {name}"))?;
        self.show(&format!("⏺ Wrote {name}\n"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    /// The parts of a window in a folder of its own, its mode `bypass` or not.
    struct Parts {
        folder: tempfile::TempDir,
        permission: Permission,
        wire: Wire,
        store: Store,
        screen: Vec<u8>,
    }

    fn parts(bypass: bool) -> Parts {
        let folder = tempfile::tempdir().unwrap();
        let wire = Wire::open(&folder.path().join("wire.jsonl"), "calm-river").unwrap();
        let store = Store::open(&folder.path().join("sessions.db"), "calm-river").unwrap();
        Parts {
            folder,
            permission: Permission::new(bypass),
            wire,
            store,
            screen: Vec::new(),
        }
    }

    impl Parts {
        fn said(&mut self) -> Said<'_, Vec<u8>> {
            Said {
                permission: &self.permission,
                wire: &mut self.wire,
                store: &mut self.store,
                screen: &mut self.screen,
                client: "client-1",
                call: "client-1-call-1".to_owned(),
            }
        }

        fn wire(&self) -> Vec<Value> {
            std::fs::read_to_string(self.folder.path().join("wire.jsonl"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    #[test]
    fn a_call_in_full_permission_runs_and_its_tool_makes_the_file() {
        let mut parts = parts(true);
        let folder = parts.folder.path().to_path_buf();
        assert_eq!(parts.said().ask("write made.txt", true), Ok(Call::Runs));
        parts.said().make(&folder, "made.txt").unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("made.txt")).unwrap(),
            MADE
        );
        let wire = parts.wire();
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["update"]["sessionUpdate"], "tool_call");
    }

    #[test]
    fn a_prompt_files_call_in_the_normal_mode_is_refused_where_devin_says_it() {
        let mut parts = parts(false);
        assert_eq!(parts.said().ask("write made.txt", true), Ok(Call::Skipped));
        let wire = parts.wire();
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["update"]["status"], "failed");
        assert_eq!(wire[0]["update"]["_meta"]["cognition.ai/rejected"], true);
        assert_eq!(wire[0]["update"]["content"][0]["content"]["text"], SKIPPED);
        let screen = String::from_utf8(parts.screen.clone()).unwrap();
        assert!(screen.contains(&format!("✗ {SKIPPED}")), "{screen}");
        assert!(!screen.contains("(Approve once)"), "no dialog: {screen}");
        assert!(!parts.folder.path().join("made.txt").exists());
    }
}
