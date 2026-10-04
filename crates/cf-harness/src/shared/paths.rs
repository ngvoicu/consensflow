//! Where the harnesses keep their things, as `src/harnesses.js` finds them:
//! the folders of each harness's own module start from the user's home here.

use cf_base::env::Env;

/// The user's home (`home`, `src/harnesses.js`): `HOME`, else `USERPROFILE`,
/// whichever is set, empty or not, as `??` took them.
///
/// Kept from Node on purpose: Node fell back to `os.homedir()`, which reads
/// the process's own environment; a Rust module may not, so an environment
/// with neither fails with `missing home in env`, the sentence of the
/// records' own `home`, which differs from this one in taking an empty value
/// for none.
pub(crate) fn home(env: &Env) -> Result<String, String> {
    env.os("HOME")
        .or_else(|| env.os("USERPROFILE"))
        .map(|home| home.to_string_lossy().into_owned())
        .ok_or_else(|| "missing home in env".to_owned())
}
