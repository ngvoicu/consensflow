use serde_json::json;

use super::*;

#[test]
fn a_status_is_a_success_from_200_to_299_and_no_other() {
    for status in [200, 204, 299] {
        assert!(succeeded(status), "{status}");
    }
    for status in [199, 300, 301, 401, 403, 500, 503] {
        assert!(!succeeded(status), "{status}");
    }
}

#[test]
fn an_id_is_ses_and_one_or_more_ascii_letters_and_digits_and_nothing_else() {
    for id in ["ses_a", "ses_ABC123", "ses_0", "ses_abc123XYZ"] {
        assert!(is_session_id(id), "{id:?}");
    }
    for id in [
        "",
        "ses_",
        "ses",
        "ses_a-b",
        "ses_a b",
        "ses_\u{e9}",
        "xses_a",
        "ses_a\n",
        "SES_a",
        "ses__a",
        " ses_a",
    ] {
        assert!(!is_session_id(id), "{id:?}");
    }
}

#[test]
fn a_body_is_the_json_in_it_with_one_byte_order_mark_taken_off_and_none_where_there_is_none() {
    assert_eq!(json_of(br#"{"a":1}"#), Some(json!({ "a": 1 })));
    assert_eq!(json_of(b"\xEF\xBB\xBF{\"a\":1}"), Some(json!({ "a": 1 })));
    assert_eq!(json_of(b"\xEF\xBB\xBF\xEF\xBB\xBF{}"), None);
    assert_eq!(json_of(b""), None);
    assert_eq!(json_of(b"not json"), None);
}
