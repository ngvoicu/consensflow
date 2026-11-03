//! The screens as a started daemon serves them: on its own address under the
//! token its handle line gave, over its own environment. (What each
//! screen answers is held to Node's recordings in `tests/screens`.)

use tokio::io::AsyncReadExt;

use super::*;

/// One request to the daemon's own address: its status and its body.
async fn call(
    url: &str,
    method: &str,
    target: &str,
    token: Option<&str>,
    body: &str,
) -> (u16, String) {
    let address = url.strip_prefix("http://").unwrap().trim_end_matches('/');
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let auth = token.map_or_else(String::new, |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    let request = format!(
        "{method} {target} HTTP/1.1\r\nHost: t\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut answered = Vec::new();
    stream.read_to_end(&mut answered).await.unwrap();
    let text = String::from_utf8_lossy(&answered).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    (
        head.split(' ').nth(1).unwrap().parse().unwrap(),
        body.to_owned(),
    )
}

#[tokio::test]
async fn the_daemon_serves_the_screens_under_its_token_over_its_own_environment() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let rig = started(root.path()).await;
            let handle = rig.daemon.handle().clone();

            let (status, page) =
                call(&handle.url, "GET", "/harnesses", Some(&handle.token), "").await;
            assert_eq!(status, 200);
            assert!(page.contains(&format!("\nconst TOKEN = \"{}\";\n", handle.token)));
            let (status, page) = call(
                &handle.url,
                "GET",
                &format!("/harnesses?token={}", handle.token),
                None,
                "",
            )
            .await;
            assert_eq!(status, 200);
            assert!(page.contains("<title>ConsensFlow Harnesses</title>"));

            // Nobody else: no token, and none of the agents' windows' (which the API checks).
            let (status, body) = call(&handle.url, "GET", "/harnesses", None, "").await;
            assert_eq!(
                (status, body.as_str()),
                (401, r#"{"error":"unauthorized"}"#)
            );
            let (status, body) = call(
                &handle.url,
                "GET",
                "/harnesses",
                Some("a-window-s-token"),
                "",
            )
            .await;
            assert_eq!(
                (status, body.as_str()),
                (401, r#"{"error":"unauthorized"}"#)
            );
        })
        .await;
}
