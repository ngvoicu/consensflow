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

/// The CLI this app runs: its own copy, with its own Node.
///
/// The app is the whole installation — someone who downloads it has not
/// installed Node, npm, or ConsensFlow, and should not have to. So the
/// bundle carries an official Node build (a Tauri sidecar) and the CLI's
/// sources (a resource), and nothing on the machine is consulted. A .app
/// launched from Finder has almost no PATH anyway, which is what made
/// depending on an installed CLI fragile in the first place.
fn bundled_cli(app: &tauri::AppHandle) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let resources = app
        .path()
        .resource_dir()
        .map_err(|error| format!("the app could not find its own resources: {error}"))?;
    let node = resources.join("binaries/node");
    let node = if node.exists() {
        node
    } else {
        // Tauri lays sidecars beside the executable in a bundled app.
        std::env::current_exe()
            .map_err(|error| format!("the app could not find itself: {error}"))?
            .parent()
            .ok_or_else(|| "the app has no directory".to_string())?
            .join("node")
    };
    let cli = resources.join("cli/bin/cf.mjs");

    if !node.exists() {
        return Err(format!("the bundled runtime is missing from this app ({node:?})"));
    }
    if !cli.exists() {
        return Err(format!("the bundled ConsensFlow is missing from this app ({cli:?})"));
    }
    Ok((node, cli))
}

/// The PATH a terminal would have.
///
/// A .app launched from Finder gets `/usr/bin:/bin:/usr/sbin:/sbin` — none of
/// the places coding agents actually live. The editor inherits our
/// environment, so without this it reports "no agents on PATH" on a machine
/// with four of them, and would install skills nowhere.
fn login_path() -> Option<String> {
    let shell = std::env::var("SHELL").ok()?;
    let output = Command::new(shell).args(["-lc", "printf %s \"$PATH\""]).output().ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() { None } else { Some(path) }
}

/// Starts the editor and returns the address to show, or a human explanation.
fn start_editor(app: &tauri::AppHandle) -> Result<(String, Child), String> {
    let (node, cli) = bundled_cli(app)?;

    let mut command = Command::new(&node);
    command.arg(&cli).args(["ui", "--json", "--no-open"]);
    if let Some(path) = login_path() {
        eprintln!("consensflow: using the login shell's PATH");
        command.env("PATH", path);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("the bundled ConsensFlow could not be started: {error}"))?;

    // Anything that goes wrong from here leaves a server running unless the
    // child is killed on the way out.
    let address = read_address(&mut child);
    match address {
        Ok(address) => Ok((address, child)),
        Err(explanation) => {
            let _ = child.kill();
            Err(explanation)
        }
    }
}

fn read_address(child: &mut Child) -> Result<String, String> {
    let line = handle_line(child)?;
    let handle: serde_json::Value = serde_json::from_str(line.trim())
        .map_err(|_| format!("the editor answered with something else: {}", line.trim()))?;
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
            let started = start_editor(app.handle());
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
