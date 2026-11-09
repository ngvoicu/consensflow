//! How many bytes written to a pipe its reader has not read yet (T-1336).
//!
//! The console host reads a window's input 4,096 bytes at a time and takes an Escape that ends a read
//! for the Escape key, whatever follows, so an Escape-led key that straddles two reads reaches
//! the window as an Escape and what came after it (`pty/keys.rs`, `conpty`). A key written into a pipe
//! that holds nothing unread is read together, and this is how the pane host knows that.

use std::fs::File;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::AsRawHandle;
use std::ptr;

use windows_sys::Wdk::Storage::FileSystem::{
    FilePipeLocalInformation, NtQueryInformationFile, FILE_PIPE_LOCAL_INFORMATION,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

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
        assert_eq!(unread(&write).expect("empty"), 0, "empty: {}", shown(&write));
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
        assert_eq!(unread(&write).expect("read"), size - 1500, "{}", shown(&write));

        let mut rest = vec![0_u8; size - 1500];
        read.read_exact(&mut rest).expect("the reader reads the rest");
        println!("all read: {}", shown(&write));
        assert_eq!(unread(&write).expect("drained"), 0, "{}", shown(&write));
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
