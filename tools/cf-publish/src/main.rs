//! `cf-publish`: see the library for what it does. The environment is read
//! here, once, and handed down.

#![forbid(unsafe_code)]
// The one place the environment is read; clippy.toml asks for exactly this.
#![allow(clippy::disallowed_methods)]

use std::io;
use std::process::ExitCode;

use cf_publish::Environment;

/// The variable `name`, where it is set.
fn variable(name: &str) -> Option<String> {
    std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let env = Environment {
        event_name: variable("GITHUB_EVENT_NAME"),
        ref_type: variable("GITHUB_REF_TYPE"),
        ref_name: variable("GITHUB_REF_NAME"),
        gh_repo: variable("GH_REPO"),
        github_repository: variable("GITHUB_REPOSITORY"),
    };
    let status = cf_publish::run(
        &args,
        &env,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    );
    ExitCode::from(status)
}
