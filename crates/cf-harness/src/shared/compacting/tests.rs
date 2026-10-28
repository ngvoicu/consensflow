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
    assert!(!drawn(
        &screen(&["• Context compacted", "› Ask Codex to do anything"]),
        &CODEX
    ));
    assert!(!drawn(
        &screen(&["• Working (4s • esc to interrupt)"]),
        &CODEX
    ));
}

#[test]
fn reads_claude_compacting_with_its_turning_glyph_or_none() {
    for line in [
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
