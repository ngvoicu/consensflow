//! What a download meets: every seal holds, and Gatekeeper takes both files as
//! notarized.

use std::path::Path;

use super::name;
use super::tools::{args, fail, Tools};
use crate::cli::Failure;

/// What `spctl` says of a file it takes as notarized, Developer ID signed code.
const NOTARIZED: &str = "source=Notarized Developer ID";

/// Holds `app` and `dmg` to their seals, and, once `notarized`, to Gatekeeper's
/// assessment of each as a download is assessed.
pub(super) fn verify(
    tools: &Tools,
    app: &Path,
    dmg: &Path,
    notarized: bool,
) -> Result<(), Failure> {
    tools.run("codesign", args!["--verify", "--deep", "--strict", app])?;
    tools.run("codesign", args!["--verify", "--strict", dmg])?;
    if !notarized {
        return Ok(());
    }
    let kinds = [
        (app, args!["--type", "execute"]),
        (
            dmg,
            args!["--type", "open", "--context", "context:primary-signature"],
        ),
    ];
    for (path, kind) in kinds {
        let mut words = args!["--assess", "--verbose=4"];
        words.extend(kind);
        words.push(path.into());
        let assessed = tools.capture("spctl", words)?;
        if assessed.code != 0 || !assessed.stderr.contains(NOTARIZED) {
            return Err(fail(format!(
                "Gatekeeper refuses {}: {}",
                name(path),
                assessed.stderr.trim()
            )));
        }
    }
    Ok(())
}
