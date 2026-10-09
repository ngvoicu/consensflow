//! `the standalone switch-over (TEST-PANE-47)`: what the native `cf` has not
//! grown back from before it stood alone.

use cf_e2e::{checkout, files};
use regex::Regex;

use crate::Outcome;

#[test]
fn removes_direct_conversation_writes_and_terminal_window_discovery_from_cf() -> Outcome {
    // The native cf's own sources, whose words a look is taken at.
    let sources_folder = checkout::path("crates/cf/src");
    let sources: Vec<_> = checkout::files_below(&sources_folder, &[])?
        .into_iter()
        .filter(|file| file.to_string_lossy().ends_with(".rs"))
        .collect();
    assert!(
        !sources.is_empty(),
        "no source of the native cf in {}",
        sources_folder.display()
    );
    let forbidden = Regex::new(r"\bsaveThread\b|liveWindowElsewhere|CMUX_SURFACE_ID|cmux tree")?;
    for file in sources {
        assert!(
            !forbidden.is_match(&files::read_string(&file)?),
            "{}",
            checkout::relative(&file)
        );
    }
    Ok(())
}
