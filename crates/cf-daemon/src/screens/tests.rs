//! The screens' paths: which are theirs, held to the list in
//! `agents-server.js`, and that none is answered until they land.

use hyper::Method;

use super::*;
use crate::api::body::Body;

#[test]
fn the_screens_paths_are_the_seven_node_lists() {
    for (path, screen) in [
        ("/", Screen::Page),
        ("/harnesses", Screen::Page),
        ("/api/agents", Screen::Agents),
        ("/api/agents/mybuilder", Screen::Agents),
        ("/api/agents/a", Screen::Agents),
        ("/api/agents/my-agent-2", Screen::Agents),
        ("/api/preferences", Screen::Preferences),
        ("/api/harnesses/check", Screen::HarnessCheck),
        ("/api/harnesses/update", Screen::HarnessUpdate),
    ] {
        assert_eq!(recognize(path), Some(screen), "{path}");
    }
}

#[test]
fn any_other_path_is_the_agents_api_s() {
    for path in [
        "",
        "/harnesses/",
        "/api",
        "/api/agents/",
        "/api/agents/Upper",
        "/api/agents/9lives",
        "/api/agents/-dash",
        "/api/agents/under_score",
        "/api/agents/a/b",
        "/api/agents/a b",
        "/api/harnesses",
        "/api/harnesses/check/",
        "/api/preferences/x",
        "/api/whoami",
        "/api/questions/1",
        "/index.html",
    ] {
        assert_eq!(recognize(path), None, "{path:?}");
    }
}

/// Every path `agents-server.js` routes is one here: a path Node gains is not
/// left to fall through unseen.
#[test]
fn every_path_node_routes_is_one() {
    let source = include_str!("../../../../src/core/agents-server.js");
    let handle = source
        .split("async handle(request, url) {")
        .nth(1)
        .and_then(|rest| rest.split("if (!page && !api) return null").next())
        .unwrap();
    let mut seen = 0;
    for piece in handle.split("'/").skip(1) {
        let path = format!("/{}", piece.split('\'').next().unwrap());
        assert!(recognize(&path).is_some(), "{path}");
        seen += 1;
    }
    // `/`, `/harnesses`, `/api/agents`, `/api/preferences`, `/api/harnesses/check`, `/api/harnesses/update`.
    assert_eq!(seen, 6);
    assert!(handle.contains(r"/^\/api\/agents\/([a-z][a-z0-9-]*)$/"));
}

#[tokio::test]
async fn until_they_land_no_request_is_answered_here() {
    let screens = Screens {
        token: "ui".to_owned(),
        on_roster_change: Rc::new(|| Ok(())),
    };
    for path in [
        "/",
        "/harnesses",
        "/api/agents",
        "/api/preferences",
        "/api/nothing",
    ] {
        let mut asked = Request::new(
            Method::GET,
            path,
            Some("Bearer ui".to_owned()),
            Body::empty(),
        )
        .unwrap();
        assert_eq!(screens.handle(&mut asked).await, None, "{path}");
    }
}
