//! How much memory this process holds: the resident set the daemon's stop and
//! alive lines report (Node's `process.memoryUsage().rss`).

/// The bytes of this process's memory that are in RAM now, as libuv's
/// `uv_resident_set_memory` reads them: the resident set on Linux and macOS,
/// the working set on Windows. None where the system will not say.
pub fn rss() -> Option<u64> {
    system::rss()
}

/// Whole megabytes, as `Math.round(rss / 1_048_576)` writes them in a line
/// of the log.
pub fn megabytes(bytes: u64) -> u64 {
    // Halves round up, as `Math.round` rounds them: the half-megabyte added
    // before the whole division.
    (bytes + 524_288) / 1_048_576
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod system {
    /// `VmRSS` of `/proc/self/status`, which the kernel counts in kilobytes
    /// of 1024 bytes.
    pub(super) fn rss() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        resident(&status)
    }

    pub(super) fn resident(status: &str) -> Option<u64> {
        let line = status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))?;
        let kilobytes: u64 = line.trim().strip_suffix("kB")?.trim().parse().ok()?;
        kilobytes.checked_mul(1024)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reads_the_resident_set_out_of_the_status_file() {
            let status = "Name:\tcf\nVmPeak:\t  900 kB\nVmRSS:\t  4096 kB\nThreads:\t3\n";
            assert_eq!(resident(status), Some(4_194_304));
            assert_eq!(resident("Name:\tcf\n"), None);
            assert_eq!(resident("VmRSS:\tmuch\n"), None);
        }
    }
}

#[cfg(target_os = "macos")]
mod system {
    /// What `proc_pidinfo` says of this process's task (`PROC_PIDTASKINFO`).
    pub(super) fn rss() -> Option<u64> {
        let pid = i32::try_from(std::process::id()).ok()?;
        let size = i32::try_from(std::mem::size_of::<libc::proc_taskinfo>()).ok()?;
        #[allow(unsafe_code)]
        // SAFETY: proc_taskinfo is plain integers, for which all zeroes is a
        // value.
        let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
        #[allow(unsafe_code)]
        // SAFETY: the buffer is `size` bytes of this process's own memory,
        // which proc_pidinfo fills and keeps no hold of; the pid is ours.
        let written = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTASKINFO,
                0,
                std::ptr::from_mut(&mut info).cast(),
                size,
            )
        };
        (written == size).then_some(info.pti_resident_size)
    }
}

#[cfg(windows)]
mod system {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    /// `WorkingSetSize` of this process's memory counters.
    pub(super) fn rss() -> Option<u64> {
        let mut counters = PROCESS_MEMORY_COUNTERS::default();
        let size = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
        counters.cb = size;
        #[allow(unsafe_code)]
        // SAFETY: the counters are this process's own memory, `cb` says how
        // big they are, and the pseudo handle of the current process needs no
        // closing.
        let read = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
        (read != 0).then(|| u64::try_from(counters.WorkingSetSize).unwrap_or(u64::MAX))
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    windows
)))]
mod system {
    pub(super) fn rss() -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_holds_some_memory_the_system_can_tell() {
        let held = rss().expect("the resident set of a running test");
        // A test process is more than a megabyte and less than a terabyte.
        assert!((1 << 20..1 << 40).contains(&held), "{held}");
    }

    #[test]
    fn megabytes_are_rounded_as_math_round_rounds_them() {
        assert_eq!(megabytes(0), 0);
        assert_eq!(megabytes(524_287), 0);
        assert_eq!(megabytes(524_288), 1);
        assert_eq!(megabytes(1_048_576), 1);
        assert_eq!(megabytes(1_572_863), 1);
        assert_eq!(megabytes(1_572_864), 2);
        assert_eq!(megabytes(150 * 1_048_576), 150);
    }
}
