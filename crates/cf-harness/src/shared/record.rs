//! What every harness's record reader shares: the shape of a reading, a
//! session's file found under a harness's folder, a JSONL file read on from
//! where the last look stopped, the transcript followed from look to look, a
//! SQLite store read as `node:sqlite` read it, the readers kept from look to
//! look, and the user's home the stores are under.

pub(crate) mod cache;
pub(crate) mod find;
pub(crate) mod followed;
pub(crate) mod jsonl;
pub(crate) mod key;
pub(crate) mod reading;
pub(crate) mod sort;
pub(crate) mod sqlite;

use cf_base::env::Env;

/// The user's home, where the harnesses keep their stores (`home`):
/// `HOME`, else `USERPROFILE`, as the environment holds it. An empty one is
/// none, and `USERPROFILE` is not read when `HOME` is there at all.
pub(crate) fn home(env: &Env) -> Result<String, String> {
    match env.os("HOME").or_else(|| env.os("USERPROFILE")) {
        Some(home) if !home.is_empty() => Ok(home.to_string_lossy().into_owned()),
        _ => Err("missing home in env".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_is_home_else_userprofile_and_never_empty() {
        let home_of = |vars: &[(&str, &str)]| home(&Env::from_vars(vars.iter().copied()));
        assert_eq!(
            home_of(&[("HOME", "/h"), ("USERPROFILE", "C:\\u")]).unwrap(),
            "/h"
        );
        assert_eq!(home_of(&[("USERPROFILE", "C:\\u")]).unwrap(), "C:\\u");
        for vars in [
            &[][..],
            &[("HOME", "")],
            &[("HOME", ""), ("USERPROFILE", "C:\\u")],
        ] {
            assert_eq!(
                home_of(vars).unwrap_err(),
                "missing home in env",
                "{vars:?}"
            );
        }
    }
}
