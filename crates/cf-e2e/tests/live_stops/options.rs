//! What the command line of the stops asks: which cases, which hook the `slowhook`
//! case runs, which harnesses the `stuck`, `midturn` and `midtool` cases run on, where
//! the Claude transcripts are kept; and what a case is given to open.

use std::path::PathBuf;

use cf_e2e::live::members::{member, named, stand_in_chief, Plan};
use cf_e2e::live::Harness;
use cf_e2e::Result;

use crate::{midturn, slow, stuck};

/// A case of the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Command,
    Early,
    Hook,
    Escape,
    SlowHook,
    Stuck,
    MidTurn,
    MidTool,
}

impl Case {
    /// Every case, in the order they are run in.
    pub const ALL: [Case; 8] = [
        Case::Command,
        Case::Early,
        Case::Hook,
        Case::Escape,
        Case::SlowHook,
        Case::Stuck,
        Case::MidTurn,
        Case::MidTool,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Case::Command => "command",
            Case::Early => "early",
            Case::Hook => "hook",
            Case::Escape => "escape",
            Case::SlowHook => "slowhook",
            Case::Stuck => "stuck",
            Case::MidTurn => "midturn",
            Case::MidTool => "midtool",
        }
    }

    /// Whether the case runs on any harness, the ones the command line names.
    pub const fn on_any_harness(self) -> bool {
        matches!(self, Case::Stuck | Case::MidTurn | Case::MidTool)
    }
}

/// What the command line asked.
pub struct Options {
    pub cases: Vec<Case>,
    pub hook: slow::Variant,
    /// The harnesses the cases that run on any harness run on.
    pub harnesses: Vec<Harness>,
    /// Where each Claude transcript is copied (`--transcripts`).
    pub transcripts: Option<PathBuf>,
}

/// The cases and the harnesses the words ask for, or all the cases and OpenCode.
pub fn options(
    cases: &[String],
    hook: Option<&str>,
    harnesses: &[String],
    transcripts: Option<String>,
) -> Result<Options> {
    Ok(Options {
        cases: named(cases, &Case::ALL, Case::name, "case", &Case::ALL)?,
        hook: slow::variant(hook.unwrap_or("stop"))?,
        harnesses: named(
            harnesses,
            &Harness::ALL,
            Harness::name,
            "harness",
            &[Harness::OpenCode],
        )?,
        transcripts: transcripts.map(PathBuf::from),
    })
}

/// The harnesses a case runs on: Claude's, but for the ones that run on any, which
/// run on each of `named`.
pub fn runs_on(case: Case, named: &[Harness]) -> Vec<Harness> {
    if case.on_any_harness() {
        named.to_vec()
    } else {
        vec![Harness::Claude]
    }
}

/// What a case is given to open: the member it runs, and the files of its folder.
pub fn plan_of(case: Case, harness: Harness, options: &Options) -> Plan {
    let name = case.name();
    match case {
        Case::SlowHook => slow::plan_of(&options.hook, member(harness), stand_in_chief()),
        Case::Stuck => Plan::new(&format!("stops-stuck-{}", harness.name()), stand_in_chief())
            .with(member(harness))
            .with_file(stuck::SCRIPT, stuck::script_text()),
        // Their folders tell a window to do what each message asks, and run no tools but
        // the one a message tells it to.
        Case::MidTurn | Case::MidTool => Plan::new(
            &format!("stops-{name}-{}", harness.name()),
            stand_in_chief(),
        )
        .with(member(harness))
        .told(midturn::FOLDER_BRIEF),
        _ => Plan::new(&format!("stops-{name}"), stand_in_chief()).with(member(harness)),
    }
}

#[cfg(test)]
mod tests {
    use cf_e2e::live;

    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn every_case_is_run_unless_some_are_named_and_the_any_harness_ones_run_on_opencode_unless_a_harness_is(
    ) {
        let all = options(&[], None, &[], None).unwrap();
        let names: Vec<&str> = all.cases.iter().map(|case| case.name()).collect();
        assert_eq!(
            names,
            ["command", "early", "hook", "escape", "slowhook", "stuck", "midturn", "midtool"]
        );
        assert_eq!(all.harnesses, [Harness::OpenCode]);
        assert_eq!(all.hook.name, "stop");
        assert!(all.transcripts.is_none());

        let some = options(
            &words(&["hook", "command"]),
            Some("prompt"),
            &words(&["codex", "pi"]),
            Some("/tmp/kept".to_owned()),
        )
        .unwrap();
        assert_eq!(some.cases, [Case::Hook, Case::Command]);
        assert_eq!(some.hook.name, "prompt");
        assert_eq!(some.harnesses, [Harness::Codex, Harness::Pi]);
        assert_eq!(some.transcripts, Some(PathBuf::from("/tmp/kept")));
    }

    #[test]
    fn a_case_or_a_hook_or_a_harness_that_is_none_is_refused_with_its_name() {
        let message = |failed: Result<Options>| failed.err().map(|failed| failed.to_string());
        assert_eq!(
            message(options(&words(&["nobody"]), None, &[], None)).as_deref(),
            Some("no such case: nobody")
        );
        assert_eq!(
            message(options(&[], Some("late"), &[], None)).as_deref(),
            Some("no such hook: late")
        );
        assert_eq!(
            message(options(&[], None, &words(&["kimi"]), None)).as_deref(),
            Some("no such harness: kimi")
        );
    }

    #[test]
    fn only_the_stuck_midturn_and_midtool_cases_run_on_the_harnesses_named_and_every_other_on_claude(
    ) {
        let named = [Harness::Devin, Harness::Codex];
        for case in Case::ALL {
            let expected: &[Harness] =
                if [Case::Stuck, Case::MidTurn, Case::MidTool].contains(&case) {
                    &named
                } else {
                    &[Harness::Claude]
                };
            assert_eq!(runs_on(case, &named), expected, "{}", case.name());
        }
    }

    #[test]
    fn each_case_has_a_folder_of_its_own_and_the_ones_that_need_files_have_them() {
        let options = options(&[], Some("pretool"), &[], None).unwrap();
        let folder = |case: Case, harness: Harness| plan_of(case, harness, &options).folder;
        assert_eq!(folder(Case::Command, Harness::Claude), "stops-command");
        assert_eq!(folder(Case::Hook, Harness::Claude), "stops-hook");
        assert_eq!(
            folder(Case::SlowHook, Harness::Claude),
            "stops-ignored-pretool"
        );
        assert_eq!(folder(Case::Stuck, Harness::Codex), "stops-stuck-codex");
        assert_eq!(folder(Case::MidTurn, Harness::Devin), "stops-midturn-devin");
        // Its folder does not tell a window to reply with one line: its reply is five thousand.
        let midturn = plan_of(Case::MidTurn, Harness::Devin, &options);
        assert_eq!(midturn.brief, midturn::FOLDER_BRIEF);
        assert!(midturn.files.is_empty());
        assert_eq!(midturn.roster[1]["kind"], "devin");
        // The case that runs a command has its own folder, told the same.
        assert_eq!(folder(Case::MidTool, Harness::Devin), "stops-midtool-devin");
        let midtool = plan_of(Case::MidTool, Harness::Codex, &options);
        assert_eq!(midtool.brief, midturn::FOLDER_BRIEF);
        assert!(midtool.files.is_empty());
        assert_eq!(midtool.roster[1]["kind"], "codex");
        let stuck = plan_of(Case::Stuck, Harness::Codex, &options);
        assert_eq!(stuck.files[0].0, "ignore-signals-cf-stops-live.sh");
        assert!(stuck.files[0].1.contains("trap '' INT TERM HUP"));
        assert!(plan_of(Case::Command, Harness::Claude, &options)
            .files
            .is_empty());
        // The member of the project is the harness's, on its cheap model.
        assert_eq!(stuck.roster[1]["kind"], "codex");
        assert_eq!(
            stuck.roster[1]["model"],
            live::agents::cheap(Harness::Codex).0
        );
    }
}
