//! libuv's names and words as Node prints them: the golden of the platform
//! this runs on (`tests/goldens/errno/<platform>.json`, which `npm run
//! goldens:errno` writes from `util.getSystemErrorMap()`). The words of every
//! name are held on every platform; on Unix so is the name of every errno,
//! which must be the one Node gives it, with none named that Node leaves
//! `UNKNOWN`. A platform with no golden fails: it is to be recorded there.

// The golden's own reading: a failure in it is the test's.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use cf_base::file::uv_words;
use serde_json::Value;

/// The number of errors in libuv's map, the same on every platform: a
/// golden that shrinks or grows fails, and is recorded again on purpose.
const ERRORS: usize = 85;

/// The platform as Node names it (`process.platform`).
fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// The errors of the golden, each `{ code, name, words }`: libuv's number
/// for it, its name and its words.
fn golden() -> Vec<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join("errno")
        .join(format!("{}.json", platform()));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "no errno golden for {}: {error}; run `node tests/goldens/errno/record.mjs` on it ({})",
            platform(),
            path.display()
        )
    });
    let golden: Value = serde_json::from_str(&text).unwrap();
    golden["errors"].as_array().unwrap().clone()
}

#[test]
fn every_error_of_libuvs_map_is_worded_as_node_words_it() {
    let errors = golden();
    assert_eq!(errors.len(), ERRORS);
    for error in &errors {
        let name = error["name"].as_str().unwrap();
        assert_eq!(
            uv_words(name),
            error["words"].as_str(),
            "the words of {name}"
        );
    }
}

/// Whether libuv's number for `error` is a system's errno, negated: not one of
/// its own, the `EAI_*` family (-3000s) and the errors that have no
/// counterpart (-4000s).
#[cfg(unix)]
fn is_a_system_errno(error: &Value) -> bool {
    error["code"].as_i64().unwrap() > -3000
}

#[cfg(unix)]
#[test]
fn every_errno_is_named_as_node_names_it_and_none_that_node_leaves_unknown() {
    use cf_base::file::errno_name;
    use std::io;

    let errors = golden();
    // Every errno up to well past the greatest any Unix has (Linux's is 133).
    for errno in 1..=255 {
        let expected = errors
            .iter()
            .find(|error| {
                is_a_system_errno(error) && error["code"].as_i64() == Some(-i64::from(errno))
            })
            .map(|error| error["name"].as_str().unwrap());
        assert_eq!(
            errno_name(&io::Error::from_raw_os_error(errno)),
            expected,
            "errno {errno}"
        );
    }
}
