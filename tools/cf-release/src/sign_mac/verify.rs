//! What a download meets: every seal holds, and Gatekeeper takes both files as
//! notarized.

use std::path::Path;

use super::name;
use super::signing::{plist, ENTITLED, USAGE};
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
    asks_as_a_terminal(tools, app)?;
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

/// Holds the signed `app` to what lets a program in its panes ask macOS for the
/// microphone, the camera and Apple Events, as a terminal's may (T-540): each
/// entitlement in its signature, and each `Info.plist` key that says why.
fn asks_as_a_terminal(tools: &Tools, app: &Path) -> Result<(), Failure> {
    let signed = tools.capture("codesign", args!["-d", "--entitlements", "-", "--xml", app])?;
    let missing: Vec<&str> = ENTITLED
        .into_iter()
        .filter(|key| signed.code != 0 || !signed.stdout.contains(&format!("<key>{key}</key>")))
        .collect();
    if !missing.is_empty() {
        return Err(fail(format!(
            "{} is signed without {}",
            name(app),
            missing.join(", ")
        )));
    }
    for key in USAGE {
        if plist(tools, app, key).unwrap_or_default().is_empty() {
            return Err(fail(format!("{}'s Info.plist says no {key}", name(app))));
        }
    }
    Ok(())
}
