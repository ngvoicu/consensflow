//! What libuv calls a failed system call and the words it has for it, as
//! Node v26.8.1 (libuv 1.52.1) says them: `error.code` (`EACCES`) and the
//! words after it in `error.message` (`permission denied`). No other table
//! in ConsensFlow names a failure. The tables ported here:
//!
//! - the words: `UV_ERRNO_MAP` (`include/uv.h`), each error's name and what
//!   `uv_strerror` says of it, the same on every system. Node prints the
//!   same table through `util.getSystemErrorMap()`, and a golden holds this
//!   copy to it (`tests/errno.rs`);
//! - on Unix: the errno behind each name of that map, as `include/uv/errno.h`
//!   defines `UV__EACCES` and the others from the system's own constants. A
//!   name the system's libc lacks (`ENONET` on macOS) is left out, as libuv
//!   gives it a number of its own there that no system call returns;
//! - on Windows: `uv_translate_sys_error` (`src/win/error.c`), which turns the
//!   system's code into one of those names.

use std::io;

#[cfg(windows)]
mod windows;

/// libuv's `UV_UNKNOWN`, which is Node's `uvUnmappedError`: the name and the
/// words of a failure libuv has no name for.
const UNKNOWN: (&str, &str) = ("UNKNOWN", "unknown error");

/// libuv 1.52.1's `UV_ERRNO_MAP` (`include/uv.h`) in its order: the name of
/// each error and its words (`uv_strerror`).
const WORDS: [(&str, &str); 85] = [
    ("E2BIG", "argument list too long"),
    ("EACCES", "permission denied"),
    ("EADDRINUSE", "address already in use"),
    ("EADDRNOTAVAIL", "address not available"),
    ("EAFNOSUPPORT", "address family not supported"),
    ("EAGAIN", "resource temporarily unavailable"),
    ("EAI_ADDRFAMILY", "address family not supported"),
    ("EAI_AGAIN", "temporary failure"),
    ("EAI_BADFLAGS", "bad ai_flags value"),
    ("EAI_BADHINTS", "invalid value for hints"),
    ("EAI_CANCELED", "request canceled"),
    ("EAI_FAIL", "permanent failure"),
    ("EAI_FAMILY", "ai_family not supported"),
    ("EAI_MEMORY", "out of memory"),
    ("EAI_NODATA", "no address"),
    ("EAI_NONAME", "unknown node or service"),
    ("EAI_OVERFLOW", "argument buffer overflow"),
    ("EAI_PROTOCOL", "resolved protocol is unknown"),
    ("EAI_SERVICE", "service not available for socket type"),
    ("EAI_SOCKTYPE", "socket type not supported"),
    ("EALREADY", "connection already in progress"),
    ("EBADF", "bad file descriptor"),
    ("EBUSY", "resource busy or locked"),
    ("ECANCELED", "operation canceled"),
    ("ECHARSET", "invalid Unicode character"),
    ("ECONNABORTED", "software caused connection abort"),
    ("ECONNREFUSED", "connection refused"),
    ("ECONNRESET", "connection reset by peer"),
    ("EDESTADDRREQ", "destination address required"),
    ("EEXIST", "file already exists"),
    ("EFAULT", "bad address in system call argument"),
    ("EFBIG", "file too large"),
    ("EHOSTUNREACH", "host is unreachable"),
    ("EINTR", "interrupted system call"),
    ("EINVAL", "invalid argument"),
    ("EIO", "i/o error"),
    ("EISCONN", "socket is already connected"),
    ("EISDIR", "illegal operation on a directory"),
    ("ELOOP", "too many symbolic links encountered"),
    ("EMFILE", "too many open files"),
    ("EMSGSIZE", "message too long"),
    ("ENAMETOOLONG", "name too long"),
    ("ENETDOWN", "network is down"),
    ("ENETUNREACH", "network is unreachable"),
    ("ENFILE", "file table overflow"),
    ("ENOBUFS", "no buffer space available"),
    ("ENODEV", "no such device"),
    ("ENOENT", "no such file or directory"),
    ("ENOMEM", "not enough memory"),
    ("ENONET", "machine is not on the network"),
    ("ENOPROTOOPT", "protocol not available"),
    ("ENOSPC", "no space left on device"),
    ("ENOSYS", "function not implemented"),
    ("ENOTCONN", "socket is not connected"),
    ("ENOTDIR", "not a directory"),
    ("ENOTEMPTY", "directory not empty"),
    ("ENOTSOCK", "socket operation on non-socket"),
    ("ENOTSUP", "operation not supported on socket"),
    ("EOVERFLOW", "value too large for defined data type"),
    ("EPERM", "operation not permitted"),
    ("EPIPE", "broken pipe"),
    ("EPROTO", "protocol error"),
    ("EPROTONOSUPPORT", "protocol not supported"),
    ("EPROTOTYPE", "protocol wrong type for socket"),
    ("ERANGE", "result too large"),
    ("EROFS", "read-only file system"),
    ("ESHUTDOWN", "cannot send after transport endpoint shutdown"),
    ("ESPIPE", "invalid seek"),
    ("ESRCH", "no such process"),
    ("ETIMEDOUT", "connection timed out"),
    ("ETXTBSY", "text file is busy"),
    ("EXDEV", "cross-device link not permitted"),
    UNKNOWN,
    ("EOF", "end of file"),
    ("ENXIO", "no such device or address"),
    ("EMLINK", "too many links"),
    ("EHOSTDOWN", "host is down"),
    ("EREMOTEIO", "remote I/O error"),
    ("ENOTTY", "inappropriate ioctl for device"),
    ("EFTYPE", "inappropriate file type or format"),
    ("EILSEQ", "illegal byte sequence"),
    ("ESOCKTNOSUPPORT", "socket type not supported"),
    ("ENODATA", "no data available"),
    ("EUNATCH", "protocol driver not attached"),
    ("ENOEXEC", "exec format error"),
];

/// libuv's words for the error `name` (`uv_strerror`): `permission denied`
/// for `EACCES`. None for a name `UV_ERRNO_MAP` does not hold.
pub fn uv_words(name: &str) -> Option<&'static str> {
    WORDS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, words)| *words)
}

/// [`uv_words`], and `unknown error` for a name libuv does not hold, as
/// `uvException` finds them: `uvErrmapGet(...) || uvUnmappedError`.
pub(super) fn words_of(name: &str) -> &'static str {
    uv_words(name).unwrap_or(UNKNOWN.1)
}

/// `(libc::NAME, "NAME")` for each name given, so that a name and its
/// constant cannot part.
#[cfg(unix)]
macro_rules! errnos {
    ($($(#[$platform:meta])* $name:ident,)*) => {
        &[$($(#[$platform])* (libc::$name, stringify!($name)),)*]
    };
}

/// The errno behind each name of `UV_ERRNO_MAP` that has one, in the order
/// of the map. libuv does not name the others (`ESTALE`, `EDQUOT`): Node
/// calls those `UNKNOWN`.
#[cfg(unix)]
const SYSTEM: &[(i32, &str)] = errnos![
    E2BIG,
    EACCES,
    EADDRINUSE,
    EADDRNOTAVAIL,
    EAFNOSUPPORT,
    EAGAIN,
    EALREADY,
    EBADF,
    EBUSY,
    ECANCELED,
    ECONNABORTED,
    ECONNREFUSED,
    ECONNRESET,
    EDESTADDRREQ,
    EEXIST,
    EFAULT,
    EFBIG,
    EHOSTUNREACH,
    EINTR,
    EINVAL,
    EIO,
    EISCONN,
    EISDIR,
    ELOOP,
    EMFILE,
    EMSGSIZE,
    ENAMETOOLONG,
    ENETDOWN,
    ENETUNREACH,
    ENFILE,
    ENOBUFS,
    ENODEV,
    ENOENT,
    ENOMEM,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    ENONET,
    ENOPROTOOPT,
    ENOSPC,
    ENOSYS,
    ENOTCONN,
    ENOTDIR,
    ENOTEMPTY,
    ENOTSOCK,
    ENOTSUP,
    EOVERFLOW,
    EPERM,
    EPIPE,
    EPROTO,
    EPROTONOSUPPORT,
    EPROTOTYPE,
    ERANGE,
    EROFS,
    ESHUTDOWN,
    ESPIPE,
    ESRCH,
    ETIMEDOUT,
    ETXTBSY,
    EXDEV,
    ENXIO,
    EMLINK,
    EHOSTDOWN,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    EREMOTEIO,
    ENOTTY,
    #[cfg(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    EFTYPE,
    EILSEQ,
    ESOCKTNOSUPPORT,
    #[cfg(any(
        target_vendor = "apple",
        target_os = "linux",
        target_os = "android",
        target_os = "netbsd"
    ))]
    ENODATA,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    EUNATCH,
    ENOEXEC,
];

/// What libuv calls the failure behind `error`, which Node prints as
/// `error.code` (`ENOENT`). On Unix the name of the errno; on Windows the
/// name libuv translates the system's code to, so access denied is `EPERM`
/// and a name no file can have is `ENOENT`, as Node says. None for a
/// failure libuv has no name for, which Node calls `UNKNOWN`
/// ([`error_code`]), and for one that is no system's.
pub fn errno_name(error: &io::Error) -> Option<&'static str> {
    system_name(error.raw_os_error()?)
}

/// Node's `error.code` for `error`: [`errno_name`], and `UNKNOWN` for the
/// failure libuv has no name for, as `uvException` says it.
pub fn error_code(error: &io::Error) -> &'static str {
    errno_name(error).unwrap_or(UNKNOWN.0)
}

/// Whether `error` says there is no such file, as Node's `ENOENT` does.
pub fn is_missing(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || errno_name(error) == Some("ENOENT")
}

#[cfg(unix)]
fn system_name(code: i32) -> Option<&'static str> {
    SYSTEM
        .iter()
        .find(|(errno, _)| *errno == code)
        .map(|(_, name)| *name)
}

#[cfg(windows)]
fn system_name(code: i32) -> Option<&'static str> {
    windows::name(code)
}

#[cfg(test)]
mod tests;
