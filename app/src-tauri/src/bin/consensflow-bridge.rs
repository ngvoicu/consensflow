use app_lib::runtime::run_headless;

fn main() {
    if !stdin_is_pipe() {
        return;
    }
    if let Err(error) = run_headless() {
        eprintln!("consensflow-bridge: {error}");
        std::process::exit(1);
    }
}

#[cfg(unix)]
fn stdin_is_pipe() -> bool {
    use std::os::fd::FromRawFd;
    use std::os::unix::fs::FileTypeExt;

    unsafe extern "C" {
        fn dup(file_descriptor: i32) -> i32;
    }

    // SAFETY: a successful dup returns a new descriptor owned by this call;
    // File closes that duplicate without affecting the process stdin.
    let file_descriptor = unsafe { dup(0) };
    if file_descriptor < 0 {
        return false;
    }
    let input = unsafe { std::fs::File::from_raw_fd(file_descriptor) };
    input.metadata().is_ok_and(|metadata| {
        let file_type = metadata.file_type();
        file_type.is_fifo() || file_type.is_socket()
    })
}

#[cfg(windows)]
fn stdin_is_pipe() -> bool {
    use std::os::windows::io::AsRawHandle;

    unsafe extern "system" {
        fn GetFileType(handle: *mut core::ffi::c_void) -> u32;
    }
    const FILE_TYPE_PIPE: u32 = 0x0003;

    // SAFETY: GetFileType only inspects the handle; the standard input handle
    // stays owned by the process.
    unsafe { GetFileType(std::io::stdin().as_raw_handle()) == FILE_TYPE_PIPE }
}

#[cfg(not(any(unix, windows)))]
fn stdin_is_pipe() -> bool {
    false
}
