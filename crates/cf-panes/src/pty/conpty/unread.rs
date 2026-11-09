//! How many bytes written to a pipe its reader has not read yet (T-1336).
//!
//! The console host reads a window's input 4,096 bytes at a time and takes an Escape that ends a read
//! for the Escape key, whatever follows, so an Escape-led key that straddles two reads reaches
//! the window as an Escape and what came after it (`pty/keys.rs`, `conpty`). A key written into a pipe
//! that holds nothing unread is read together, and this is how the pane host knows that.

use std::fs::File;
use std::io::{self, Write as _};
use std::mem::{size_of, zeroed};
use std::os::windows::io::AsRawHandle;
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Wdk::Storage::FileSystem::{
    FilePipeLocalInformation, NtQueryInformationFile, FILE_PIPE_LOCAL_INFORMATION,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use crate::pty::escapes::parts;

/// What the system says of the end `pipe` of a pipe (`FilePipeLocalInformation`).
pub(super) fn information(pipe: &File) -> io::Result<FILE_PIPE_LOCAL_INFORMATION> {
    // SAFETY: both are plain data the call fills in, and zero is a value of each.
    let (mut information, mut status): (FILE_PIPE_LOCAL_INFORMATION, IO_STATUS_BLOCK) =
        unsafe { (zeroed(), zeroed()) };
    // SAFETY: `pipe` is an open handle of this process, `information` is as large as the length
    // says, and `status` is a block of the call's own.
    let result = unsafe {
        NtQueryInformationFile(
            pipe.as_raw_handle(),
            &mut status,
            ptr::addr_of_mut!(information).cast(),
            u32::try_from(size_of::<FILE_PIPE_LOCAL_INFORMATION>()).unwrap_or(u32::MAX),
            FilePipeLocalInformation,
        )
    };
    if result < 0 {
        return Err(io::Error::other(format!(
            "NtQueryInformationFile of a pipe answered {result:#x}"
        )));
    }
    Ok(information)
}

/// How many bytes written to this end of a pipe its reader has not read: what the pipe's buffer
/// holds of the writer's, its quota less what is left of it.
pub(super) fn unread(pipe: &File) -> io::Result<usize> {
    let information = information(pipe)?;
    Ok(information
        .OutboundQuota
        .saturating_sub(information.WriteQuotaAvailable) as usize)
}

/// How long the wait of a key that begins with Escape is slept between looks at the pipe.
const LOOK: Duration = Duration::from_millis(1);

/// Waits until the reader of `pipe` has read everything written to it, and says how long that took. A
/// pipe that cannot be asked, or that is broken, is waited for no longer: the write that
/// follows says what is wrong with it.
pub(super) fn wait_until_read(pipe: &File) -> Duration {
    let started = Instant::now();
    while matches!(unread(pipe), Ok(held) if held > 0) {
        thread::sleep(LOOK);
    }
    started.elapsed()
}

/// Writes `bytes` to `pipe` so that the bytes of no key that begins with Escape can be read in
/// two (T-1336): each is written into a pipe that holds nothing unread, together, after the keys and
/// the text before it, in order and whole. The pipe's reader reads what it finds, and finds the
/// two bytes of Alt+Enter or the three of an arrow together. A pipe whose reader is not
/// reading holds the key back as it would have held it in the pipe, behind what is there: the
/// wait is as long as that, and returned.
pub(super) fn write_all_keeping_keys_whole(pipe: &mut File, bytes: &[u8]) -> io::Result<Duration> {
    let mut waited = Duration::ZERO;
    for part in parts(bytes) {
        if part.first() == Some(&0x1b) {
            waited += wait_until_read(pipe);
        }
        pipe.write_all(part)?;
    }
    Ok(waited)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;
    use crate::pty::conpty::pipe;

    /// Every field of what the system says of the write end of a pipe, for a failure's message.
    fn shown(pipe: &File) -> String {
        match information(pipe) {
            Ok(each) => format!(
                "type {} configuration {} instances {}/{} inbound {} readable {} outbound {} writable {} state {} end {}",
                each.NamedPipeType,
                each.NamedPipeConfiguration,
                each.CurrentInstances,
                each.MaximumInstances,
                each.InboundQuota,
                each.ReadDataAvailable,
                each.OutboundQuota,
                each.WriteQuotaAvailable,
                each.NamedPipeState,
                each.NamedPipeEnd,
            ),
            Err(error) => error.to_string(),
        }
    }

    /// The spike (T-1336): what the system reports of the write end of a pipe of the kind the
    /// console's input is (`pipe()`: the system's size, 4,096), empty, part full, full, and as the
    /// reader reads: the bytes the reader has not read, truly.
    #[test]
    fn the_write_end_of_a_pipe_says_how_many_bytes_the_reader_has_not_read() {
        let (mut read, mut write) = pipe().expect("a pipe");
        println!("empty: {}", shown(&write));
        assert_eq!(
            unread(&write).expect("empty"),
            0,
            "empty: {}",
            shown(&write)
        );
        let size = information(&write).expect("size").OutboundQuota as usize;
        assert!(size >= 512, "the pipe's size {size}: {}", shown(&write));

        write.write_all(&vec![b'x'; 1000]).expect("part full");
        println!("1000 written: {}", shown(&write));
        assert_eq!(unread(&write).expect("part"), 1000, "{}", shown(&write));

        write.write_all(&vec![b'x'; size - 1000]).expect("full");
        println!("full: {}", shown(&write));
        assert_eq!(unread(&write).expect("full"), size, "{}", shown(&write));

        let mut buffer = vec![0_u8; 1500];
        read.read_exact(&mut buffer).expect("the reader reads 1500");
        println!("1500 read: {}", shown(&write));
        assert_eq!(
            unread(&write).expect("read"),
            size - 1500,
            "{}",
            shown(&write)
        );

        let mut rest = vec![0_u8; size - 1500];
        read.read_exact(&mut rest)
            .expect("the reader reads the rest");
        println!("all read: {}", shown(&write));
        assert_eq!(unread(&write).expect("drained"), 0, "{}", shown(&write));
    }

    /// An Escape-led key written while nothing reads the pipe waits behind what is unread, as it
    /// would have waited in the pipe (T-1336): it is not written, nothing written after it passes
    /// it, no byte is lost, and another pane, with a pipe of its own, goes on at once. Once the
    /// reader reads, the key arrives together and what follows it after.
    #[test]
    fn an_escape_led_key_waits_for_a_pipe_nothing_reads_and_nothing_is_lost_or_held_elsewhere() {
        use std::sync::{mpsc, Arc, Mutex};

        let (mut read, write) = pipe().expect("a pane's input");
        let input = Arc::new(Mutex::new(write));
        // What is unread: text, which waits for nothing.
        let started = Instant::now();
        write_all_keeping_keys_whole(&mut input.lock().expect("the pane's writer"), &[b'x'; 100])
            .expect("text into the pipe");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(
            unread(&input.lock().expect("the writer")).expect("unread"),
            100
        );

        // The key waits, holding the pane's writer as a write does; what is written after it is
        // behind it.
        let (key_done, key_waited) = mpsc::channel();
        let (later_done, later_waited) = mpsc::channel();
        let keyed = Arc::clone(&input);
        let key = thread::spawn(move || {
            let started = Instant::now();
            let mut writer = keyed.lock().expect("the pane's writer");
            let result = write_all_keeping_keys_whole(&mut writer, b"\x1b\r");
            let _ = key_done.send((result.is_ok(), started.elapsed()));
        });
        thread::sleep(Duration::from_millis(200));
        let after = Arc::clone(&input);
        let later = thread::spawn(move || {
            let mut writer = after.lock().expect("the pane's writer");
            let result = write_all_keeping_keys_whole(&mut writer, b"z");
            let _ = later_done.send(result.is_ok());
        });
        thread::sleep(Duration::from_millis(800));
        assert!(
            key_waited.try_recv().is_err(),
            "the key was written into a pipe that holds unread bytes"
        );
        assert!(
            later_waited.try_recv().is_err(),
            "a key written after it passed it"
        );

        // Another pane goes on: its arrow is written and is there to be read at once.
        let (mut other_read, mut other_write) = pipe().expect("another pane's input");
        let started = Instant::now();
        write_all_keeping_keys_whole(&mut other_write, b"\x1b[A").expect("another pane's arrow");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        let mut arrow = [0_u8; 8];
        let count = other_read
            .read(&mut arrow)
            .expect("another pane's arrow is there");
        assert_eq!(&arrow[..count], b"\x1b[A");

        // The reader reads what is there: the key, and the text after it, come in order.
        let mut text = [0_u8; 100];
        read.read_exact(&mut text).expect("the 100 bytes of text");
        assert_eq!(text, [b'x'; 100]);
        let (written, waited) = key_waited
            .recv_timeout(Duration::from_secs(10))
            .expect("the key is written once the pipe is read");
        assert!(written, "the key's write failed");
        let mut rest = [0_u8; 8];
        let count = read.read(&mut rest).expect("the key");
        assert_eq!(
            &rest[..count],
            b"\x1b\r",
            "the two bytes of the key come in one read"
        );
        assert!(later_waited
            .recv_timeout(Duration::from_secs(10))
            .expect("the later key"));
        let count = read.read(&mut rest).expect("the text after it");
        assert_eq!(&rest[..count], b"z");
        key.join().expect("the key's thread");
        later.join().expect("the later thread");
        println!("the longest wait: {waited:?} (nothing read the pipe for about a second)");
        assert!(
            waited >= Duration::from_millis(700),
            "the key did not wait: {waited:?}"
        );
    }

    /// A pipe whose reader has gone: the answer is an error or nothing unread, never a hang.
    #[test]
    fn the_write_end_of_a_pipe_whose_reader_has_gone_does_not_hang() {
        let (read, write) = pipe().expect("a pipe");
        drop(read);
        println!("reader gone: {}", shown(&write));
        let _ = unread(&write);
    }
}
