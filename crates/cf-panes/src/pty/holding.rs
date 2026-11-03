//! How many windows are open, and which window holds a process (T-1194). The watcher of the
//! system's privacy dialogs runs while a window is open (`crate::tcc`), and a dialog the system
//! names by the number of the process that waits on it is told to the daemon with the window
//! whose tree holds that process.

use super::{PaneKey, PaneTable};

impl PaneTable {
    /// How many windows are open.
    pub fn open_count(&self) -> usize {
        self.lock_panes().map_or(0, |panes| panes.len())
    }

    /// Any window that is open: the one a report that is no window's in particular is told with.
    pub fn some_pane(&self) -> Option<PaneKey> {
        let panes = self.lock_panes().ok()?;
        panes.keys().min_by(|a, b| a.id.cmp(&b.id)).cloned()
    }

    /// The window whose tree holds the process `pid` now, where one does: a look at each
    /// tree, none holding the table's lock. A process in no window's tree (a job a shell
    /// detached and no look saw, or one of the app's own) is no window's.
    #[cfg(target_os = "macos")]
    pub(crate) fn pane_holding(&self, pid: u32) -> Option<PaneKey> {
        let trees: Vec<(PaneKey, crate::process_tree::ProcessTree)> = {
            let panes = self.lock_panes().ok()?;
            panes
                .iter()
                .map(|(key, pane)| (key.clone(), pane.process_tree.clone()))
                .collect()
        };
        trees
            .into_iter()
            .find(|(_, tree)| tree.holds(pid).unwrap_or(false))
            .map(|(key, _)| key)
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use portable_pty::PtySize;

    use super::*;
    use crate::pty::serial_pty_test;

    fn open_sleep(table: &PaneTable) -> (PaneKey, u32) {
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let argv = vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "/bin/sleep 30 & wait".to_owned(),
        ];
        let opened = table
            .open(Path::new("/tmp"), &argv, &HashMap::new(), size)
            .expect("a window");
        let pid = table
            .panes
            .lock()
            .unwrap()
            .get(&opened.key)
            .and_then(|pane| pane.child.process_id())
            .expect("its program's number");
        (opened.key, pid)
    }

    #[test]
    fn a_window_holds_its_program_and_the_sleep_below_it_and_this_test_is_no_windows() {
        let _serial = serial_pty_test();
        let table = PaneTable::new();
        assert_eq!(table.open_count(), 0);
        let (key, program) = open_sleep(&table);
        assert_eq!(table.open_count(), 1);

        assert_eq!(table.pane_holding(program), Some(key.clone()));
        // The sleep the program started: found in the window's own tree, then asked for by its number.
        let tree = table
            .panes
            .lock()
            .unwrap()
            .get(&key)
            .map(|pane| pane.process_tree.clone())
            .expect("its tree");
        let sleeper = tree
            .members()
            .expect("a look")
            .into_iter()
            .find(|member| member.program == "sleep")
            .expect("the sleep");
        assert_eq!(table.pane_holding(sleeper.key.pid), Some(key.clone()));
        assert_eq!(table.pane_holding(std::process::id()), None);

        table.kill(&key).expect("it ends");
        assert_eq!(table.open_count(), 0);
        assert_eq!(table.pane_holding(program), None);
    }
}
