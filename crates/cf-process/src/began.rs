//! Whether the process that holds a number now began after a moment: a number a file wrote
//! while its process ran may be another program's by the time it is read, and only when the
//! process holding it began tells the two apart. Windows gives a number back at once and to
//! any program, and a busy machine's next program soon has it (T-1226).

use std::time::SystemTime;

/// Whether the process with the number `pid` began after `time`, as the system says when it
/// began: false where it began by then, is gone, or the system will not say.
pub fn began_after(pid: u32, time: SystemTime) -> bool {
    began(pid).is_some_and(|began| began > time)
}

/// When the process `pid` began, as the system says (`Started`: to the microsecond).
#[cfg(unix)]
fn began(pid: u32) -> Option<SystemTime> {
    let micros = crate::started::began_at(&crate::Started::of(pid)?)?;
    SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_micros(micros))
}

/// When the process `pid` began, its creation time as `GetProcessTimes` says it (100 ns since
/// 1601).
#[cfg(windows)]
fn began(pid: u32) -> Option<SystemTime> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    #[allow(unsafe_code)]
    // SAFETY: OpenProcess takes plain values and returns a handle or null.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    #[allow(unsafe_code)]
    // SAFETY: the handle is open; each pointer is to a local FILETIME the call fills.
    let read =
        unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
    #[allow(unsafe_code)]
    // SAFETY: the handle is open, and closed once, here.
    unsafe {
        CloseHandle(handle);
    }
    if read == 0 {
        return None;
    }
    from_file_time(u64::from(created.dwHighDateTime) << 32 | u64::from(created.dwLowDateTime))
}

/// The time Windows' 100 ns since 1601 say.
#[cfg(any(windows, test))]
fn from_file_time(ticks: u64) -> Option<SystemTime> {
    const UNIX_FROM_1601: u64 = 11_644_473_600;
    let seconds = (ticks / 10_000_000).checked_sub(UNIX_FROM_1601)?;
    let nanos = u32::try_from((ticks % 10_000_000) * 100).ok()?;
    SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::new(seconds, nanos))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn this_process_began_before_now_and_after_a_moment_before_it() {
        let own = std::process::id();
        // 2096: long after it began.
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(4_000_000_000);
        assert!(!began_after(own, later));
        assert!(began_after(
            own,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1)
        ));
        assert!(!began_after(u32::MAX - 1, SystemTime::UNIX_EPOCH));
    }

    #[test]
    fn windows_time_is_read_from_1601() {
        assert_eq!(
            from_file_time(116_444_736_000_000_000),
            Some(SystemTime::UNIX_EPOCH)
        );
        assert_eq!(
            from_file_time(116_444_736_000_000_000 + 25_000_000),
            Some(SystemTime::UNIX_EPOCH + Duration::from_millis(2500))
        );
        assert_eq!(from_file_time(1), None);
    }
}
