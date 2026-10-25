//! Live: what clears a paste the app left unsent in a window's input box. It is
//! opt-in (`cargo xtask live unsent-box`): it spends the machine's quota on a
//! window of each harness for each strategy, and needs the harness installed and
//! logged in, so it is ignored unless asked for and is no part of `check` or of
//! CI.
//!
//! The app pastes into Claude Code and Devin and presses Enter once more for a
//! paste their record does not show. That Enter is withheld where what the paste
//! held was withdrawn, or its project closed, while the screen was looked at: the
//! paste stays in the box, and the next paste would go in beside it, to be sent
//! with it (both reviews of 2026-10-20, round 10). The app clears the box first,
//! with the keys of its harness (`cf_harness::clearing_keys`). Each strategy here
//! is a window of its own: a first message pasted and never sent, the keys of the
//! strategy, then a second message pasted as the app pastes. What the harness's
//! record holds of the second says whether the first went with it. When it did
//! not, the same keys are pressed at the empty box of a window at rest, before a
//! third message, which must arrive alone and be answered: a key that harms an
//! empty prompt (ends the window, or opens a dialog that takes the next Enter) is
//! no way to clear one.
//!
//! ```text
//! cargo xtask live unsent-box                        Devin, every strategy
//! cargo xtask live unsent-box claude devin           both
//! cargo xtask live unsent-box --strategy ctrl-c --require ctrl-c
//! ```
//!
//! With no `--require`, a run fails unless the app's own key ([`keys::daemons`],
//! as the harness code says it) cleared the box of each harness tried; another
//! strategy that does not is a finding, not a failure.
//!
//! Found with Devin 3000.11.92 and Claude Code 2.1.305 (2026-10-21): with nothing
//! pressed, each sends the first message with the second, as one; Control-C
//! empties the whole box of each and does no harm to an empty one at rest. On
//! Devin, Control-U empties the cursor's line only (the first message's header
//! went with the second) and one Escape leaves the box as it is. Devin 3000.11.92
//! on Windows does the same with each key (2026-10-21).

#[path = "live_unsent_box/flow.rs"]
mod flow;
#[path = "live_unsent_box/keys.rs"]
mod keys;

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{pasted_installs, Open};
use cf_e2e::live::{self, ask, Log};
use cf_e2e::rig::Rig;
use cf_e2e::Result;

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows:
/// no window of this test runs it, but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of each harness for each strategy; cargo xtask live unsent-box"]
fn the_keys_the_app_clears_a_box_with_take_a_paste_left_unsent_and_are_harmless_at_an_empty_one(
) -> Result {
    let log = Log::begin("live unsent-box")?;
    ask::prepare()?;
    let installs = pasted_installs(&log)?;
    let tried = keys::chosen(&live::list("strategy"))?;
    let named = live::list("require");
    let rig = Rig::start(live::rig(&installs, FAKE_AGENT))?;
    let mut checks = Vec::new();
    for install in &installs {
        let harness = install.harness();
        let who = harness.name();
        let own = keys::daemons(harness)?;
        if own.is_none() && named.is_empty() {
            checks.push(Check::failed(
                who,
                "clear",
                "the app presses none of these keys to clear its box",
            ));
        }
        for strategy in &tried {
            log.say(format!("{who}: strategy {}", strategy.name));
            let pane = format!("unsent-box-{who}-{}", strategy.name);
            let window = Open::new(&rig, install, "unsent-box", &pane).open_telling(&log, who)?;
            let outcome = flow::flow(&window, strategy)?;
            drop(window);
            log.block(
                &format!("{who}: {}: {}", strategy.name, outcome.said()),
                &outcome.lines.join("\n"),
            );
            let required = keys::required(own, strategy, &named);
            checks.push(flow::check_of(who, strategy, &outcome, required));
        }
    }
    rig.close()?;
    report::conclude(&log, "what each strategy came to", &checks)
}
