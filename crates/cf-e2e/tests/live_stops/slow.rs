//! `slowhook`: Escape while a hook of the project's own, that ignores the signals
//! Claude ends a hook with, outlives Claude's turn: the stop the daemon could take
//! for ignored. What keeps a real Claude window at work through its presses is
//! such a hook (`.claude/settings.local.json`, which Claude reads beside the
//! settings the daemon launches it with) that ignores `INT`, `TERM` and `HUP` and
//! runs for [`IGNORE_S`] seconds. Where the hook runs decides what Claude is doing
//! meanwhile, and `--hook` picks it. When Claude stopped, the resume of the task,
//! and its result (the hook runs again in the turn after it).

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::claude::{processes_with, runs};
use cf_e2e::live::members::{find, plural, plus, plus_maybe, Account, Members, Plan};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

use crate::resume::{resume_after, Expect};
use crate::stop::{idle_after, late, pause_text};
use crate::watch::{
    interrupted, say_events, settlement_line, told_the_human, told_the_requester, unstopped, watch,
    whole,
};
use crate::{now_ms, Ctx, WORK};

/// How many seconds the hook ignores the signals for.
pub const IGNORE_S: u64 = 100;

/// What the hook's command line holds that nothing else of the machine does.
pub const HOOK_MARK: &str = "cf-stops-live-ignoring-hook";

/// The hook: it ignores the signals Claude ends a hook with and runs for a while,
/// and ends with a command that names the mark.
fn slow_hook() -> String {
    format!("trap '' INT TERM HUP; sleep {IGNORE_S}; : {HOOK_MARK}")
}

/// Where the hook runs, and what the member is asked to do to get there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub name: &'static str,
    /// The event of Claude's the hook is on.
    pub event: &'static str,
    /// What the hook is limited to, for a tool event.
    pub matcher: Option<&'static str>,
    pub brief: &'static str,
    /// Whether the case goes on to resume the task once Claude stopped. Claude
    /// writes a prompt's record after its `UserPromptSubmit` hooks: a hook of a
    /// hundred seconds keeps the resume's record out for longer than the daemon waits
    /// for it, which sends it again. The stop is what that variant shows.
    pub resumes: bool,
}

/// The variants: `stop` (the hook of Claude's end of turn), `pretool` (before its
/// tool) and `prompt` (before its first word).
const VARIANTS: [Variant; 3] = [
    Variant {
        name: "stop",
        event: "Stop",
        matcher: None,
        brief: "Reply with exactly one line: DONE",
        resumes: true,
    },
    Variant {
        name: "pretool",
        event: "PreToolUse",
        matcher: Some("Bash"),
        brief: "Run exactly this one shell command, then stop: echo hello",
        resumes: true,
    },
    Variant {
        name: "prompt",
        event: "UserPromptSubmit",
        matcher: None,
        brief: "Reply with exactly one line: DONE",
        resumes: false,
    },
];

/// The variant `--hook` names.
pub fn variant(name: &str) -> Result<Variant> {
    VARIANTS
        .iter()
        .find(|variant| variant.name == name)
        .cloned()
        .ok_or_else(|| Error::Daemon(format!("no such hook: {name}")))
}

/// The settings of the project's own that put the slow hook on the variant's event.
pub fn settings_of(variant: &Variant) -> Value {
    let mut entry = serde_json::Map::new();
    if let Some(matcher) = variant.matcher {
        entry.insert("matcher".to_owned(), json!(matcher));
    }
    entry.insert(
        "hooks".to_owned(),
        json!([{ "type": "command", "command": slow_hook(), "timeout": IGNORE_S + 120 }]),
    );
    json!({ "hooks": { variant.event: [Value::Object(entry)] } })
}

/// The plan of the project the case runs in: Claude's member, and the settings.
pub fn plan_of(variant: &Variant, member: Value, stand_in_chief: Value) -> Plan {
    Plan::new(&format!("stops-ignored-{}", variant.name), stand_in_chief)
        .with(member)
        .with_file(
            ".claude/settings.local.json",
            settings_of(variant).to_string(),
        )
}

/// How the lane of the window read over the stop, which the board says.
struct Lane {
    /// When the board first said the window did not stop, and what it said.
    flagged: Option<(i64, Value)>,
    /// When it stopped saying so.
    cleared: Option<i64>,
    /// When the board first showed the window gone: the daemon closes one that
    /// ignored every round.
    closed: Option<i64>,
}

/// How long the lane is looked at once the window is gone, for the notes that
/// say why to land: the host answers the close once it has ended the window.
const AFTER_THE_CLOSE_MS: i64 = 10_000;

/// The board's own word on it: `unstopped` on the lane, once every round was
/// ignored and the host would not close the window; looked at until it is said
/// and unsaid and the window is idle, for a little while once the window is
/// gone, or, for a window that stopped and was never said to have ignored the
/// stop, until it has nothing more to show.
fn watch_the_lane(
    members: &Members,
    agent: &str,
    status: &cf_e2e::live::claude::Status,
    made: i64,
) -> Result<Lane> {
    let mut lane = Lane {
        flagged: None,
        cleared: None,
        closed: None,
    };
    let end = made + i64::try_from((IGNORE_S + 40) * 1_000).unwrap_or(i64::MAX);
    while now_ms() < end {
        let read = members.lane(agent)?;
        let flag = unstopped(&read);
        if let (Some(flag), None) = (&flag, &lane.flagged) {
            lane.flagged = Some((now_ms(), flag.clone()));
        }
        if flag.is_none() && lane.flagged.is_some() && lane.cleared.is_none() {
            lane.cleared = Some(now_ms());
        }
        if read["pane"].is_null() && lane.closed.is_none() {
            lane.closed = Some(now_ms());
        }
        if lane
            .closed
            .is_some_and(|closed| now_ms() - closed > AFTER_THE_CLOSE_MS)
        {
            break;
        }
        let idle = status
            .samples()
            .into_iter()
            .rev()
            .find(|sample| sample.at_ms > made)
            .is_some_and(|sample| sample.status.as_deref() == Some("idle"));
        let flagged = lane.flagged.is_some();
        if nothing_more_to_show(flagged, lane.cleared.is_some(), idle, now_ms() - made) {
            break;
        }
        sleep(Duration::from_millis(500));
    }
    Ok(lane)
}

/// Whether the board has nothing more to show of a stop `since` milliseconds after
/// the pause: it said the window did not stop, then unsaid it, and the window is
/// idle; or it never said so, and the window has read idle for a good while: a window
/// that stopped and was never said to have ignored the stop has nothing more to show.
pub fn nothing_more_to_show(flagged: bool, cleared: bool, idle: bool, since: i64) -> bool {
    (cleared && idle) || (!flagged && idle && since > 18_000)
}

/// Whether Claude stopped at the first Escape: its status read idle within five seconds
/// of it, whatever the hook it left behind does. Where the hook keeps the first message
/// from being written (`UserPromptSubmit`), the first Escape comes when the message is, a
/// hundred seconds after the pause: the moment is the Escape's.
pub fn stopped_at_the_first_escape(idle_at: Option<i64>, presses: &[i64]) -> bool {
    idle_at
        .zip(presses.first())
        .is_some_and(|(idle, first)| idle - first < 5_000)
}

/// The gaps between presses, each in seconds with a decimal.
pub fn gaps(presses: &[i64]) -> Vec<String> {
    presses
        .windows(2)
        .map(|pair| cf_e2e::live::seconds(pair[1] - pair[0]))
        .collect()
}

/// What was told, as a line: how many notes to the human and to the requester,
/// and the words of the first of each.
pub fn told_line(human: &[&Value], requester: &[&Value]) -> String {
    format!(
        "{} to the human{} and {} to the requester{}",
        plural(human.len(), "note"),
        whole(human.first().copied()),
        requester.len(),
        whole(requester.first().copied())
    )
}

/// The case: the member is inside a hook that ignores the signals, and is paused.
pub fn run(ctx: &Ctx, case: &mut Account, variant: &Variant) -> Result<()> {
    let number = ctx.members.give(ctx.agent, variant.brief)?;
    let running = find(
        ctx.log,
        &format!("the {} hook that ignores signals to run", variant.event),
        WORK,
        Duration::from_millis(250),
        || Ok((!processes_with(HOOK_MARK)?.is_empty()).then_some(())),
    )?;
    if running.is_none() {
        case.fail(format!(
            "the {} hook that ignores signals never ran in {} s",
            variant.event,
            WORK.as_secs()
        ));
        return Ok(());
    }
    let watched = watch(ctx.members, ctx.agent)?;
    sleep(Duration::from_secs(1));
    let paused = ctx.members.pause(number)?;
    let made = paused.made_ms;
    let pids = processes_with(HOOK_MARK)?
        .iter()
        .map(|process| process.pid.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    case.say(format!(
        "T-{number}: the member is inside a {} hook that ignores INT, TERM and HUP for {IGNORE_S} s (process {pids}); {}",
        variant.event,
        pause_text(number, &paused.answer)
    ));

    let lane = watch_the_lane(ctx.members, ctx.agent, &watched.status, made)?;
    sleep(ctx.rest / 2);
    let presses = watched.escapes(ctx.members);
    let hook_alive = !processes_with(HOOK_MARK)?.is_empty();
    let human_inbox = ctx.members.inbox(None)?;
    let requester_inbox = ctx.members.inbox(Some("chief"))?;
    let (human, requester) = (
        told_the_human(&human_inbox),
        told_the_requester(&requester_inbox),
    );
    let samples = watched.status.samples();
    let idle_at = idle_after(&samples, made);
    let items = watched.record(ctx.members);
    let interrupt = interrupted(&items, made);
    let gaps = gaps(&presses);
    let stopped_at_once = stopped_at_the_first_escape(idle_at, &presses);
    let late_keys = late(&presses, idle_at);
    let pane_open = !ctx.members.lane(ctx.agent)?["pane"].is_null();
    let told = told_line(&human, &requester);
    let how = format!(
        "{} Escapes ({}; gaps {} s); unstopped {}; its pane {}; the hook {}",
        presses.len(),
        presses
            .iter()
            .map(|press| plus(*press, made))
            .collect::<Vec<_>>()
            .join(", "),
        gaps.join(", "),
        lane.flagged.as_ref().map_or_else(
            || "never on its lane".to_owned(),
            |(at, flag)| format!(
                "{flag} on its lane at {}{}",
                plus(*at, made),
                lane.cleared.map_or_else(
                    || ", still there when it was last looked at".to_owned(),
                    |cleared| format!(", gone at {}", plus(cleared, made))
                )
            )
        ),
        if pane_open {
            "is still open"
        } else {
            "is gone"
        },
        if hook_alive { "still runs" } else { "ended" },
    );
    if stopped_at_once {
        // Claude did stop: what the design promises of a window that stopped.
        case.check(
            "Claude stopped at the first Escape",
            true,
            format!(
                "status read idle {}; its record {}; the {} hook it was in is still running",
                plus_maybe(idle_at, made),
                interrupt.map_or_else(
                    || "holds no interrupt".to_owned(),
                    |interrupt| format!(
                        "holds \"{}\" at {}",
                        interrupt.text.chars().take(60).collect::<String>(),
                        plus_maybe(interrupt.at_ms, made)
                    )
                ),
                variant.event
            ),
        );
        case.check(
            "nothing is pressed once it has stopped",
            late_keys.is_empty(),
            format!(
                "{} Escapes more than a look after the status read idle",
                late_keys.len()
            ),
        );
        case.check(
            "it is not said to have ignored the stop",
            lane.flagged.is_none() && human.is_empty() && requester.is_empty(),
            &told,
        );
    } else {
        // The window kept working through every round: the daemon closes it, and says why.
        case.check(
            "the window kept working through its presses",
            presses.len() >= 3,
            format!(
                "the status first read idle {}",
                idle_at.map_or_else(|| "never".to_owned(), |idle| plus(idle, made))
            ),
        );
        case.check(
            "the daemon closed the window",
            !pane_open,
            lane.closed.map_or_else(
                || format!("its pane is still open; {how}"),
                |closed| format!("its pane was gone at {}", plus(closed, made)),
            ),
        );
        case.check("the human is told, once", human.len() == 1, &told);
        case.check("the requester is told, once", requester.len() == 1, &told);
    }
    case.say(format!("     the stop, as it ran: {how}"));
    case.say(format!(
        "     {}",
        settlement_line(ctx.members, &watched.session)
    ));
    let kept: Vec<_> = samples
        .iter()
        .filter(|sample| sample.at_ms >= made - 2_000)
        .cloned()
        .collect();
    case.say(format!(
        "     status over the stop: {}",
        runs(&kept, made).join("; ")
    ));
    // A window that stopped is paid at rest: the task goes on, and ends with its result.
    if stopped_at_once && variant.resumes && case.held() {
        resume_after(
            ctx.members,
            case,
            ctx.log,
            (ctx.agent, number),
            &watched,
            Expect::default(),
        )?;
    }
    say_events(case, ctx.members, number);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hook_that_ignores_the_signals_claude_ends_a_hook_with_names_a_mark_only_it_has() {
        assert_eq!(
            slow_hook(),
            "trap '' INT TERM HUP; sleep 100; : cf-stops-live-ignoring-hook"
        );
    }

    #[test]
    fn the_variants_are_the_hooks_on_the_end_of_a_turn_before_a_tool_and_before_the_first_word() {
        let names: Vec<&str> = VARIANTS.iter().map(|variant| variant.name).collect();
        assert_eq!(names, ["stop", "pretool", "prompt"]);
        assert_eq!(variant("prompt").unwrap().event, "UserPromptSubmit");
        assert_eq!(variant("pretool").unwrap().matcher, Some("Bash"));
        assert_eq!(variant("stop").unwrap().matcher, None);
        assert_eq!(
            variant("nobody").unwrap_err().to_string(),
            "no such hook: nobody"
        );
    }

    #[test]
    fn only_the_hook_before_the_first_word_does_not_go_on_to_resume_the_task() {
        // The record of a prompt is written after its hooks: a hook of a hundred seconds keeps the
        // resume's out for longer than the daemon waits, which sends it again.
        let resumes: Vec<(&str, bool)> = VARIANTS.iter().map(|v| (v.name, v.resumes)).collect();
        assert_eq!(
            resumes,
            [("stop", true), ("pretool", true), ("prompt", false)]
        );
    }

    #[test]
    fn the_settings_of_the_project_put_the_slow_hook_on_the_event_with_a_timeout_that_outlasts_it()
    {
        assert_eq!(
            settings_of(&variant("pretool").unwrap()),
            json!({ "hooks": { "PreToolUse": [{
                "matcher": "Bash",
                "hooks": [{ "type": "command", "command": slow_hook(), "timeout": 220 }],
            }] } })
        );
        let stop = settings_of(&variant("stop").unwrap());
        assert!(stop["hooks"]["Stop"][0].get("matcher").is_none(), "{stop}");
        assert_eq!(
            stop["hooks"]["Stop"][0]["hooks"][0]["timeout"],
            IGNORE_S + 120
        );
    }

    #[test]
    fn the_project_of_the_case_has_the_settings_where_claude_reads_them_beside_its_own() {
        let plan = plan_of(
            &variant("prompt").unwrap(),
            cf_e2e::live::members::member(cf_e2e::live::Harness::Claude),
            cf_e2e::live::members::stand_in_chief(),
        );
        assert_eq!(plan.folder, "stops-ignored-prompt");
        assert_eq!(plan.files[0].0, ".claude/settings.local.json");
        assert!(plan.files[0].1.contains("UserPromptSubmit"));
    }

    #[test]
    fn claude_stopped_at_the_first_escape_when_its_status_read_idle_within_five_seconds_of_it() {
        assert!(stopped_at_the_first_escape(Some(11_000), &[10_000, 13_000]));
        // Idle at the very press, or before it (a window that had stopped): it stopped.
        assert!(stopped_at_the_first_escape(Some(9_500), &[10_000]));
        assert!(!stopped_at_the_first_escape(Some(15_000), &[10_000]));
        // The moment is the Escape's: where the hook keeps the first message from being
        // written (UserPromptSubmit), the first Escape comes a hundred seconds after the
        // pause, and Claude stops at that one.
        assert!(stopped_at_the_first_escape(
            Some(110_500),
            &[110_000, 113_000, 116_000]
        ));
        // A window that never read idle, or was never pressed, did not stop at a press.
        assert!(!stopped_at_the_first_escape(None, &[10_000]));
        assert!(!stopped_at_the_first_escape(Some(11_000), &[]));
    }

    #[test]
    fn the_board_has_nothing_more_to_show_once_it_unsaid_what_it_said_or_never_said_it() {
        // It said the window did not stop, then unsaid it, and the window is idle.
        assert!(nothing_more_to_show(true, true, true, 60_000));
        // It said it and has not unsaid it, or the window is still at work: more to show.
        assert!(!nothing_more_to_show(true, false, true, 60_000));
        assert!(!nothing_more_to_show(true, true, false, 60_000));
        // It never said it, and the window has read idle for more than eighteen seconds.
        assert!(nothing_more_to_show(false, false, true, 18_001));
        assert!(!nothing_more_to_show(false, false, true, 18_000));
        assert!(!nothing_more_to_show(false, false, false, 99_000));
    }

    #[test]
    fn the_gaps_between_presses_are_in_seconds_and_a_note_is_told_with_its_words() {
        assert_eq!(gaps(&[1_000, 4_100, 7_200]), ["3.1", "3.1"]);
        assert!(gaps(&[1_000]).is_empty());
        let note = json!({ "body": "T-3 did not stop for T-3" });
        let (human, requester) = ([&note], [&note, &note]);
        assert_eq!(
            told_line(&human, &requester),
            "1 note to the human (\"T-3 did not stop for T-3\") and 2 to the requester (\"T-3 did not stop for T-3\")"
        );
        assert_eq!(
            told_line(&[], &[]),
            "0 notes to the human and 0 to the requester"
        );
    }
}
