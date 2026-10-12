//! What the bare arm asks of the pane host.

use super::*;

#[test]
fn the_window_is_opened_with_the_fields_the_pane_host_takes_and_no_other() {
    let body = pane_open(
        Path::new("/work/space"),
        &["/bin/claude".to_owned(), "--session-id".to_owned()],
        &["CLAUDE_CONFIG_DIR".to_owned()],
    );
    // The pane host refuses a request that has a field it does not know (`launch`
    // was sent once: `invalid-body: unknown field`, found by playing the arm against
    // the stand-in agent).
    let mut fields: Vec<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        ["argv", "cwd", "dropEnv", "env", "generation", "id"]
    );
    assert_eq!(body["id"], PANE);
    assert_eq!(body["generation"], 1);
    assert_eq!(body["cwd"], "/work/space");
    assert_eq!(body["argv"], json!(["/bin/claude", "--session-id"]));
    assert_eq!(body["env"], json!({}));
    assert_eq!(body["dropEnv"], json!(["CLAUDE_CONFIG_DIR"]));
}
