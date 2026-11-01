//! The identity a build of the app is made with, as Tauri's own build reads it: the app's
//! `tauri.conf.json`, the target's own file over it (`tauri.windows.conf.json`), and over both
//! what Tauri's command line was given with `--config`, which it hands the build as
//! `TAURI_CONFIG`, each merged as a JSON merge patch. The build script writes it into the app's
//! file (`BUILT_IDENTITY`, src/lib.rs), where a test reads it without starting the app
//! (app/scripts/built-identity.mjs, T-951).

use serde_json::Value;

/// `patch` merged into `target` as a JSON merge patch (RFC 7386): an object's fields one by
/// one, a null taking a field away, anything else in place of what was there.
pub fn merge(target: &mut Value, patch: &Value) {
    let Value::Object(fields) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(serde_json::Map::new());
    }
    if let Value::Object(into) = target {
        for (name, value) in fields {
            if value.is_null() {
                into.remove(name);
            } else {
                merge(into.entry(name.clone()).or_insert(Value::Null), value);
            }
        }
    }
}

/// The identifier of the configuration `base` with `patches` merged over it in turn.
pub fn identity_of(base: &Value, patches: &[&Value]) -> Option<String> {
    let mut config = base.clone();
    for patch in patches {
        merge(&mut config, patch);
    }
    config
        .get("identifier")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The identity a build for `target_os` is made with: `tauri.conf.json`, then the target's own
/// file, then `given` (`TAURI_CONFIG`), as `read` (the build's folder) and the build's
/// environment have them. The files it read are said, for the build to be run again when they
/// change: only those that are there, as tauri-build says its own. Cargo takes a file it is
/// told to watch that is not there for one that changed, and every build would make the app
/// again (`tauri.macos.conf.json`, which the Mac has not; T-978).
pub fn built_identity(
    read: &dyn Fn(&str) -> Result<Option<String>, String>,
    target_os: &str,
    given: Option<&str>,
) -> Result<(String, Vec<String>), String> {
    let parse = |text: &str| serde_json::from_str::<Value>(text).map_err(|error| error.to_string());
    let base = read("tauri.conf.json")?.ok_or("the app has no tauri.conf.json")?;
    let base = parse(&base)?;
    let platform = format!("tauri.{target_os}.conf.json");
    let mut patches = Vec::new();
    let mut files = vec!["tauri.conf.json".to_owned()];
    if let Some(text) = read(&platform)? {
        patches.push(parse(&text)?);
        files.push(platform);
    }
    if let Some(text) = given {
        patches.push(parse(text)?);
    }
    let patches: Vec<&Value> = patches.iter().collect();
    let identity =
        identity_of(&base, &patches).ok_or("Tauri's configuration names no identifier")?;
    Ok((identity, files))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The release's configuration says the release's identity; a smoke's `--config` over it
    /// says the smoke's, and the target's own file, which names none, changes nothing.
    #[test]
    fn a_build_is_of_the_identity_its_configuration_and_its_command_line_say() {
        let base = json!({
            "identifier": "dev.ngvoicu.consensflow",
            "productName": "ConsensFlow",
            "build": { "beforeBuildCommand": "npm run prepare:app" },
        });
        let windows = json!({ "bundle": { "windows": { "nsis": {} } } });
        let smoke = json!({
            "identifier": "dev.ngvoicu.consensflow.smoke",
            "build": { "beforeBuildCommand": null },
        });
        assert_eq!(
            identity_of(&base, &[&windows]).as_deref(),
            Some("dev.ngvoicu.consensflow")
        );
        assert_eq!(
            identity_of(&base, &[&windows, &smoke]).as_deref(),
            Some("dev.ngvoicu.consensflow.smoke")
        );
        let mut merged = base.clone();
        merge(&mut merged, &smoke);
        assert_eq!(
            merged,
            json!({
                "identifier": "dev.ngvoicu.consensflow.smoke",
                "productName": "ConsensFlow",
                "build": {},
            })
        );
        assert_eq!(identity_of(&json!({ "productName": "x" }), &[]), None);
    }

    /// A build reads its identity from the files of its folder and from what Tauri's command
    /// line was given: a smoke's `tauri build --config` makes the smoke's, on every target, and
    /// a target's own file that names one makes that. The files said are the ones there.
    #[test]
    fn a_build_reads_its_identity_from_its_files_and_from_what_tauri_was_given() {
        let files = |windows: Option<&'static str>| {
            move |name: &str| -> Result<Option<String>, String> {
                Ok(match name {
                    "tauri.conf.json" => Some(r#"{"identifier":"dev.ngvoicu.consensflow"}"#.into()),
                    "tauri.windows.conf.json" => windows.map(str::to_owned),
                    _ => None,
                })
            }
        };
        let smoke = Some(r#"{"identifier":"dev.ngvoicu.consensflow.smoke"}"#);
        let plain = files(Some(r#"{"bundle":{}}"#));
        for target in ["windows", "macos"] {
            assert_eq!(
                built_identity(&plain, target, None).map(|(identity, _)| identity),
                Ok("dev.ngvoicu.consensflow".to_owned()),
                "{target}"
            );
            assert_eq!(
                built_identity(&plain, target, smoke).map(|(identity, _)| identity),
                Ok("dev.ngvoicu.consensflow.smoke".to_owned()),
                "{target}"
            );
        }
        // No file of the target: the configuration's alone, and only it to watch.
        assert_eq!(
            built_identity(&files(None), "windows", smoke),
            Ok((
                "dev.ngvoicu.consensflow.smoke".to_owned(),
                vec!["tauri.conf.json".to_owned()]
            ))
        );
        let own = files(Some(r#"{"identifier":"dev.example.windows"}"#));
        assert_eq!(
            built_identity(&own, "windows", None),
            Ok((
                "dev.example.windows".to_owned(),
                vec![
                    "tauri.conf.json".to_owned(),
                    "tauri.windows.conf.json".to_owned()
                ]
            ))
        );
        assert!(built_identity(&|_| Ok(None), "windows", None).is_err());
    }
}
