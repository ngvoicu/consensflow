//! A window's command that waits on a macOS privacy dialog drawn in the app's name (T-1194).
//!
//! On a Mac every window's process has the app as its responsible process, so a folder or a
//! library that a worker's command touches (`ls ~/Music`, `find ~`, a project on the home folder)
//! is asked of the system in the app's name. The system draws the dialog ("Allow “ConsensFlow” to
//! access your music, video activity, and media library in Apple Music?") on the Mac's screen, not
//! in the window; the command waits with no timeout of its own until a person answers; and the
//! board said nothing (field, 3.0.7-alpha.1, 2026-11-03). The pane host reads the system's log of
//! those requests and tells the daemon which window's tree holds the process that waits, and for
//! which service (`pane.tcc`, [`cf_proto::panes::TccPrompt`]); this is what the engine does of it.
//!
//! - **A dialog for a process of a window** is told once for each window and service, to whoever
//!   gave the window's task and to the human, in words that name the window, the program, the
//!   service and what the dialog is: it is on the Mac's screen, the window waits until someone
//!   answers there, and "Don't Allow" is the safe answer unless the task needs what it guards.
//!   Nothing is pressed, as the dialog is the system's and no key of the window answers it.
//! - **The window waits** while the dialog does: nothing is pasted into it ([`Waiting`], as a dialog
//!   of its harness's holds it, `dialogs`). The host says nothing when a dialog is answered, so the
//!   wait ends at the first of three: the window's record moves (its command ended, and the harness
//!   wrote what it left), no alert of the system's notification centre is on the Mac's screen any
//!   more ([`crate::host::EngineHost::alerts`], looked at from [`FIRST_LOOK_MS`] on, once in
//!   [`LOOK_EVERY_MS`]), or [`HOLD_MS`] have gone. A host that cannot count alerts never ends it by
//!   them. Once a service is told for a window it is not told again for that window, whatever
//!   follows; a window that closes forgets them.
//! - **A process of the app's that no window holds** (a job a shell detached, a helper) is told to
//!   the human alone, once for each service while the daemon runs, in the open project with the
//!   lowest number (else the lowest there is).
//! - **A log the host cannot read** (a Mac user outside the admin group is refused by
//!   `log stream`) is said once, in the daemon's log and as one note to the human: no dialog is
//!   told then, and the note says so. That is the limit.
//! - **A window opened on the home folder** is told to the human once (a note of its own, before
//!   any dialog): everything that looks around in it asks, one dialog after another.
//!
//! Limits: a dialog an alert of another app keeps on screen is held to the bound; a process that
//! left its window's tree (`setsid`, its shell gone) is told as no window's; and what a harness's
//! own process asks of the system in the app's name is a window's, as its tree holds it.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

use cf_harness::contract::{Observed, Pane, Waiting};
use cf_ledger::{NewNote, ParticipantView, ProjectView};
use cf_proto::panes::{TccPrompt, TccUnreadable};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;

/// How long a window waits on a dialog at most, whatever the host says of the alerts on screen.
pub(crate) const HOLD_MS: i64 = 10 * 60 * 1_000;

/// How long after a dialog was told the first look at the alerts on screen is made: a dialog
/// takes a moment to be drawn, and an answer that came before it is the record's to show.
pub(crate) const FIRST_LOOK_MS: i64 = 3_000;

/// How often at most the alerts on screen are looked at while a window waits.
pub(crate) const LOOK_EVERY_MS: i64 = 5_000;

/// What the engine keeps across windows.
#[derive(Debug, Default)]
pub(crate) struct PrivacyState {
    /// Whether the unreadable log was said.
    unreadable: Cell<bool>,
    /// The services told of a process that no window holds.
    loose: RefCell<HashSet<String>>,
    /// The participants whose window opened on the home folder was told of.
    home: RefCell<HashSet<i64>>,
}

/// What one window keeps.
#[derive(Debug, Default)]
pub(crate) struct PrivacyPart {
    /// The services told of in this window.
    told: HashSet<String>,
    /// What it waits on, while it does.
    hold: Option<Hold>,
}

/// A window's wait on dialogs.
#[derive(Debug, Clone)]
struct Hold {
    /// When the first dialog was told.
    since: i64,
    /// How many items its record held at the first look after, which it moving from ends the
    /// wait.
    items: Option<usize>,
    /// When the alerts on screen were last looked at.
    looked: Option<i64>,
    /// What the dialogs guard, in words, in the order they were told.
    on: Vec<String>,
}

impl Dispatcher {
    /// A dialog raised in the app's name, for a process of a window or of the app's own, which
    /// the pane host read from the system's log (`pane.tcc`).
    pub fn privacy_prompt(self: &Rc<Self>, prompt: TccPrompt) {
        if let Err(cause) = self.told_of_prompt(&prompt) {
            self.write_down(&cause);
        }
    }

    /// The pane host cannot read the system's log of those dialogs (`pane.tcc_unreadable`).
    pub fn privacy_unreadable(self: &Rc<Self>, event: TccUnreadable) {
        if self.privacy.unreadable.replace(true) {
            return;
        }
        let said = if event.said.trim().is_empty() {
            "it said nothing".to_owned()
        } else {
            event.said.trim().to_owned()
        };
        self.seams.log.warn(&format!(
            "the log of macOS's privacy dialogs cannot be read ({said}): a dialog a window's \
             command raises will not be told"
        ));
        if let Err(cause) = self.told_unreadable(&event, &said) {
            self.write_down(&cause);
        }
    }

    fn told_of_prompt(&self, prompt: &TccPrompt) -> Result<(), EngineError> {
        let pane = prompt
            .id
            .clone()
            .zip(prompt.generation)
            .map(|(id, generation)| Pane { id, generation });
        match pane.and_then(|pane| self.record_of_pane(&pane)) {
            Some(record) => self.prompt_in_window(&record, prompt),
            None => self.prompt_in_no_window(prompt),
        }
    }

    fn prompt_in_window(&self, record: &Rc<Record>, prompt: &TccPrompt) -> Result<(), EngineError> {
        let Some(project) = self.project_of(record.id)? else {
            return Ok(());
        };
        let Some(participant) = project
            .participants
            .iter()
            .find(|participant| participant.id == record.id)
            .cloned()
        else {
            return Ok(());
        };
        let now = self.now();
        {
            let mut part = record.privacy.borrow_mut();
            if !part.told.insert(prompt.service.clone()) {
                return Ok(());
            }
            let hold = part.hold.get_or_insert_with(|| Hold {
                since: now,
                items: None,
                looked: None,
                on: Vec::new(),
            });
            hold.on.push(service_words(&prompt.service));
        }
        // To whoever gave the window's task and to the human: a chief's is the human's alone.
        self.tell_dialog(
            &project,
            &participant,
            &told_in_window(&participant.handle, prompt),
        )?;
        Ok(())
    }

    fn prompt_in_no_window(&self, prompt: &TccPrompt) -> Result<(), EngineError> {
        if !self
            .privacy
            .loose
            .borrow_mut()
            .insert(prompt.service.clone())
        {
            return Ok(());
        }
        let Some(project) = self.first_open_project()? else {
            return Ok(());
        };
        self.tell_the_human(&project, &told_in_no_window(prompt))
    }

    fn told_unreadable(&self, event: &TccUnreadable, said: &str) -> Result<(), EngineError> {
        let named = event
            .id
            .clone()
            .zip(event.generation)
            .map(|(id, generation)| Pane { id, generation })
            .and_then(|pane| self.record_of_pane(&pane))
            .map(|record| self.project_of(record.id))
            .transpose()?
            .flatten();
        let project = match named {
            Some(project) => Some(project),
            None => self.first_open_project()?,
        };
        match project {
            Some(project) => self.tell_the_human(&project, &told_unreadable(said)),
            None => Ok(()),
        }
    }

    /// The project a report that is no window's is told in: the open one with the lowest number,
    /// else (a daemon just started leaves every project suspended until the human's Resume) the
    /// lowest number there is.
    fn first_open_project(&self) -> Result<Option<ProjectView>, EngineError> {
        let projects = self.seams.ledger.borrow().projects()?;
        Ok(projects
            .iter()
            .filter(|project| project.state == "open")
            .min_by_key(|project| project.id)
            .or_else(|| projects.iter().min_by_key(|project| project.id))
            .cloned())
    }

    /// `body`, as a note of ConsensFlow's to the human in `project`.
    fn tell_the_human(&self, project: &ProjectView, body: &str) -> Result<(), EngineError> {
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: body.to_owned(),
            },
        )?;
        self.changed();
        Ok(())
    }

    /// The look at a window that waits on a dialog: it goes on waiting, as a window whose harness
    /// says it waits does, until its record moves, no alert is on screen any more, or the bound.
    pub(crate) async fn watch_privacy(
        &self,
        record: &Rc<Record>,
        mut observed: Observed,
    ) -> Observed {
        let Some(hold) = record.privacy.borrow().hold.clone() else {
            return observed;
        };
        let now = self.now();
        let items = observed.items().len();
        let baseline = hold.items.unwrap_or(items);
        let mut over = items != baseline || now - hold.since >= HOLD_MS;
        let due = now - hold.since >= FIRST_LOOK_MS
            && hold
                .looked
                .is_none_or(|looked| now - looked >= LOOK_EVERY_MS);
        if !over && due {
            if let Some(part) = record.privacy.borrow_mut().hold.as_mut() {
                part.looked = Some(now);
            }
            let alerts = self.seams.host.alerts().await;
            if self.forgotten(record) {
                return observed;
            }
            over = matches!(alerts, Ok(Some(0)));
        }
        let on = {
            let mut part = record.privacy.borrow_mut();
            if over {
                part.hold = None;
                return observed;
            }
            let Some(held) = part.hold.as_mut() else {
                return observed;
            };
            held.items.get_or_insert(items);
            held.on.clone()
        };
        // A dialog of its harness's, or its own question, says why it waits in its words.
        if observed.waiting.is_none() {
            observed.waiting = Some(Waiting {
                reason: Some(format!(
                    "a macOS permission dialog waits for an answer at the Mac ({})",
                    on.join(", ")
                )),
            });
        }
        observed
    }

    /// A window opened on the home folder (or in the Music folder) is told to the human, once for
    /// each participant while the daemon runs: `home` is the home folder its program was given,
    /// and `folder` the folder it opened on. A Mac's alone.
    pub(crate) fn home_note(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        home: Option<&str>,
        folder: &str,
    ) {
        if !cfg!(target_os = "macos") {
            return;
        }
        let Some(home) = home else {
            return;
        };
        let real = cf_base::file::real_path(Path::new(home))
            .to_string_lossy()
            .into_owned();
        let Some(opened) = gated_folder(folder, home).or_else(|| gated_folder(folder, &real))
        else {
            return;
        };
        if !self.privacy.home.borrow_mut().insert(participant.id) {
            return;
        }
        let body = told_on_the_home(&participant.handle, opened);
        if let Err(cause) = self.tell_the_human(project, &body) {
            self.write_down(&cause);
        }
    }
}

/// A folder a window opened on that macOS guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Opened {
    /// The home folder itself: everything in it that looks around asks.
    Home,
    /// The Music folder, or one in it: it holds the Apple Music library.
    Music,
}

/// Whether `folder` is the home folder `home`, or the Music folder or one in it. Not Desktop,
/// Documents or Downloads: a project there is asked once, and the one grant is the answer.
pub(crate) fn gated_folder(folder: &str, home: &str) -> Option<Opened> {
    let (folder, home) = (folder.trim_end_matches('/'), home.trim_end_matches('/'));
    if home.is_empty() {
        return None;
    }
    let music = format!("{home}/Music");
    if folder == home {
        Some(Opened::Home)
    } else if folder == music || folder.starts_with(&format!("{music}/")) {
        Some(Opened::Music)
    } else {
        None
    }
}

/// What a service the system names guards, in words (`MediaLibrary` is Apple Music's library).
pub(crate) fn service_words(service: &str) -> String {
    match service {
        "MediaLibrary" => "your music and media library (Apple Music)",
        "Photos" | "PhotosAdd" => "your Photos library",
        "SystemPolicyDesktopFolder" => "the Desktop folder",
        "SystemPolicyDocumentsFolder" => "the Documents folder",
        "SystemPolicyDownloadsFolder" => "the Downloads folder",
        "SystemPolicyRemovableVolumes" => "files on a removable drive",
        "SystemPolicyNetworkVolumes" => "files on a network volume",
        "SystemPolicyAppData" | "SystemPolicyAppDataDetailed" => "the data of other apps",
        "AddressBook" | "ContactsFull" | "ContactsLimited" => "your Contacts",
        "Calendar" => "your Calendars",
        "Reminders" => "your Reminders",
        "Camera" => "the camera",
        "Microphone" => "the microphone",
        "ScreenCapture" => "recording the screen",
        "Accessibility" => "control of the computer (Accessibility)",
        "ListenEvent" => "watching the keyboard (Input Monitoring)",
        "PostEvent" => "typing for you (Post Events)",
        "AppleEvents" => "control of another app (Automation)",
        other => return format!("\u{201c}{other}\u{201d}"),
    }
    .to_owned()
}

/// What the human and the task's giver are told of a dialog some command of `handle`'s window
/// waits on.
pub(crate) fn told_in_window(handle: &str, prompt: &TccPrompt) -> String {
    let guards = service_words(&prompt.service);
    format!(
        "@{handle}'s window waits on a macOS permission dialog: `{}` (process {}) in it asked for {guards}, \
         and macOS asks whether ConsensFlow may allow that, on the Mac's screen, not in the window. The command \
         waits until someone answers there, and nothing is typed into the window meanwhile. \
         \u{201c}Don\u{2019}t Allow\u{201d} is the safe answer unless the task needs {guards}.",
        prompt.program, prompt.pid
    )
}

/// What the human is told of a dialog a process of the app's that no window holds waits on.
pub(crate) fn told_in_no_window(prompt: &TccPrompt) -> String {
    let guards = service_words(&prompt.service);
    format!(
        "A process of ConsensFlow's that no window holds (`{}`, process {}) waits on a macOS permission dialog: it \
         asked for {guards}, and macOS asks whether ConsensFlow may allow that, on the Mac's screen, not in a \
         window. It waits until someone answers there. \u{201c}Don\u{2019}t Allow\u{201d} is the safe answer \
         unless you started it for {guards}.",
        prompt.program, prompt.pid
    )
}

/// What the human is told when the log of the dialogs cannot be read.
pub(crate) fn told_unreadable(said: &str) -> String {
    format!(
        "ConsensFlow cannot read the Mac's log of privacy dialogs ({said}), so it will not tell you when a \
         window's command waits on one. macOS draws such a dialog on the Mac's screen, in ConsensFlow's name, when \
         a command looks in a protected folder or library (find ~, ls ~/Music, a project on the home folder), and \
         the window waits until someone answers there; \u{201c}Don\u{2019}t Allow\u{201d} is the safe answer unless \
         the task needs it. Reading that log needs a Mac user in the admin group."
    )
}

/// What the human is told of a window of `handle`'s opened on a folder macOS guards.
pub(crate) fn told_on_the_home(handle: &str, opened: Opened) -> String {
    match opened {
        Opened::Home => format!(
            "@{handle}'s window opened on your home folder. When a command in it looks around (find, grep -r or \
             ls over the home folder), macOS asks whether ConsensFlow may open Music, Photos, Desktop, Documents \
             and Downloads, and the command waits for your answer on the Mac's screen, not in the window. Open the \
             project on a subfolder to avoid it; \u{201c}Don\u{2019}t Allow\u{201d} is the safe answer to those \
             dialogs unless the task needs them."
        ),
        Opened::Music => format!(
            "@{handle}'s window opened in your Music folder. macOS asks whether ConsensFlow may access your music \
             and media library (Apple Music) when a command in it looks there, and the command waits for your \
             answer on the Mac's screen, not in the window. Open the project on a folder outside Music to avoid \
             it; \u{201c}Don\u{2019}t Allow\u{201d} is the safe answer unless the task needs the library."
        ),
    }
}

#[cfg(test)]
mod tests;
