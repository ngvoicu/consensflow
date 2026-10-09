//! A `gh` that is GitHub as the tests have it: it asks the simulator, which
//! the test runs in its own process, what to do. The workflow's steps run as
//! written call `gh` through the PATH; the tests put this first on it, named `gh`,
//! and `GH_SIM` says where the simulator is (`http://127.0.0.1:<port>`).
//!
//! It posts `{"args": […], "cwd": "…"}` to `/__gh` and prints what comes back
//! as `gh` would: stdout, stderr, and the status as its own. Like the real one,
//! in a folder that is no repository, it does nothing without `GH_TOKEN` (the
//! token it acts with) and `GH_REPO` (the repository it acts on): a step of the
//! workflow that runs it is held to giving it both. It ships with nothing: it
//! is built only with the `test-support` feature.

#![forbid(unsafe_code)]
// The simulator's address is the environment's.
#![allow(clippy::disallowed_methods)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::ExitCode;

use serde_json::{json, Value};

/// What the simulator answered to a call of `gh` with `args`.
fn ask(address: &str, args: &[String]) -> Result<Value, String> {
    let cwd = std::env::current_dir()
        .map_err(|cause| cause.to_string())?
        .to_string_lossy()
        .into_owned();
    let body = json!({ "args": args, "cwd": cwd }).to_string();
    let mut stream = TcpStream::connect(address).map_err(|cause| format!("{address}: {cause}"))?;
    let request = format!(
        "POST /__gh HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|cause| cause.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|cause| cause.to_string())?;
    let at = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("the simulator answered with no head")?;
    serde_json::from_slice(&response[at + 4..]).map_err(|cause| cause.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    if !set("GH_TOKEN") {
        let _ = writeln!(
            std::io::stderr(),
            "gh: To use GitHub CLI in a GitHub Actions workflow, set the GH_TOKEN environment variable"
        );
        return ExitCode::from(4);
    }
    if !set("GH_REPO") {
        let _ = writeln!(
            std::io::stderr(),
            "failed to determine base repo: set GH_REPO, or run in a git repository"
        );
        return ExitCode::from(1);
    }
    let Some(base) = std::env::var_os("GH_SIM") else {
        let _ = writeln!(std::io::stderr(), "fake gh: GH_SIM names no simulator");
        return ExitCode::from(1);
    };
    let base = base.to_string_lossy().into_owned();
    let address = base.strip_prefix("http://").unwrap_or(&base);
    match ask(address, &args) {
        Ok(answer) => {
            let _ = std::io::stdout()
                .write_all(answer["stdout"].as_str().unwrap_or_default().as_bytes());
            let _ = std::io::stderr()
                .write_all(answer["stderr"].as_str().unwrap_or_default().as_bytes());
            let status = answer["status"].as_i64().unwrap_or(1);
            ExitCode::from(u8::try_from(status).unwrap_or(1))
        }
        Err(cause) => {
            let _ = writeln!(std::io::stderr(), "fake gh: {cause}");
            ExitCode::from(1)
        }
    }
}
