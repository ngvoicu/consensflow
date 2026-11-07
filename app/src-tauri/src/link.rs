//! A link a program printed in a window, opened in the human's browser (T-1291).
//!
//! The page asks for it (`open_link`) when the human clicks a link in a terminal with Cmd (a Mac)
//! or Ctrl (Windows) held. The page never opens one itself: its webview is the app's, and the
//! browser is the human's. A program can print any address, and hide it behind any words (an
//! OSC 8 hyperlink), so what may open is decided here, whatever the page checked: a web page
//! (`http` or `https`, never `file:`, `javascript:`, `data:` or an app's own scheme), of no more
//! than 2,048 characters, with no control character in it.

use serde_json::{json, Value};
use url::Url;

/// The most characters an address may have, counted as characters and not bytes.
const MAX_CHARS: usize = 2048;

/// The web page `address` names, if it may open.
///
/// The control characters are looked for in what the program printed and not in what the parser
/// makes of it: the parser drops a tab or a line break from the middle of an address, so one
/// with such a character in it would read as another address.
pub(crate) fn checked(address: &str) -> Result<Url, String> {
    if address.chars().count() > MAX_CHARS {
        return Err(format!("The address is longer than {MAX_CHARS} characters"));
    }
    if address.chars().any(char::is_control) {
        return Err("The address has a control character".into());
    }
    let url = Url::parse(address).map_err(|_| "That is not a web address".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!(
            "Only http and https addresses open, not {}:",
            url.scheme()
        ));
    }
    Ok(url)
}

/// Opens `address` as `open` opens a page, if it may open. The opener is given the address as
/// the parser writes it: the same page, with no space, quote or control character in it.
pub(crate) fn open_with(
    open: impl FnOnce(&str) -> Result<(), String>,
    address: &str,
) -> Result<(), String> {
    open(checked(address)?.as_str())
}

/// What opens a page in the tests of the command: none by default, and then the system's own,
/// which a test must never reach with an address that may open (it would open a page in the
/// browser of the machine it runs on). A test of the command stands one in.
#[cfg(test)]
pub(crate) static TEST_OPENER: std::sync::Mutex<Option<Opener>> = std::sync::Mutex::new(None);

/// A stand-in for the system's opener.
#[cfg(test)]
pub(crate) type Opener = fn(&str) -> Result<(), String>;

/// Opens `address` in the default browser, the way the update's download page is opened: one
/// argument to the system's opener, never through a shell.
pub(crate) fn open(address: &str) -> Result<(), String> {
    #[cfg(test)]
    if let Some(stand_in) = *TEST_OPENER.lock().expect("the stand-in opener") {
        return open_with(stand_in, address);
    }
    open_with(crate::update_page::open, address)
}

/// The page's ask to open a link a program printed in a window.
#[tauri::command]
pub async fn open_link(url: String) -> Value {
    crate::panics::settled("open_link", async move {
        // The opener is waited for, and a first start of a browser is slow.
        match tauri::async_runtime::spawn_blocking(move || open(&url)).await {
            Ok(Ok(())) => json!({"ok": true}),
            Ok(Err(error)) => json!({"ok": false, "error": error}),
            Err(error) => json!({"ok": false, "error": format!("open_link: {error}")}),
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// What `open_with` hands an opener that records it, or why it opened nothing.
    fn asked(address: &str) -> (Result<(), String>, Vec<String>) {
        let given = RefCell::new(Vec::new());
        let opened = open_with(
            |page| {
                given.borrow_mut().push(page.to_owned());
                Ok(())
            },
            address,
        );
        (opened, given.into_inner())
    }

    /// The addresses of a query and a fragment are the opener's one argument as the program
    /// printed them: an `&` does not end it, a `%` does not start an escape, a `#` does not cut it.
    #[test]
    fn a_web_address_with_an_ampersand_a_percent_and_a_hash_reaches_the_opener_whole() {
        for address in [
            "https://example.com/docs/a?x=1&y=%20z#frag",
            "http://localhost:8080/p?a=%26&b=%25&c=100%#h&i=j",
            "https://example.org/a%2Fb?q=a%26b&n=2#top",
            "https://example.com/search?q=rust&lang=en&page=2",
            "https://example.com/#/route?x=1&y=2",
        ] {
            let (opened, given) = asked(address);
            assert_eq!(opened, Ok(()), "{address}");
            assert_eq!(given, [address], "{address}");
        }
    }

    /// The address the opener gets is the page's, written as the parser writes it, so that a
    /// space, a quote or an upper-case host in what a program printed is not the opener's to read.
    #[test]
    fn an_address_is_opened_as_the_parser_writes_it() {
        for (printed, written) in [
            ("https://example.com", "https://example.com/"),
            (
                "HTTPS://Example.COM/Path?Q=1",
                "https://example.com/Path?Q=1",
            ),
            ("https://example.com/a b", "https://example.com/a%20b"),
            ("https://example.com/\"q\"", "https://example.com/%22q%22"),
            (
                "https://example.com/?a=\"1\"",
                "https://example.com/?a=%221%22",
            ),
            ("https://example.com:443/x", "https://example.com/x"),
            (" https://example.com/x ", "https://example.com/x"),
        ] {
            let (opened, given) = asked(printed);
            assert_eq!(opened, Ok(()), "{printed:?}");
            assert_eq!(given, [written], "{printed:?}");
        }
    }

    #[test]
    fn an_address_that_is_no_web_page_opens_nothing() {
        for address in [
            "file:///etc/passwd",
            "FILE:///C:/Windows/System32/calc.exe",
            "javascript:alert(document.cookie)",
            "JavaScript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "consensflow://open?x=1",
            "dev.ngvoicu.consensflow://open",
            "mailto:someone@example.com",
            "ftp://example.com/a",
            "ws://example.com/socket",
            "blob:https://example.com/0f0e0d0c",
            "ssh://example.com",
            "vscode://file/etc/passwd",
            "tel:+40123456789",
            "about:blank",
            "//example.com/a",
            "example.com/a",
            "/etc/passwd",
            "C:\\Windows\\System32\\calc.exe",
            "https://",
            "https://exa mple.com/",
            "",
        ] {
            let (opened, given) = asked(address);
            assert!(opened.is_err(), "{address:?} opened");
            assert_eq!(
                given,
                Vec::<String>::new(),
                "{address:?} reached the opener"
            );
        }
        assert_eq!(
            asked("javascript:alert(1)").0,
            Err("Only http and https addresses open, not javascript:".to_owned())
        );
        assert_eq!(
            asked("not an address").0,
            Err("That is not a web address".to_owned())
        );
    }

    #[test]
    fn an_address_of_more_than_2048_characters_opens_nothing() {
        let base = "https://example.com/";
        let at_most = format!("{base}{}", "a".repeat(2048 - base.len()));
        assert_eq!(at_most.chars().count(), 2048);
        assert_eq!(
            asked(&at_most).1,
            std::slice::from_ref(&at_most),
            "2048 characters open"
        );
        let too_long = format!("{at_most}a");
        let (opened, given) = asked(&too_long);
        assert_eq!(
            opened,
            Err("The address is longer than 2048 characters".to_owned())
        );
        assert_eq!(given, Vec::<String>::new());
        // Characters, not bytes: 2048 of two bytes each open, one more does not.
        let wide = format!("{base}{}", "é".repeat(2048 - base.len()));
        assert!(wide.len() > 2048 && wide.chars().count() == 2048);
        assert_eq!(asked(&wide).0, Ok(()));
        assert!(asked(&format!("{wide}é")).0.is_err());
    }

    #[test]
    fn an_address_with_a_control_character_opens_nothing() {
        // The parser drops a tab or a line break from the middle of an address: it is the
        // printed address that is looked at.
        assert!(Url::parse("https://exa\tmple.com/\nx").is_ok());
        for control in [
            '\0', '\t', '\n', '\r', '\u{1b}', '\u{7f}', '\u{85}', '\u{9b}',
        ] {
            for address in [
                format!("https://example.com/{control}"),
                format!("https://exa{control}mple.com/"),
                format!("{control}https://example.com/"),
                format!("https://example.com/a?b=c{control}d#e"),
            ] {
                let (opened, given) = asked(&address);
                assert_eq!(
                    opened,
                    Err("The address has a control character".to_owned()),
                    "{address:?}"
                );
                assert_eq!(given, Vec::<String>::new(), "{address:?}");
            }
        }
    }

    /// The opener a program's address reaches is the system's own, started with the address as
    /// its one argument and no shell between: a script that says what it was given.
    #[cfg(unix)]
    #[test]
    fn the_address_reaches_the_systems_opener_as_one_argument_with_no_shell_between() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::Duration;

        let folder = tempfile::tempdir().expect("a folder");
        let said = folder.path().join("said");
        let opener = folder.path().join("open");
        std::fs::write(
            &opener,
            format!(
                "#!/bin/sh\nprintf '%s\\n%s\\n' \"$#\" \"$1\" > '{}'\n",
                said.display()
            ),
        )
        .expect("the opener");
        std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755))
            .expect("an executable");
        // A shell would end the address at the `;` and the `&`, and read the rest as commands.
        let address = "https://example.com/a?x=1&y=%20z;rm;echo|cat#frag$(id)`id`";
        let written = Url::parse(address).expect("an address").to_string();
        // The opener was written a moment ago, and a program started on another thread
        // meanwhile can hold it open for writing: such a start is refused, and tried again.
        let opened = (0..50)
            .map(|_| open_with(|page| crate::update_page::open_with(&opener, page), address))
            .find(|opened| {
                let busy = matches!(opened, Err(refused) if refused.contains("Text file busy"));
                if busy {
                    std::thread::sleep(Duration::from_millis(100));
                }
                !busy
            })
            .expect("the opener started");
        assert_eq!(opened, Ok(()));
        assert_eq!(
            std::fs::read_to_string(&said).expect("what it said"),
            format!("1\n{written}\n")
        );
    }

    /// The page's origin, as the app's webview has it on this system: Tauri refuses the commands of
    /// a page of any other as a remote one's ("open_link not allowed. Plugin not found").
    const PAGE_ORIGIN: &str = if cfg!(windows) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    };

    /// What the command's stand-in opener was given.
    static OPENED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    fn record(page: &str) -> Result<(), String> {
        OPENED.lock().expect("the record").push(page.to_owned());
        Ok(())
    }

    /// The command the page asks, through the app's own list of them (`with_page`), with a
    /// stand-in for the system's opener (`TEST_OPENER`): an address that may not open is
    /// answered with why and reaches no opener, one that may is handed to it as the parser
    /// writes it and answered with ok.
    #[test]
    fn the_page_is_told_why_an_address_did_not_open_and_one_that_may_is_opened() {
        *TEST_OPENER.lock().expect("the stand-in") = Some(record);
        let app = crate::with_page(tauri::test::mock_builder())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");
        let ask = |address: String| {
            tauri::test::get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: "open_link".into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: PAGE_ORIGIN.parse().expect("invoke URL"),
                    body: tauri::ipc::InvokeBody::Json(json!({ "url": address })),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            )
            .expect("the command answers")
            .deserialize::<Value>()
            .expect("JSON")
        };
        let long = format!("https://example.com/{}", "a".repeat(2048));
        for (address, error) in [
            (
                "file:///etc/passwd".to_owned(),
                "Only http and https addresses open, not file:",
            ),
            (
                "javascript:alert(1)".to_owned(),
                "Only http and https addresses open, not javascript:",
            ),
            (long, "The address is longer than 2048 characters"),
            (
                "https://example.com/\u{1b}[31m".to_owned(),
                "The address has a control character",
            ),
        ] {
            assert_eq!(ask(address), json!({"ok": false, "error": error}));
        }
        assert_eq!(
            *OPENED.lock().expect("the record"),
            Vec::<String>::new(),
            "an address that may not open reached the opener"
        );
        assert_eq!(
            ask("HTTPS://Example.com/a?x=1&y=%20z#frag".to_owned()),
            json!({"ok": true})
        );
        assert_eq!(
            *OPENED.lock().expect("the record"),
            ["https://example.com/a?x=1&y=%20z#frag"]
        );
        *TEST_OPENER.lock().expect("the stand-in") = None;
    }
}
