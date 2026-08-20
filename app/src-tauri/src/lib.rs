use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

/// The desktop window around ConsensFlow's roster editor.
///
/// The editor itself stays exactly what it is on the command line: a
/// loopback HTTP server the CLI starts (`consensflow ui --json --no-open`),
/// which prints one handle line saying where it is listening and with which
/// token. This app starts that process, points a window at it, and kills it
/// on quit — so there is one implementation of the editor, not two, and
/// nothing is left running after the window closes.
struct Editor(Mutex<Option<Child>>);

fn handle_line(child: &mut Child) -> Result<String, String> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "the editor process gave no output to read".to_string())?;
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .map_err(|error| format!("could not read the editor's handle line: {error}"))?;
    if line.trim().is_empty() {
        return Err("the editor exited before saying where it was listening".to_string());
    }
    Ok(line)
}

/// Where the CLI is, for a program that did not inherit a shell.
///
/// A .app launched from Finder gets a minimal PATH — `/usr/bin:/bin:...` —
/// which never contains a global npm bin directory. So: try PATH, then the
/// usual install locations, then ask the user's login shell, which is the
/// only thing that truly knows.
fn locate_cli() -> Option<String> {
    if Command::new("consensflow")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
    {
        return Some("consensflow".to_string());
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        "/opt/homebrew/bin/consensflow".to_string(),
        "/usr/local/bin/consensflow".to_string(),
        format!("{home}/.local/bin/consensflow"),
        format!("{home}/.volta/bin/consensflow"),
    ];
    if let Some(found) = candidates.iter().find(|path| std::path::Path::new(path).exists()) {
        return Some(found.clone());
    }

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let resolved = Command::new(shell)
        .args(["-lc", "command -v consensflow"])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&resolved.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

/// Starts the editor and returns the address to show, or a human explanation.
fn start_editor() -> Result<(String, Child), String> {
    let cli = locate_cli().ok_or_else(|| {
        "ConsensFlow was not found on this machine. Install it with \
         `npm install -g ngvoicu/consensflow`, then open this app again."
            .to_string()
    })?;

    let mut child = Command::new(&cli)
        .args(["ui", "--json", "--no-open"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("`{cli} ui` could not be started: {error}"))?;

    // Anything that goes wrong from here leaves a server running unless the
    // child is killed on the way out.
    let address = read_address(&mut child, &cli);
    match address {
        Ok(address) => Ok((address, child)),
        Err(explanation) => {
            let _ = child.kill();
            Err(explanation)
        }
    }
}

fn read_address(child: &mut Child, cli: &str) -> Result<String, String> {
    let line = handle_line(child)?;
    let handle: serde_json::Value = serde_json::from_str(line.trim()).map_err(|_| {
        format!(
            "`{cli} ui --json` answered with something else: {}. This app needs a newer \
             ConsensFlow — update it with `npm install -g ngvoicu/consensflow`.",
            line.trim()
        )
    })?;
    let url = handle["url"]
        .as_str()
        .ok_or_else(|| "the editor did not say where it is listening".to_string())?;
    let token = handle["token"]
        .as_str()
        .ok_or_else(|| "the editor did not hand over a token".to_string())?;
    // macOS App Transport Security only lets the webview load cleartext http
    // from the domain declared in the bundle — `localhost`, not the literal
    // loopback address the server prints.
    Ok(format!("{}?token={token}", url.replace("127.0.0.1", "localhost")))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(Editor(Mutex::new(None)))
        .setup(|app| {
            // The window is built AT the editor's address rather than being
            // navigated afterwards: a window that starts somewhere else and
            // moves can fail silently and leave a blank frame.
            let started = start_editor();
            let url = match &started {
                Ok((address, _)) => {
                    eprintln!("consensflow: editor at {address}");
                    WebviewUrl::External(address.parse()?)
                }
                Err(explanation) => {
                    eprintln!("consensflow: {explanation}");
                    WebviewUrl::default()
                }
            };

            let window = WebviewWindowBuilder::new(app, "main", url)
                .title("ConsensFlow")
                .inner_size(880.0, 900.0)
                .min_inner_size(560.0, 480.0)
                .on_navigation(|target| {
                    eprintln!("consensflow: navigating to {target}");
                    true
                })
                .on_page_load(|_window, payload| {
                    eprintln!("consensflow: page {:?} {}", payload.event(), payload.url());
                })
                .build()?;
            eprintln!("consensflow: window built");

            match started {
                Ok((_, child)) => {
                    *app.state::<Editor>().0.lock().unwrap() = Some(child);
                }
                Err(explanation) => {
                    // A blank window explains nothing; say what went wrong and
                    // what to do about it, in the window itself.
                    window.eval(&format!(
                        "document.body.dataset.error = {};",
                        serde_json::to_string(&explanation).unwrap_or_default()
                    ))?;
                }
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the ConsensFlow app");

    app.run(|handle, event| {
        // The editor is ours: it goes when the window does, so no stray
        // server survives the app.
        if matches!(event, RunEvent::Exit) {
            if let Some(mut child) = handle.state::<Editor>().0.lock().unwrap().take() {
                let _ = child.kill();
            }
        }
    });
}
