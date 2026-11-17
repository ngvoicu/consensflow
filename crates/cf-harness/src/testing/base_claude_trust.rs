//! Claude Code's question about a folder it was never told to trust, as its window
//! shows it before anything else (2.1.305, `cargo xtask live folder-trust`): the
//! folder, a few lines that ask whether it is a project the person trusts, and two
//! choices, "No, exit" and "Yes, I trust this folder", the `❯` on the first. Enter
//! on "No, exit" ends the window; Down moves to the yes, which Enter confirms, and
//! Claude Code writes the trust down for the root of the folder's git repository
//! and goes on to its prompt, and to the first message it was opened with. Before
//! 2.1.287 the yes came first, numbered: the choice is read by its words. Its prompt,
//! once it is past the question, is told by the line under its input box.
//!
//! The same dialog asks again of a folder whose trust is already accepted, but by a
//! folder above it rather than for the folder itself (or its repository's root),
//! where the folder's own settings grant more than trusting it does (2.1.309, T-354:
//! a plain folder below a trusted one, or a repository trusted at a folder below its
//! root, whose `.claude/settings.json` pre-approves a tool, or whose `.mcp.json`
//! names a server with a header helper). Its no then reads "No, continue without
//! these permissions" and goes on into the window without them; its yes lets them
//! apply without asking, and writes the trust down for the root of the folder's
//! repository, or the folder itself out of one, as the first question's yes does
//! (T-381). It is answered as that question is ([`Question::Grants`]).
//!
//! Once the folder is trusted, a window whose folder's `.mcp.json` (or a folder
//! above's) names a server it was never told to use, nor to leave out, asks of it in
//! a dialog of its own (2.1.309, T-383): "New MCP server found in this project:",
//! the server's name, and three choices, "Use this MCP server", "Use this and all
//! future MCP servers in this project" and "Continue without using this MCP server",
//! its `❯` on the last; Up and Down move it, and wrap. The first keeps the server's
//! name in `enabledMcpjsonServers` of the folder's `.claude/settings.local.json` and
//! starts it; the second keeps `enableAllProjectMcpServers` there too; the last, or
//! Escape, keeps it in `disabledMcpjsonServers`. Two servers or more are asked of
//! together: "N new MCP servers found in this project", a box a line for each,
//! ticked, the `❯` on the first, then "Enable selected", on which Enter uses the
//! ticked ones; Enter on a box unticks it, and Down stops at the last line. It is
//! not asked where a window is shut out of the project's servers
//! (`--strict-mcp-config`), nor where full-permission mode was accepted in the
//! person's own settings (`skipDangerousModePermissionPrompt` in their
//! `settings.json`, not a `--settings` file's): it then uses them without asking
//! ([`Question::Servers`]).

use crate::contract::Question;
use crate::shared::trust::{chosen_at, names, offers, shows, DOWN, ENTER, UP};

/// What Claude's prompt shows as ConsensFlow opens it, in full-permission mode.
pub(crate) const PROMPT: &str = "bypass permissions on";

/// The choice that trusts the folder, and the one that ends the window.
const YES: &str = "Yes, I trust this folder";
const NO: &str = "No, exit";

/// The no of the question asked of a trusted folder's grants: it goes on without them.
const NO_GRANTS: &str = "No, continue without these permissions";

/// The choices of the dialog of a new server of `.mcp.json`: the one that uses it,
/// the one that uses every server the project will name, and the one that goes on
/// without it, on which it opens.
const SERVER_YES: &str = "Use this MCP server";
const SERVER_ALL: &str = "Use this and all future MCP servers in this project";
const SERVER_NO: &str = "Continue without using this MCP server";

/// The line that names the server the dialog of one new server asks of.
const SERVER_TITLE: &str = "New MCP server found in this project:";

/// The choice of the dialog of two new servers or more that uses the ticked ones, the
/// line it draws below them (Claude Code's words for the `confirm:no` key, which a
/// person's own key bindings word otherwise), the words of its frame, and the ticks of
/// its boxes.
const SERVERS_YES: &str = "Enable selected";
const SERVERS_HINT: &str = "Esc to reject all";
const SERVERS_FRAME: [&str; 2] = [
    "new MCP servers found in this project",
    "Select any you wish to enable.",
];
const TICKED: &str = "[✔]";
const UNTICKED: &str = "[ ]";

/// The line the question draws below its choices (2.1.305), and no input box does.
/// It is made of the key bindings, and reads otherwise while a Ctrl-C is pending
/// ("Press Ctrl-C again to exit"): a cursor on a choice the reader does not know then
/// reads as no question, as it did before round 6.
const HINT: &str = "Enter to confirm";

/// The question, where `screen` shows it, and the key toward a yes: Enter once the
/// `❯` is on the yes, Down while it is on the no. The question is its two choices,
/// each a line of its own: a screen that only names them (a message that quotes the
/// question) asks nothing. A `❯` with the prompt's line below it is Claude's input
/// box, whatever it holds. A `❯` on a line that is neither choice is the question's
/// cursor on a choice the reader does not know where the question's hint line is
/// drawn below it, as the question draws it; elsewhere it is the input box, or a past
/// user turn (2.1.308 marks each with a `❯`), below a conversation that quotes the
/// choices, each on a line of its own (a list in a message): no question. Where there is no `❯` at all (a cursor drawn
/// otherwise, or a redraw caught halfway), the question is there and its key unknown.
pub(crate) fn toward_trust(screen: &[String]) -> Option<Question> {
    if let Some(drawn) = grants_drawn(screen) {
        return toward_trust(&drawn).map(|read| match read {
            Question::Toward(key) => Question::Grants(Some(key)),
            _ => Question::Grants(None),
        });
    }
    if !names_choices(screen) {
        return toward_servers(screen);
    }
    Some(match chosen_at(screen, '❯') {
        Some((at, _)) if shows(&screen[at + 1..], PROMPT) => return None,
        Some((_, choice)) if names(choice, YES) => Question::Toward(ENTER),
        Some((_, choice)) if names(choice, NO) => Question::Toward(DOWN),
        Some((at, _)) if shows(&screen[at + 1..], HINT) => Question::Unplaced,
        Some(_) => return None,
        None => Question::Unplaced,
    })
}

/// The dialog of a new server of `.mcp.json` (or of two or more), where `screen`
/// shows it, read by the rules of the question about the folder: its choices each a
/// line of its own, a `❯` with the prompt's line below it the input box. The key
/// toward using the servers: Up while the `❯` is below "Use this MCP server" (Up
/// moves it there whether or not a later Claude Code wraps), Enter on it; for two or
/// more, Down while it is on a box, Enter on "Enable selected", and none where a box
/// is unticked, which is the human's choice.
fn toward_servers(screen: &[String]) -> Option<Question> {
    let one = offers(screen, SERVER_YES) && offers(screen, SERVER_NO);
    let more = offers(screen, SERVERS_YES)
        && (shows(screen, SERVERS_HINT) || SERVERS_FRAME.iter().any(|words| shows(screen, words)));
    if !(one || more) {
        return None;
    }
    let boxed = |choice: &str| choice.starts_with(TICKED) || choice.starts_with(UNTICKED);
    Some(Question::Servers(match chosen_at(screen, '❯') {
        Some((at, _)) if shows(&screen[at + 1..], PROMPT) => return None,
        Some((_, choice)) if one && names(choice, SERVER_YES) => Some(ENTER),
        Some((_, choice)) if one && (names(choice, SERVER_ALL) || names(choice, SERVER_NO)) => {
            Some(UP)
        }
        Some(_) if more && shows(screen, UNTICKED) => None,
        Some((_, choice)) if more && names(choice, SERVERS_YES) => Some(ENTER),
        Some((_, choice)) if more && boxed(choice) => Some(DOWN),
        Some((at, _))
            if shows(&screen[at + 1..], HINT) || shows(&screen[at + 1..], SERVERS_HINT) =>
        {
            None
        }
        Some(_) => return None,
        None => None,
    }))
}

/// The servers the dialog of new servers on `screen` would use, as its lines name them
/// (a plugin's server as "name (from plugin X)"): the one its title names, or the
/// ticked boxes of the dialog of several. None where its lines do not show them.
pub(crate) fn servers_named(screen: &[String]) -> Vec<String> {
    if !matches!(toward_servers(screen), Some(Question::Servers(_))) {
        return Vec::new();
    }
    if let Some(named) = screen
        .iter()
        .find_map(|line| line.split_once(SERVER_TITLE).map(|(_, name)| name.trim()))
    {
        return vec![named.to_owned()];
    }
    screen
        .iter()
        .filter_map(|line| {
            let choice = line.trim_matches(|at: char| at.is_whitespace() || at == '❯');
            choice
                .strip_prefix(TICKED)
                .map(|name| name.trim().to_owned())
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// `screen` with the no of the question asked of a trusted folder's grants in the
/// words of the other question's, where it names that question's choices and not the
/// other's: it is drawn the same, and read by the same rules, its question the human's
/// wherever its `❯` is. None where it names other choices.
fn grants_drawn(screen: &[String]) -> Option<Vec<String>> {
    if !(offers(screen, YES) && offers(screen, NO_GRANTS)) || offers(screen, NO) {
        return None;
    }
    Some(
        screen
            .iter()
            .map(|line| line.replace(NO_GRANTS, NO))
            .collect(),
    )
}

/// Whether `screen` names both of the question's choices, each on a line of its own:
/// the question does, and so may a conversation that quotes it; a screen past the
/// question it was answered in does not.
pub(crate) fn names_choices(screen: &[String]) -> bool {
    offers(screen, YES) && offers(screen, NO)
}

/// Whether `screen` is Claude's prompt, as ConsensFlow opens its windows: the
/// line under its input box that says it is in full-permission mode ("⏵⏵ bypass
/// permissions on", 2.1.305), and no question whose choice is placed: the question
/// draws no such line, so one read with its choice unplaced beside it is quoted.
pub(crate) fn shows_prompt(screen: &[String]) -> bool {
    let placed = |drawn: Vec<String>| matches!(toward_trust(&drawn), Some(Question::Toward(_)));
    if grants_drawn(screen).is_some_and(placed) {
        return false;
    }
    shows(screen, PROMPT)
        && !matches!(
            toward_trust(screen),
            Some(Question::Toward(_) | Question::Servers(Some(_)))
        )
}
