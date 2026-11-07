//! The Windows way to open a web page: `ShellExecuteW` (T-1307).
//!
//! The first version of the link opener started `explorer.exe` with the address as its argument,
//! as the update's download page is. Explorer reads its own command line: commas and equals signs
//! separate its fields, and a `/root` among them names a folder. A web address that holds
//! `,/root,C:/Windows/` passed every check of `link::checked` and could open a folder instead
//! of a page, and the answer of the process was never read (the review of T-1291, T-1301).
//!
//! Here the address is handed to the shell as the file to open, with the verb `open` and nothing
//! else, so the shell finds the program set for `http` or `https` and gives it the address, and
//! no command line of ours is read by anything. What the shell answers is read: 33 or more is
//! a start, and 32 or less is a refusal the page shows in its status line.
//!
//! The arguments and the answers are plain data (`Call`, `result_of`) and tested everywhere;
//! the call itself is Windows' alone, and a test build never makes it: it asks for a stand-in.

/// What `ShellExecuteW` is asked, as the NUL-ended UTF-16 strings it reads. A string that is
/// `None` is passed as a null pointer.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Call {
    /// The verb: `open`, the one the shell reads as "what a double click does".
    pub(crate) verb: Vec<u16>,
    /// The file to open: the web address.
    pub(crate) file: Vec<u16>,
    /// Its parameters: none. An address is no program's argument list.
    pub(crate) parameters: Option<Vec<u16>>,
    /// The folder to run in: none.
    pub(crate) directory: Option<Vec<u16>>,
}

/// `text` as `ShellExecuteW` reads it: UTF-16, NUL-ended.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// What the shell is asked to open `address`, which `link::checked` has written as the parser
/// does: a web address with no space, quote or control character in it, and so in ASCII.
pub(crate) fn call_for(address: &str) -> Call {
    Call {
        verb: wide("open"),
        file: wide(address),
        parameters: None,
        directory: None,
    }
}

/// What the page is told of the shell's answer. `ShellExecuteW` answers a number: more than 32
/// is a start (the instance of the program), and 32 or less is one of its errors, which this
/// says in words.
pub(crate) fn result_of(code: isize) -> Result<(), String> {
    if code > 32 {
        return Ok(());
    }
    let opener = "the program that opens web addresses";
    let why = match code {
        // SE_ERR_OOM, and the shell's own "out of memory or resources".
        0 | 8 => "Windows is out of memory".to_owned(),
        // ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND.
        2 | 3 => "Windows did not find what opens web addresses".to_owned(),
        // SE_ERR_ACCESSDENIED.
        5 => "Windows refused access to what opens web addresses".to_owned(),
        // ERROR_BAD_FORMAT.
        11 => format!("{opener} is damaged"),
        // SE_ERR_SHARE.
        26 => format!("{opener} is in use"),
        // SE_ERR_ASSOCINCOMPLETE.
        27 => format!("{opener} is set up in part"),
        // SE_ERR_DDETIMEOUT, SE_ERR_DDEFAIL.
        28 | 29 => format!("{opener} did not answer"),
        // SE_ERR_DDEBUSY.
        30 => format!("{opener} is busy"),
        // SE_ERR_NOASSOC.
        31 => "no program is set to open web addresses".to_owned(),
        // SE_ERR_DLLNOTFOUND.
        32 => format!("a library {opener} needs was not found"),
        _ => "an answer it does not know".to_owned(),
    };
    Err(format!(
        "Windows could not open the address: {why} (ShellExecute answered {code})"
    ))
}

/// Opens `address` with `execute`, the call to the shell, and says what it answered.
pub(crate) fn open_with(execute: impl FnOnce(&Call) -> isize, address: &str) -> Result<(), String> {
    result_of(execute(&call_for(address)))
}

/// A stand-in for the shell's call, which a test build asks for: it never makes the call, for
/// the call opens a page in the browser of the machine the test runs on.
#[cfg(all(windows, test))]
pub(crate) type StandIn = fn(&Call) -> isize;

/// The stand-in the test running has put in the place of the shell.
#[cfg(all(windows, test))]
pub(crate) static TEST_SHELL: std::sync::Mutex<Option<StandIn>> = std::sync::Mutex::new(None);

/// Opens `address` in the browser the human has set for web addresses.
#[cfg(windows)]
pub(crate) fn open(address: &str) -> Result<(), String> {
    open_with(execute, address)
}

/// The call to the shell: made by the app, and in a test build the stand-in's, or none.
#[cfg(windows)]
fn execute(call: &Call) -> isize {
    #[cfg(test)]
    {
        let stand_in = *TEST_SHELL.lock().expect("the stand-in shell");
        let Some(stand_in) = stand_in else {
            panic!("a test reached ShellExecuteW with no stand-in: it would open a page");
        };
        stand_in(call)
    }
    #[cfg(not(test))]
    native(call)
}

/// `ShellExecuteW`, with COM started on the thread as its documentation asks: the shell hands
/// the address to the program set for `http` through extensions that are COM objects, some of
/// which need a single-threaded apartment.
#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
fn native(call: &Call) -> isize {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let pointer = |text: &Option<Vec<u16>>| text.as_ref().map_or(null(), |text| text.as_ptr());
    // SAFETY: starts COM on this thread, in a single-threaded apartment; a start that succeeded
    // (S_OK, or S_FALSE where the thread had it in that apartment already) is ended below, and a
    // refusal (the thread is in another apartment) leaves nothing to end.
    let started = unsafe {
        CoInitializeEx(
            null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    } >= 0;
    // SAFETY: each string is NUL-ended UTF-16 owned by `call`, which outlives the call, or null
    // where it is `None`; no window owns the call.
    let answer = unsafe {
        ShellExecuteW(
            null_mut(),
            call.verb.as_ptr(),
            call.file.as_ptr(),
            pointer(&call.parameters),
            pointer(&call.directory),
            SW_SHOWNORMAL,
        )
    };
    if started {
        // SAFETY: ends the start made above, on the same thread.
        unsafe { CoUninitialize() };
    }
    answer as isize
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `ShellExecuteW` was asked for `address`, and what the page was told of its answer.
    fn asked(address: &str, answer: isize) -> (Call, Result<(), String>) {
        let mut asked = None;
        let told = open_with(
            |call| {
                asked = Some(Call {
                    verb: call.verb.clone(),
                    file: call.file.clone(),
                    parameters: call.parameters.clone(),
                    directory: call.directory.clone(),
                });
                answer
            },
            address,
        );
        (asked.expect("the shell was asked"), told)
    }

    /// The address is the file the shell opens, as it is: its commas, equals signs, `&`, `%` and
    /// `#` are no grammar of an argument list, for there is none (Explorer read a comma and an
    /// equals sign in its own arguments, and a `/root` after them).
    #[test]
    fn the_shell_is_asked_to_open_the_address_as_the_file_with_no_parameters() {
        for address in [
            "https://example.com/",
            "https://example.com/?x=1,/root,C:/Windows/",
            "https://example.com/,/root,C:/Windows/",
            "https://example.com/docs/a?x=1&y=%20z#frag",
            "https://example.com/a,b^c",
            "http://[::1]:8080/a?b=c=d",
        ] {
            let (call, told) = asked(address, 42);
            assert_eq!(told, Ok(()), "{address}");
            assert_eq!(
                call,
                Call {
                    verb: wide("open"),
                    file: wide(address),
                    parameters: None,
                    directory: None,
                },
                "{address}"
            );
        }
    }

    /// 32 or less is the shell's refusal, and the page is told which.
    #[test]
    fn an_answer_of_32_or_less_is_a_refusal_the_page_is_told() {
        for (answer, said) in [
            (0, "Windows is out of memory"),
            (2, "Windows did not find what opens web addresses"),
            (3, "Windows did not find what opens web addresses"),
            (5, "Windows refused access to what opens web addresses"),
            (8, "Windows is out of memory"),
            (11, "the program that opens web addresses is damaged"),
            (26, "the program that opens web addresses is in use"),
            (27, "the program that opens web addresses is set up in part"),
            (28, "the program that opens web addresses did not answer"),
            (29, "the program that opens web addresses did not answer"),
            (30, "the program that opens web addresses is busy"),
            (31, "no program is set to open web addresses"),
            (
                32,
                "a library the program that opens web addresses needs was not found",
            ),
            (-1, "an answer it does not know"),
        ] {
            let (_, told) = asked("https://example.com/", answer);
            assert_eq!(
                told,
                Err(format!(
                    "Windows could not open the address: {said} (ShellExecute answered {answer})"
                )),
                "{answer}"
            );
        }
    }

    /// Above 32 the shell started something: 33 is the least it answers for that.
    #[test]
    fn an_answer_above_32_is_a_success() {
        for answer in [33, 42, 1000, isize::MAX] {
            assert_eq!(asked("https://example.com/", answer).1, Ok(()), "{answer}");
        }
    }
}
