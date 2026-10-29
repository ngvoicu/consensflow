//! The list: every place that tells one harness from another outside the registry
//! (`cf_harness::launch`), as `tests/dispatch` finds them today.
//!
//! The list only gets shorter. A site that is closed (the code asks the adapter, or the
//! registry, instead) is taken off it, and the test fails until it is; a site that is not
//! on it fails the test, and is not added here: it goes through the contract. Each line is
//! a file, the item in it, how it tells (`scan::Kind`), and how many times.
//!
//! Sorted by file, then item, then kind, which a test holds; one site to a line, which is
//! why rustfmt is left out of it.

use crate::scan::Kind;

#[rustfmt::skip]
pub const SITES: &[(&str, &str, Kind, usize)] = &[
    // The catalog's tables by harness: the efforts each accepts, which of them a model profile is for.
    ("crates/cf-catalog/src/catalog.rs", "efforts", Kind::Match, 1),
    ("crates/cf-catalog/src/profile.rs", "Catalog::model_profile", Kind::IdMatch, 1),
    ("crates/cf-catalog/src/profile.rs", "Catalog::model_profile", Kind::IdEq, 1),
    ("crates/cf-catalog/src/profile.rs", "Catalog::profile", Kind::IdEq, 2),
    ("crates/cf-catalog/src/profile.rs", "harness_of_kind", Kind::IdEq, 1),
    ("crates/cf-catalog/src/roster/add.rs", "Catalog::checked_new", Kind::Eq, 1),
    ("crates/cf-catalog/src/roster/agent_row.rs", "effort_key", Kind::IdEq, 1),

    // The daemon's computer-use seam calls the Claude and Codex implementations directly.
    ("crates/cf-daemon/src/computer_use.rs", "MachineComputerUse::codex", Kind::Adapter, 2),
    ("crates/cf-daemon/src/computer_use.rs", "use cf_harness::claude::computer_use", Kind::Adapter, 1),

    // The engine's seam names the Claude and Codex computer-use types, and a page names a harness by its id.
    ("crates/cf-engine/src/computer_use.rs", "Dispatcher::tell_computer_use_once", Kind::IdEq, 1),
    ("crates/cf-engine/src/handoff/text.rs", "name_of", Kind::IdMatch, 1),
    ("crates/cf-engine/src/seams.rs", "ComputerUse::claude", Kind::Adapter, 1),
    ("crates/cf-engine/src/seams.rs", "ComputerUse::codex", Kind::Adapter, 1),

    // The harnesses' crate, outside its registry: its readers, its admin, its detection and the files a launch leaves.
    ("crates/cf-harness/src/admin/inspect.rs", "inspect", Kind::Match, 1),
    ("crates/cf-harness/src/admin/inspect.rs", "offers", Kind::Eq, 1),
    ("crates/cf-harness/src/admin/inspect.rs", "setup", Kind::Match, 1),
    ("crates/cf-harness/src/admin/source.rs", "feed", Kind::Match, 1),
    ("crates/cf-harness/src/admin/source.rs", "name", Kind::Match, 1),
    ("crates/cf-harness/src/admin/source.rs", "npm_package", Kind::Match, 1),
    ("crates/cf-harness/src/admin/source.rs", "own_installer", Kind::Match, 1),
    ("crates/cf-harness/src/admin/source.rs", "release_source", Kind::Eq, 3),
    ("crates/cf-harness/src/detect.rs", "locations", Kind::Match, 1),
    ("crates/cf-harness/src/detect.rs", "managed", Kind::Eq, 1),
    ("crates/cf-harness/src/records.rs", "has_transcript", Kind::Match, 1),
    ("crates/cf-harness/src/records.rs", "reader", Kind::Match, 1),
    ("crates/cf-harness/src/records.rs", "took", Kind::Match, 1),
    ("crates/cf-harness/src/records.rs", "transcript_boundary", Kind::Match, 1),
    ("crates/cf-harness/src/records.rs", "transcript_left", Kind::Match, 1),
    ("crates/cf-harness/src/records/thread.rs", "Caches::of", Kind::Match, 1),
    ("crates/cf-harness/src/shared/input_box.rs", "read", Kind::Match, 1),
    ("crates/cf-harness/src/shared/input_box.rs", "shown", Kind::Match, 1),
    ("crates/cf-harness/src/shared/launch_files.rs", "harness_folder", Kind::Match, 1),
    ("crates/cf-harness/src/shared/launch_files.rs", "remove_launch", Kind::IdEq, 1),
    ("crates/cf-harness/src/shared/window_args.rs", "model_and_effort", Kind::Match, 1),
    ("crates/cf-harness/src/shared/window_args.rs", "window", Kind::Match, 1),

    // The identity table: a harness's id and kind. It stays while the table is a `match`.
    ("crates/cf-proto/src/agents.rs", "Harness::as_str", Kind::Match, 1),
    ("crates/cf-proto/src/agents.rs", "Harness::kind", Kind::Match, 1),

    // The CLI's hooks: `cf hook claude|devin` picks the adapter's handler, and the doctor reads Claude's.
    ("crates/cf/src/hook.rs", "run", Kind::IdMatch, 1),
    ("crates/cf/src/hook.rs", "run", Kind::IdEq, 1),
    ("crates/cf/src/hook.rs", "use cf_harness::claude", Kind::Adapter, 1),
    ("crates/cf/src/hook.rs", "use cf_harness::devin", Kind::Adapter, 1),
    ("crates/cf/src/standalone/doctor.rs", "use cf_harness::claude::stale_hooks", Kind::Adapter, 1),
];
