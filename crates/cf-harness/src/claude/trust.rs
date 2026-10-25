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
const PROMPT: &str = "bypass permissions on";

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

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn the_question_opens_on_no_and_is_answered_with_down_then_enter() {
        // As Claude Code 2.1.305 draws it, its folder named above.
        let asked = lines(
            " Accessing workspace:\n /work/project\n Quick safety check: Is this a project you created or one you trust?\n Security guide\n ❯ No, exit\n   Yes, I trust this folder\n Enter to confirm · Esc to cancel",
        );
        assert_eq!(toward_trust(&asked), Some(Question::Toward(DOWN)));
        let on_yes = lines(
            " Quick safety check: Is this a project you created or one you trust?\n   No, exit\n ❯ Yes, I trust this folder\n Enter to confirm · Esc to cancel",
        );
        assert_eq!(toward_trust(&on_yes), Some(Question::Toward(ENTER)));
        // Before 2.1.287 the yes came first, numbered.
        let numbered = lines("❯ 1. Yes, I trust this folder\n  2. No, exit");
        assert_eq!(toward_trust(&numbered), Some(Question::Toward(ENTER)));
    }

    #[test]
    fn its_prompt_is_the_line_under_its_input_box_in_full_permission_mode() {
        // As Claude Code 2.1.305 draws it.
        let prompt = lines(" ❯ Try \"how do I log an error?\"\n   fresh Sonnet   0 tokens\n   ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents");
        assert!(shows_prompt(&prompt));
        let asked = lines(" Quick safety check\n ❯ No, exit\n   Yes, I trust this folder");
        assert!(!shows_prompt(&asked));
        assert!(!shows_prompt(&lines(" Accessing workspace:\n /work")));
    }

    #[test]
    fn a_message_that_quotes_the_question_asks_nothing() {
        // A conversation about this very dialog, as a resumed window shows it.
        let quoted = lines(" ⏺ Claude asks whether you trust the folder: it offers \"No, exit\" and\n   \"Yes, I trust this folder\", the cursor on the first.\n ❯ \n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        assert_eq!(toward_trust(&quoted), None);
        assert!(shows_prompt(&quoted));
        // The same before its input box is drawn: no `❯` at all, and still no question.
        assert_eq!(toward_trust(&quoted[..2]), None);
    }

    #[test]
    fn a_question_whose_cursor_is_on_neither_choice_is_asked_with_no_key_known() {
        // Its cursor drawn otherwise than Claude Code 2.1.305 draws it.
        let otherwise = lines(" Quick safety check: Is this a project you created or one you trust?\n > No, exit\n   Yes, I trust this folder\n Enter to confirm · Esc to cancel");
        assert_eq!(toward_trust(&otherwise), Some(Question::Unplaced));
        assert!(!shows_prompt(&otherwise));
        // Drawn halfway, its cursor not yet on a choice.
        let halfway = lines(" Quick safety check\n   No, exit\n   Yes, I trust this folder");
        assert_eq!(toward_trust(&halfway), Some(Question::Unplaced));
        // Numbered, as before 2.1.287.
        assert_eq!(
            toward_trust(&lines("  1. Yes, I trust this folder\n  2. No, exit")),
            Some(Question::Unplaced)
        );
    }

    #[test]
    fn a_conversation_that_lists_the_choices_above_the_input_box_asks_nothing() {
        // Calliope's check of round 4 (T-231): a resumed chief at rest whose last lines
        // quote the dialog, each choice a line of its own.
        let listed = lines(" ⏺ The dialog offers:\n   - No, exit\n   - Yes, I trust this folder\n ❯ \n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        assert_eq!(toward_trust(&listed), None);
        assert!(shows_prompt(&listed));
        // Its input box holding words that end with a choice is no cursor on it.
        let typed = lines(" ⏺ The dialog offers:\n   - No, exit\n   - Yes, I trust this folder\n ❯ tell me more about No, exit\n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        assert_eq!(toward_trust(&typed), None);
        assert!(shows_prompt(&typed));
        // The same where the prompt is drawn otherwise, with no line of its below.
        assert_eq!(toward_trust(&typed[..4]), None);
        // Quoting the dialog's hint line too, above an empty input box with no prompt
        // line below it (Calliope's last re-check, T-325).
        let hint_quoted = lines(" ⏺ The dialog offers:\n   - No, exit\n   - Yes, I trust this folder\n   and says: Enter to confirm · Esc to cancel\n ❯ ");
        assert_eq!(toward_trust(&hint_quoted), None);
        // Holding a choice's very words, the prompt's line below it.
        let typed_a_choice = lines(" ⏺ The dialog offers:\n   - No, exit\n   - Yes, I trust this folder\n ❯ No, exit\n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        assert_eq!(toward_trust(&typed_a_choice), None);
        assert!(shows_prompt(&typed_a_choice));
        // The same with no input box drawn yet: unplaced, but the prompt's line wins.
        let tail = lines(" ⏺ The dialog offers:\n   - No, exit\n   - Yes, I trust this folder\n   ⏵⏵ bypass permissions on");
        assert_eq!(toward_trust(&tail), Some(Question::Unplaced));
        assert!(shows_prompt(&tail));
    }

    #[test]
    fn a_cursor_on_a_choice_the_reader_does_not_know_is_the_question_with_its_cursor_unplaced() {
        // Calliope's re-check of round 5 (T-299): a third choice between the two, as a
        // later Claude Code may draw, the dialog's hint line below it.
        let third = lines(" Quick safety check: Is this a project you created or one you trust?\n   No, exit\n ❯ Yes, and remember this\n   Yes, I trust this folder\n\n Enter to confirm · Esc to cancel");
        assert_eq!(toward_trust(&third), Some(Question::Unplaced));
        assert!(!shows_prompt(&third));
    }

    /// The question of a trusted folder whose settings grant more, its `❯` on the no
    /// where `on_no`, as Claude Code 2.1.309 draws its last lines (T-354's probe: a
    /// plain folder below a trusted one, its `.claude/settings.json` pre-approving a
    /// tool).
    fn grants(on_no: bool) -> Vec<String> {
        let (no, yes) = if on_no { ("❯", " ") } else { (" ", "❯") };
        lines(&format!(" ⚠ This folder pre-approves 1 tool permission in .claude/settings.json:\n   Bash(ls:*)\n These will apply without asking. Only proceed if you trust this configuration.\n\n Security guide\n\n {no} No, continue without these permissions\n {yes} Yes, I trust this folder\n\n Enter to confirm · Esc to cancel"))
    }

    #[test]
    fn the_question_of_a_trusted_folder_whose_settings_grant_more_is_read_with_its_key_toward_a_yes(
    ) {
        // It opens on the no, which goes on into the window without the grants: Down,
        // then Enter on the yes, as the first question.
        assert_eq!(
            toward_trust(&grants(true)),
            Some(Question::Grants(Some(DOWN)))
        );
        assert_eq!(
            toward_trust(&grants(false)),
            Some(Question::Grants(Some(ENTER)))
        );
        assert!(!shows_prompt(&grants(true)) && !shows_prompt(&grants(false)));
        // A prompt's line above it, from before it was drawn, is no prompt.
        let mut below_a_prompt = lines("   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        below_a_prompt.extend(grants(true));
        assert!(!shows_prompt(&below_a_prompt));
        assert_eq!(
            toward_trust(&below_a_prompt),
            Some(Question::Grants(Some(DOWN)))
        );
        // Its cursor drawn otherwise, or not yet drawn: asked, no key known.
        let otherwise: Vec<String> = grants(true)
            .iter()
            .map(|line| line.replace('❯', ">"))
            .collect();
        assert_eq!(toward_trust(&otherwise), Some(Question::Grants(None)));
        let halfway: Vec<String> = grants(true)
            .iter()
            .map(|line| line.replace('❯', " "))
            .collect();
        assert_eq!(toward_trust(&halfway), Some(Question::Grants(None)));
        // A header helper of `.mcp.json`'s, as it names one.
        let helper = lines(" ⚠ This folder runs commands to mint HTTP headers (headersHelper), declared in .mcp.json\n These will apply without asking. Only proceed if you trust this configuration.\n\n ❯ No, continue without these permissions\n   Yes, I trust this folder\n\n Enter to confirm · Esc to cancel");
        assert_eq!(toward_trust(&helper), Some(Question::Grants(Some(DOWN))));
    }

    #[test]
    fn a_conversation_that_quotes_the_question_of_a_trusted_folders_grants_asks_nothing() {
        // Its choices listed above the input box, the prompt's line below it.
        let listed = lines(" ⏺ The dialog offers:\n   - No, continue without these permissions\n   - Yes, I trust this folder\n ❯ \n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        assert_eq!(toward_trust(&listed), None);
        assert!(shows_prompt(&listed));
        // Its very words typed in the input box.
        let typed = lines(" ⏺ The dialog offers:\n   - No, continue without these permissions\n   - Yes, I trust this folder\n ❯ No, continue without these permissions\n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        assert_eq!(toward_trust(&typed), None);
        assert!(shows_prompt(&typed));
        // A past user turn's `❯` above more of the conversation, no hint below it.
        let past = lines("   - No, continue without these permissions\n   - Yes, I trust this folder\n ❯ what does it mean?\n ⏺ It continues without them.");
        assert_eq!(toward_trust(&past), None);
        // Named among other words, or with one choice alone, it is no question.
        let prose = lines(
            " ⏺ It says No, continue without these permissions and Yes, I trust this folder.",
        );
        assert_eq!(toward_trust(&prose), None);
        assert_eq!(
            toward_trust(&lines(" ❯ No, continue without these permissions")),
            None
        );
        // The other question below a conversation that lists this one's no: that one.
        let below = lines("   - No, continue without these permissions\n Quick safety check\n ❯ No, exit\n   Yes, I trust this folder\n Enter to confirm · Esc to cancel");
        assert_eq!(toward_trust(&below), Some(Question::Toward(DOWN)));
    }

    /// The dialog of a new server of `.mcp.json`, its `❯` on the choice `on` (0 to 2),
    /// as Claude Code 2.1.309 draws its last lines (T-354's probe).
    fn server(on: usize) -> Vec<String> {
        let choices = [SERVER_YES, SERVER_ALL, SERVER_NO];
        let drawn: Vec<String> = choices
            .iter()
            .enumerate()
            .map(|(at, choice)| format!(" {} {choice}", if at == on { '❯' } else { ' ' }))
            .collect();
        lines(&format!(" New MCP server found in this project: probe\n\n MCP servers may execute code or access system resources. All tool calls require approval.\n\n{}\n\n Enter to confirm · Esc to cancel", drawn.join("\n")))
    }

    #[test]
    fn the_dialog_of_a_new_server_of_the_projects_mcp_json_is_a_question() {
        // T-354's known failure: as Claude Code 2.1.309 draws its last lines, its cursor
        // on the last choice, on which it opens.
        let asked = lines(" New MCP server found in this project: probe\n\n MCP servers may execute code or access system resources. All tool calls require approval.\n\n   Use this MCP server\n   Use this and all future MCP servers in this project\n ❯ Continue without using this MCP server\n\n Enter to confirm · Esc to cancel");
        assert_eq!(toward_trust(&asked), Some(Question::Servers(Some(UP))));
        assert_eq!(asked, server(2));
        assert!(!shows_prompt(&asked));
    }

    #[test]
    fn the_dialog_of_a_new_server_is_answered_up_to_using_it_then_enter() {
        // Up from the last choice, and from the one that uses every future server too,
        // whether or not the list wraps; Enter on the one that uses this server alone.
        assert_eq!(toward_trust(&server(2)), Some(Question::Servers(Some(UP))));
        assert_eq!(toward_trust(&server(1)), Some(Question::Servers(Some(UP))));
        assert_eq!(
            toward_trust(&server(0)),
            Some(Question::Servers(Some(ENTER)))
        );
        for on in 0..3 {
            assert!(!shows_prompt(&server(on)), "{on}");
            assert!(!names_choices(&server(on)), "{on}");
        }
        // A prompt's line above it, from before it was drawn, is no prompt.
        let mut below_a_prompt = lines("   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        below_a_prompt.extend(server(2));
        assert_eq!(
            toward_trust(&below_a_prompt),
            Some(Question::Servers(Some(UP)))
        );
        assert!(!shows_prompt(&below_a_prompt));
        // Its cursor drawn otherwise, or not yet drawn: asked, no key known.
        let otherwise: Vec<String> = server(2)
            .iter()
            .map(|line| line.replace('❯', ">"))
            .collect();
        assert_eq!(toward_trust(&otherwise), Some(Question::Servers(None)));
        let halfway: Vec<String> = server(2)
            .iter()
            .map(|line| line.replace('❯', " "))
            .collect();
        assert_eq!(toward_trust(&halfway), Some(Question::Servers(None)));
        // A choice the reader does not know, the dialog's hint below it.
        let mut third = server(5);
        third.insert(5, " ❯ Use this MCP server for this session only".to_owned());
        assert_eq!(toward_trust(&third), Some(Question::Servers(None)));
    }

    /// The dialog of two new servers, its `❯` on the line `on` (0 and 1 the boxes, 2
    /// "Enable selected"), the second box ticked where `ticked`, as Claude Code 2.1.309
    /// draws it (T-383's probe).
    fn servers(on: usize, ticked: bool) -> Vec<String> {
        let second = if ticked { TICKED } else { UNTICKED };
        let choices = [
            format!("{TICKED} probe"),
            format!("{second} probe2"),
            SERVERS_YES.to_owned(),
        ];
        let drawn: Vec<String> = choices
            .iter()
            .enumerate()
            .map(|(at, choice)| format!(" {} {choice}", if at == on { '❯' } else { ' ' }))
            .collect();
        lines(&format!(" 2 new MCP servers found in this project\n Select any you wish to enable.\n\n MCP servers may execute code or access system resources. All tool calls require approval.\n\n{}\n\n Space to select · Esc to reject all", drawn.join("\n")))
    }

    #[test]
    fn the_dialog_of_new_servers_is_answered_down_to_enable_selected_then_enter() {
        assert_eq!(
            toward_trust(&servers(0, true)),
            Some(Question::Servers(Some(DOWN)))
        );
        assert_eq!(
            toward_trust(&servers(1, true)),
            Some(Question::Servers(Some(DOWN)))
        );
        assert_eq!(
            toward_trust(&servers(2, true)),
            Some(Question::Servers(Some(ENTER)))
        );
        assert!(!shows_prompt(&servers(0, true)));
        // A box unticked is the human's choice: asked, no key.
        for on in 0..3 {
            assert_eq!(
                toward_trust(&servers(on, false)),
                Some(Question::Servers(None)),
                "{on}"
            );
        }
    }

    #[test]
    fn the_dialog_of_new_servers_is_read_by_its_frame_whatever_key_rejects_all() {
        // A person's own binding for `confirm:no` words the hint line otherwise (T-415).
        let bound: Vec<String> = servers(0, true)
            .iter()
            .map(|line| line.replace("Esc to reject all", "ctrl+r to reject all"))
            .collect();
        assert_eq!(toward_trust(&bound), Some(Question::Servers(Some(DOWN))));
        // Its title alone, or its subtitle alone, frames it.
        let titled: Vec<String> = bound
            .iter()
            .filter(|line| !line.contains("Select any"))
            .cloned()
            .collect();
        assert_eq!(toward_trust(&titled), Some(Question::Servers(Some(DOWN))));
        let subtitled: Vec<String> = bound
            .iter()
            .filter(|line| !line.contains("servers found"))
            .cloned()
            .collect();
        assert_eq!(
            toward_trust(&subtitled),
            Some(Question::Servers(Some(DOWN)))
        );
    }

    #[test]
    fn the_servers_the_dialog_would_use_are_named_as_its_lines_name_them() {
        assert_eq!(servers_named(&server(2)), ["probe"]);
        let plugin: Vec<String> = server(0)
            .iter()
            .map(|line| line.replace(": probe", ": probe (from plugin kit)"))
            .collect();
        assert_eq!(servers_named(&plugin), ["probe (from plugin kit)"]);
        assert_eq!(servers_named(&servers(2, true)), ["probe", "probe2"]);
        assert_eq!(servers_named(&servers(0, false)), ["probe"]);
        // No dialog, no servers: a conversation that quotes the title.
        assert_eq!(
            servers_named(&lines(" ⏺ It said New MCP server found in this project: probe\n ❯ \n   ⏵⏵ bypass permissions on")),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_conversation_that_quotes_the_dialog_of_a_new_server_asks_nothing() {
        // Its choices listed above the input box, the prompt's line below it.
        let listed = lines(" ⏺ The dialog offers:\n   - Use this MCP server\n   - Use this and all future MCP servers in this project\n   - Continue without using this MCP server\n ❯ \n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        assert_eq!(toward_trust(&listed), None);
        assert!(shows_prompt(&listed));
        // Its very words typed in the input box.
        let typed = lines("   - Use this MCP server\n   - Continue without using this MCP server\n ❯ Use this MCP server\n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        assert_eq!(toward_trust(&typed), None);
        assert!(shows_prompt(&typed));
        // A past user turn's `❯` above more of the conversation, no hint below it.
        let past = lines("   - Use this MCP server\n   - Continue without using this MCP server\n ❯ which one?\n ⏺ The first.");
        assert_eq!(toward_trust(&past), None);
        // Named among other words, or one choice alone, it is no question.
        let prose = lines(" ⏺ Pick Use this MCP server or Continue without using this MCP server.");
        assert_eq!(toward_trust(&prose), None);
        assert_eq!(toward_trust(&lines(" ❯ Use this MCP server")), None);
        // The two servers' dialog quoted above the input box.
        let quoted = lines("   - Enable selected\n   Space to select · Esc to reject all\n ❯ \n   ⏵⏵ bypass permissions on (shift+tab to cycle)");
        assert_eq!(toward_trust(&quoted), None);
        assert!(shows_prompt(&quoted));
    }

    #[test]
    fn a_prompt_or_a_question_with_one_choice_alone_is_no_question_about_the_folder() {
        assert_eq!(
            toward_trust(&lines(" ❯ Try \"fix lint errors\"\n bypass permissions on")),
            None
        );
        assert_eq!(toward_trust(&lines("❯ Yes, I trust this folder")), None);
        assert_eq!(toward_trust(&[]), None);
    }
}
