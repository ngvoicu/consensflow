//! Where the harnesses keep their things, as `src/harnesses.js` finds them:
//! the folders of each harness's own module start from the user's home here.

use cf_base::env::Env;
use cf_base::path;

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

/// Where the XDG places keep data: `XDG_DATA_HOME`, an empty one too, as
/// `??` kept it, else `.local/share` in the home.
pub(crate) fn data_home(env: &Env) -> Result<String, String> {
    match env.os("XDG_DATA_HOME") {
        Some(data) => Ok(data.to_string_lossy().into_owned()),
        None => Ok(path::join(&[&home(env)?, ".local", "share"])),
    }
}

/// A variable set to something, as `env.NAME ||` and `.filter(Boolean)` take
/// it, read as Node read its environment: bytes that are no UTF-8 as U+FFFD.
pub(crate) fn set(env: &Env, name: &str) -> Option<String> {
    env.path(name)
        .map(|value| value.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_home_is_xdg_s_an_empty_one_too_else_the_home_s_share() {
        let data = |vars: &[(&str, &str)]| data_home(&Env::from_vars(vars.iter().copied()));
        assert_eq!(
            data(&[("XDG_DATA_HOME", "/x"), ("HOME", "/h")]).unwrap(),
            "/x"
        );
        assert_eq!(data(&[("XDG_DATA_HOME", ""), ("HOME", "/h")]).unwrap(), "");
        assert_eq!(
            data(&[("HOME", "/h")]).unwrap(),
            path::join(&["/h", ".local", "share"])
        );
        assert_eq!(data(&[]).unwrap_err(), "missing home in env");
    }

    #[test]
    fn a_variable_is_set_when_it_holds_something() {
        let env = Env::from_vars([("A", "x"), ("B", "")]);
        assert_eq!(set(&env, "A").as_deref(), Some("x"));
        assert_eq!(set(&env, "B"), None);
        assert_eq!(set(&env, "C"), None);
    }
}
