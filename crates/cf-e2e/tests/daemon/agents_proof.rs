//! The proof of the agents screens (`cf_e2e::agents_proof`) that the packaged
//! smoke runs against the daemon a built app chose: held here to what it is for,
//! which is failing when the daemon's agents are wrong, and run against the
//! native daemon from the checkout (`npm run test:daemons`, or `npm run
//! test:agents` for this alone).

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use cf_e2e::agents_proof::{prove, Daemon as Target, GONE, PRESENT};
use cf_e2e::daemon::Home;
use cf_e2e::files;
use cf_e2e::http::server::{Answer, Request, Response, Server};
use regex::Regex;
use serde_json::{json, Value};

use crate::{secs, Outcome};

const TOKEN: &str = "proof-ui-token";

/// A daemon's agents screens, as far as the proof asks, with one thing wrong
/// where `fault` says so (nothing, for a right one). The server stands in for
/// the daemon, and writes the roster file in `home` as the daemon does.
fn agents_api(home: &Path, fault: &'static str) -> std::io::Result<Server> {
    let size = if fault == "catalog" { 119 } else { 120 };
    let catalog: Vec<Value> = (0..size)
        .map(|at| {
            json!({
                "name": if at == 0 { "pygmalion".to_owned() } else { format!("catalog-{at}") },
                "model": if at == 0 && fault != "model" { "codex-image" } else { "fake" },
                "harness": "codex",
                "profile": { "workTier": "standard" },
            })
        })
        .collect();
    let mine = Mutex::new(Vec::<Value>::new());
    let file = home.join("agents.json");
    let write = move |mine: &[Value]| {
        // A roster that cannot be written is one the proof will not find.
        let _ = files::write(
            &file,
            json!({ "schemaVersion": 1, "agents": mine }).to_string(),
        );
    };
    write(&[]);
    let page = move |title: &str| {
        let present: Vec<&str> = PRESENT
            .iter()
            .copied()
            .filter(|text| fault != "page" || *text != "Work tier")
            .collect();
        let old = if fault == "old-page" { GONE[0] } else { "" };
        format!("<title>{title}</title>{}{old}", present.join(" "))
    };
    let view = move |row: &Value| {
        json!({
            "name": row["id"],
            "harness": row["kind"],
            "model": row["model"],
            "effort": row["effort"],
            "custom": true,
            "profile": { "workTier": if fault == "tier" { "standard" } else { "light" } },
        })
    };
    Server::start(move |request: &Request| {
        let bearer = request
            .header("authorization")
            .map(|value| value.strip_prefix("Bearer ").unwrap_or(value))
            .filter(|value| !value.is_empty());
        let presented = bearer
            .or_else(|| request.query("token"))
            .unwrap_or_default();
        if presented != TOKEN && fault != "open" {
            return Answer::Respond(Response::json(401, &json!({ "error": "unauthorized" })));
        }
        let mut mine = mine.lock().unwrap_or_else(PoisonError::into_inner);
        match (request.method.as_str(), request.path()) {
            ("GET", "/") => Answer::Respond(Response::html(page("ConsensFlow — Agents"))),
            ("GET", "/harnesses") => Answer::Respond(Response::html(page("ConsensFlow Harnesses"))),
            ("GET", "/api/agents") => {
                let mut agents = catalog.clone();
                agents.extend(mine.iter().map(view));
                Answer::Respond(Response::json(200, &json!({ "agents": agents })))
            }
            ("POST", "/api/agents") => {
                let input: Value = serde_json::from_str(&request.body).unwrap_or(Value::Null);
                let mut row = serde_json::Map::new();
                row.insert("id".to_owned(), input["name"].clone());
                row.insert("kind".to_owned(), input["harness"].clone());
                row.insert("model".to_owned(), input["model"].clone());
                if fault != "effort" {
                    row.insert("effort".to_owned(), input["effort"].clone());
                }
                if fault == "profile-saved" {
                    row.insert("profile".to_owned(), json!({ "workTier": "light" }));
                }
                mine.push(Value::Object(row));
                write(&mine);
                let saved = mine.last().map_or(Value::Null, view);
                Answer::Respond(Response::json(201, &json!({ "agent": saved })))
            }
            ("DELETE", path) if path.starts_with("/api/agents/") => {
                let name = &path["/api/agents/".len()..];
                if !mine.iter().any(|row| row["id"] == name) && fault != "delete-catalog" {
                    return Answer::Respond(Response::json(
                        400,
                        &json!({ "error": "a catalog agent is not yours to delete" }),
                    ));
                }
                if fault != "delete-kept" {
                    mine.retain(|row| row["id"] != name);
                    write(&mine);
                }
                Answer::Respond(Response::status(204))
            }
            _ => Answer::Respond(Response::json(404, &json!({ "error": "not found" }))),
        }
    })
}

/// The proof, run against an agents API with `fault` in it.
fn proving(fault: &'static str) -> cf_e2e::Result<()> {
    let home = tempfile::tempdir().map_err(|source| cf_e2e::Error::File {
        action: "make a folder in",
        path: std::env::temp_dir(),
        source,
    })?;
    let api = agents_api(home.path(), fault).map_err(|source| cf_e2e::Error::File {
        action: "start a server for",
        path: home.path().to_path_buf(),
        source,
    })?;
    prove(Target {
        url: &api.url(),
        token: TOKEN,
        home: home.path(),
    })
}

#[test]
fn passes_a_daemon_whose_agents_are_right() -> Outcome {
    proving("")?;
    Ok(())
}

#[test]
fn fails_a_daemon_whose_agents_are_wrong_whichever_way() -> Outcome {
    for (fault, says) in [
        ("open", r" with no token: 200"),
        ("catalog", r"packaged preset count"),
        ("model", r"'codex-image'"),
        ("effort", r"'low'"),
        ("profile-saved", r"\[ 'low', 'gpt-6-astra', true \]|true"),
        ("tier", r"'light'"),
        ("page", r"Work tier"),
        ("old-page", r#"gone: id="catalog-section""#),
        ("delete-catalog", r"400"),
        ("delete-kept", r"."),
    ] {
        match proving(fault) {
            Ok(()) => panic!("{fault}: the proof passed a daemon whose agents are wrong"),
            Err(failed) => assert!(
                Regex::new(says)?.is_match(&failed.to_string()),
                "{fault}: {failed} does not say {says}"
            ),
        }
    }
    Ok(())
}

#[test]
fn serves_the_agents_as_the_packaged_smoke_holds_the_built_app_to() -> Outcome {
    let home = Home::new()?;
    let mut daemon = home.daemon()?;
    // Its handle is the first line it says: where it listens, and the app's token.
    let handle = daemon.handle(secs(60))?;
    prove(Target {
        url: handle["url"].as_str().unwrap_or_default(),
        token: handle["token"].as_str().unwrap_or_default(),
        home: &home.consensflow(),
    })?;
    let roster: Value = serde_json::from_str(&files::read_string(
        &home.consensflow().join("agents.json"),
    )?)?;
    assert_eq!(
        roster["agents"].as_array().map(Vec::len),
        Some(0),
        "the agent saved is gone from the file"
    );
    daemon.stop()?;
    Ok(())
}
