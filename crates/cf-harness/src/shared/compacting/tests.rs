use super::{drawn, CLAUDE, CODEX, PI};

fn screen(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|&line| line.to_owned()).collect()
}

#[test]
fn reads_codex_compacting_as_the_field_s_screen_drew_it() {
    let field = screen(&[
        "• Compacting context (2m 57s • esc to interrupt)",
        "└ Making room to continue.",
        "└ Tip: Press ⌃g to edit your current draft in an external editor.",
        "› Ask Codex to do anything",
        "GPT-6.1-Sol xhigh · ~/Projects/ngvoicu/consensflow · Test candidate pass T-852",
    ]);
    assert!(drawn(&field, &CODEX));
    // Its bullet blinks to a hollow one between frames.
    assert!(drawn(
        &screen(&["◦ Compacting context (3s • esc to interrupt)"]),
        &CODEX
    ));
    assert!(!drawn(
        &screen(&["• Context compacted", "› Ask Codex to do anything"]),
        &CODEX
    ));
    assert!(!drawn(
        &screen(&["• Working (4s • esc to interrupt)"]),
        &CODEX
    ));
}

/// A real Claude Code 2.1.312 chief's last lines a moment after the human sent `/compact`,
/// as the pane host drew them (`cargo xtask live compaction-screens claude`, 2026-10-29):
/// the status line, and the conversation above it that quotes nothing of it.
#[test]
fn reads_claude_compacting_on_a_screen_it_drew() {
    let live = screen(&[
        "  Get to finished work sooner with Opus 5.5. Switch anytime with /model.",
        "  1 more notice hidden",
        "❯ Reply with exactly one line: READY",
        "⏺ READY",
        "✻ Cogitated for 1s · done 2:06 AM",
        "❯ /compact",
        "✢ Compacting conversation… (0s)",
        "  ⎿ \u{a0}Tip: Working on a plan or design doc? Ask Claude to publish it as an",
        "     artifact — a polished web page you can open in your browser.",
        "  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt",
    ]);
    assert!(drawn(&live, &CLAUDE));
    // The same screen once it is done compacting: the status line gone.
    let done: Vec<String> = live
        .iter()
        .filter(|line| !line.contains("Compacting"))
        .cloned()
        .collect();
    assert!(!drawn(&done, &CLAUDE));
}

#[test]
fn reads_claude_compacting_with_its_turning_glyph_or_none() {
    for line in [
        // Off macOS Claude Code turns a `*` where macOS has `✳` (2.1.312's `other` frames).
        "* Compacting conversation… (12s)",
        "✻ Compacting conversation… (12s)",
        "· Compacting conversation… (3m 2s · ↓ 1.2k tokens)",
        "  Compacting conversation…",
        "✢ Running PreCompact hooks…",
    ] {
        assert!(drawn(&screen(&[line]), &CLAUDE), "{line}");
    }
    assert!(!drawn(
        &screen(&["⎿  Conversation compacted · ctrl+o for history"]),
        &CLAUDE
    ));
    assert!(!drawn(
        &screen(&["✻ Thinking… (esc to interrupt)"]),
        &CLAUDE
    ));
    // A conversation that quotes the words in its middle is not compacting.
    assert!(!drawn(
        &screen(&["⏺ Claude shows \"Compacting conversation…\" while it compacts"]),
        &CLAUDE
    ));
}

/// A real Pi 1.4.0 chief's last lines a moment after the human sent `/compact`, as the pane
/// host drew them (`cargo xtask live compaction-screens pi`, 2026-10-29): Pi draws its
/// status inside the rule above its input box, the spinner after the rule's start.
#[test]
fn reads_pi_compacting_on_a_screen_it_drew() {
    let live = screen(&[
        "  consensflow-delivery.mjs",
        " Reply with exactly one line: READY",
        " READY",
        "── ⠧ Compacting context... (escape to cancel) ──────────────────────────────────",
        "~/.consensflow-candidate/live/compaction-screens-pi",
        "↑6.2k ↓5 $0.001 (sub) 2.3%/272k (auto)      (openai-codex) gpt-5.6-luna • medium",
    ]);
    assert!(drawn(&live, &PI));
    // The rule with no status in it, and the failure Pi says when it has nothing to compact.
    let idle = screen(&[
        " READY",
        " Error: Compaction failed: Nothing to compact (session too small)",
        "────────────────────────────────────────────────────────────────────────────────",
        "↑6.2k ↓5 $0.001 (sub) 2.3%/272k (auto)      (openai-codex) gpt-5.6-luna • medium",
    ]);
    assert!(!drawn(&idle, &PI));
}

#[test]
fn reads_each_of_pi_s_compactions() {
    for line in [
        "⠋ Compacting context... (escape to cancel)",
        "⠙ Auto-compacting... (escape to cancel)",
        "⠹ Context overflow detected, Auto-compacting... (escape to cancel)",
    ] {
        assert!(drawn(&screen(&[line]), &PI), "{line}");
    }
    assert!(!drawn(
        &screen(&["[compaction] Compacted from 182340 tokens"]),
        &PI
    ));
    assert!(!drawn(&screen(&["Compaction cancelled"]), &PI));
}

/// A line that quotes a status (a `cat` or `git diff` of the readers' own source, a
/// Markdown bullet or quote) begins with something no harness draws before its status:
/// it is no compaction (Calliope's P2-1 of T-917).
#[test]
fn reads_no_quote_of_a_status_line_as_compaction() {
    for (line, statuses) in [
        (
            r#"        "• Compacting context (2m 57s • esc to interrupt)","#,
            &CODEX,
        ),
        ("+    \"Compacting context (\",", &CODEX),
        ("-• Compacting context (12s • esc to interrupt)", &CODEX),
        ("- Compacting context (12s • esc to interrupt)", &CODEX),
        ("> • Compacting context (12s • esc to interrupt)", &CODEX),
        ("+    \"Compacting conversation…\",", &CLAUDE),
        ("- Compacting conversation… (12s)", &CLAUDE),
        ("> ✻ Compacting conversation… (12s)", &CLAUDE),
        (r#""Compacting conversation…""#, &CLAUDE),
        ("- Auto-compacting... (escape to cancel)", &PI),
        ("    \"Auto-compacting... (escape to cancel)\",", &PI),
        ("⏺ • Compacting context (2m 57s • esc to interrupt)", &CODEX),
    ] {
        assert!(!drawn(&screen(&[line]), statuses), "{line}");
    }
}

/// Pi says the key that cancels as its keymap has it (`keyText("app.interrupt")`), which
/// the human may have bound to another (Calliope's P3-1 of T-917).
#[test]
fn reads_pi_s_compaction_whatever_key_cancels_it() {
    for line in [
        "⠋ Compacting context... (ctrl+c to cancel)",
        "⠙ Auto-compacting... (esc to cancel)",
        "⠹ Context overflow detected, Auto-compacting... (ctrl+g to cancel)",
    ] {
        assert!(drawn(&screen(&[line]), &PI), "{line}");
    }
}
