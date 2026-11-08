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
//! The arguments and the answers are plain data (`Call`, `result_of`, `com_started`) and tested
//! everywhere; the call itself is Windows' alone, and a test build never makes it: it asks for a
//! stand-in. The update's download page opens the same way (`update_page::open`), so the one
//! route is read and held in one place.

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

/// What the thread's `CoInitializeEx` answered, and whether `CoUninitialize` must follow (T-1315).
///
/// `S_OK` and `S_FALSE` (the thread had COM in this apartment already) are both a start that
/// is ended once. `RPC_E_CHANGED_MODE` is a thread that started COM in another apartment before
/// us (a multi-threaded one): COM is there and the shell can be asked, and the start is not ours
/// to end. Any other refusal (`E_OUTOFMEMORY`, `E_INVALIDARG`, ...) is a thread the shell cannot
/// be asked on, and is said to the human instead of going on without COM and leaving the shell
/// to fail in a way it cannot name.
pub(crate) fn com_started(code: i32) -> Result<bool, String> {
    /// 0x80010106: COM was started on this thread in the other apartment model.
    const RPC_E_CHANGED_MODE: i32 = 0x8001_0106_u32 as i32;
    match code {
        RPC_E_CHANGED_MODE => Ok(false),
        started if started >= 0 => Ok(true),
        refused => Err(format!(
            "Windows could not start what opens web addresses (CoInitializeEx answered 0x{:08X})",
            refused as u32
        )),
    }
}

/// Runs `run`, the shell's call, on a thread COM has been started on, and ends the start after
/// it where the start is ours to end (T-1315). A thread COM could not be started on is an error:
/// `run` is not called.
///
/// `start` is `CoInitializeEx` and `end` is `CoUninitialize`; both are the caller's, so that
/// the order of the three, and what each answer does, are tested without COM.
pub(crate) fn with_com<T>(
    start: impl FnOnce() -> i32,
    end: impl FnOnce(),
    run: impl FnOnce() -> T,
) -> Result<T, String> {
    let ours = com_started(start())?;
    let answer = run();
    if ours {
        end();
    }
    Ok(answer)
}

/// Opens `address` with `execute`, the call to the shell, and says what it answered. A call that
/// could not be made at all is its error.
pub(crate) fn open_with(
    execute: impl FnOnce(&Call) -> Result<isize, String>,
    address: &str,
) -> Result<(), String> {
    result_of(execute(&call_for(address))?)
}

/// A stand-in for the shell's call, which a test build asks for: it never makes the call, for
/// the call opens a page in the browser of the machine the test runs on.
#[cfg(all(windows, test))]
pub(crate) type StandIn = fn(&Call) -> isize;

/// The stand-in the test running has put in the place of the shell.
#[cfg(all(windows, test))]
pub(crate) static TEST_SHELL: std::sync::Mutex<Option<StandIn>> = std::sync::Mutex::new(None);

/// One test at a time has its own stand-in shell: the stand-in is the process's, and the tests of
/// the link and of the update's page both put one in its place.
#[cfg(all(windows, test))]
pub(crate) static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Opens `address` in the browser the human has set for web addresses.
#[cfg(windows)]
pub(crate) fn open(address: &str) -> Result<(), String> {
    open_with(execute, address)
}

/// The call to the shell: made by the app, and in a test build the stand-in's, or none. What it
/// is told when the call could not be made at all is its error.
#[cfg(windows)]
fn execute(call: &Call) -> Result<isize, String> {
    #[cfg(test)]
    {
        let stand_in = *TEST_SHELL.lock().expect("the stand-in shell");
        let Some(stand_in) = stand_in else {
            panic!("a test reached ShellExecuteW with no stand-in: it would open a page");
        };
        Ok(stand_in(call))
    }
    #[cfg(not(test))]
    native(call)
}

/// `ShellExecuteW`, with COM started on the thread as its documentation asks: the shell hands
/// the address to the program set for `http` through extensions that are COM objects, some of
/// which need a single-threaded apartment. A thread COM refuses to start on is an error said to
/// the human (`com_started`), and the shell is not asked on it.
#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
fn native(call: &Call) -> Result<isize, String> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let pointer = |text: &Option<Vec<u16>>| text.as_ref().map_or(null(), |text| text.as_ptr());
    with_com(
        // SAFETY: starts COM on this thread, in a single-threaded apartment; `with_com` ends a
        // start that succeeded (S_OK, or S_FALSE where the thread had it in that apartment
        // already) once, below, and a refusal (the thread is in another apartment, or COM could
        // not start) leaves nothing to end.
        || unsafe {
            CoInitializeEx(
                null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        },
        // SAFETY: ends the start made above, on the same thread.
        || unsafe { CoUninitialize() },
        // SAFETY: each string is NUL-ended UTF-16 owned by `call`, which outlives the call, or
        // null where it is `None`; no window owns the call.
        || unsafe {
            ShellExecuteW(
                null_mut(),
                call.verb.as_ptr(),
                call.file.as_ptr(),
                pointer(&call.parameters),
                pointer(&call.directory),
                SW_SHOWNORMAL,
            ) as isize
        },
    )
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
                Ok(answer)
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

    /// What COM's start on the thread answered decides whether the shell is asked and whether the
    /// start is ended after: both of its successes are ended once, a thread COM was started on in
    /// the other apartment model is asked on and not ended, and any other refusal is said and the
    /// shell is not asked.
    #[test]
    fn a_com_start_that_succeeded_is_ended_one_that_changed_mode_is_not_and_any_other_is_said() {
        assert_eq!(com_started(0), Ok(true), "S_OK");
        assert_eq!(com_started(1), Ok(true), "S_FALSE");
        assert_eq!(
            com_started(0x8001_0106_u32 as i32),
            Ok(false),
            "RPC_E_CHANGED_MODE"
        );
        for (code, said) in [
            (0x8007_000E_u32, "0x8007000E"),
            (0x8007_0057, "0x80070057"),
            (0x8000_FFFF, "0x8000FFFF"),
        ] {
            assert_eq!(
                com_started(code as i32),
                Err(format!(
                    "Windows could not start what opens web addresses (CoInitializeEx answered {said})"
                )),
                "{said}"
            );
        }
    }

    /// The shell is asked between COM's start and its end: a start that succeeded is ended once
    /// after the ask, one that changed mode is not ended, and a refusal asks nothing and ends nothing.
    #[test]
    fn the_shell_is_asked_between_com_s_start_and_its_end_and_a_refused_start_asks_nothing() {
        use std::cell::RefCell;

        let changed_mode = 0x8001_0106_u32 as i32;
        let out_of_memory = 0x8007_000E_u32 as i32;
        for (code, steps, asked) in [
            (0, vec!["start", "ask", "end"], true),
            (1, vec!["start", "ask", "end"], true),
            (changed_mode, vec!["start", "ask"], true),
            (out_of_memory, vec!["start"], false),
        ] {
            let log = RefCell::new(Vec::new());
            let answered = with_com(
                || {
                    log.borrow_mut().push("start");
                    code
                },
                || log.borrow_mut().push("end"),
                || {
                    log.borrow_mut().push("ask");
                    42
                },
            );
            assert_eq!(*log.borrow(), steps, "{code:#x}");
            assert_eq!(answered.is_ok(), asked, "{code:#x}");
            if asked {
                assert_eq!(answered, Ok(42), "{code:#x}");
            }
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
