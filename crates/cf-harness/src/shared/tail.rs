//! A record read from its end, within a bound (CHF-05): what a reopen asks of a
//! conversation is near its end, and a Claude transcript can be 300 MB, which a whole read
//! takes a second over. Its lines are given from the last back, block by block, until the
//! bound is read. A reader looks for the words it needs in a line before it parses it: a
//! tool's output in a line can be megabytes of JSON, and parsing every line of 16 MB took
//! half a second where looking first took 8 ms (`tools/probes/chief-harness/README.md`).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::ControlFlow;
use std::path::Path;

/// How much of a record's end a reopen reads: past it, the record is taken to say
/// nothing, and the reopen launches on the model picked for the chief.
pub(crate) const TAIL_BOUND: u64 = 16 * 1024 * 1024;

/// How much each read takes.
const BLOCK: u64 = 64 * 1024;

/// Gives `visit` each line of `file` from its last back, within its last `bound` bytes,
/// until it breaks: a line the bound cuts is not given. A line that is no UTF-8 is not
/// given; nor is the empty one after the last line break.
pub(crate) fn lines_back(
    file: &Path,
    bound: u64,
    mut visit: impl FnMut(&str) -> ControlFlow<()>,
) -> std::io::Result<()> {
    let mut opened = File::open(file)?;
    let size = opened.metadata()?.len();
    let floor = size.saturating_sub(bound);
    let mut at = size;
    // What the block read last began with, up to its first line break: the end of a
    // line whose beginning is in a block before it.
    let mut carried: Vec<u8> = Vec::new();
    while at > floor {
        let start = at.saturating_sub(BLOCK).max(floor);
        let mut block = vec![0; usize::try_from(at - start).unwrap_or(0)];
        opened.seek(SeekFrom::Start(start))?;
        opened.read_exact(&mut block)?;
        block.extend_from_slice(&carried);
        at = start;
        let first_break = block.iter().position(|byte| *byte == b'\n');
        let whole_from = match first_break {
            Some(found) => found + 1,
            // No break in the block: the line goes on before it.
            None if at > 0 => {
                carried = block;
                continue;
            }
            None => 0,
        };
        let (head, whole) = block.split_at(whole_from);
        for line in whole.rsplit(|byte| *byte == b'\n') {
            if give(line, &mut visit).is_break() {
                return Ok(());
            }
        }
        carried = head.strip_suffix(b"\n").unwrap_or(head).to_vec();
        if at == 0 && first_break.is_some() {
            // The file's first line, which no break begins.
            let _ = give(&carried, &mut visit);
            return Ok(());
        }
    }
    // The line the bound begins in is whole where a break ends the byte before it.
    if floor > 0 && !carried.is_empty() {
        let mut before = [0];
        opened.seek(SeekFrom::Start(floor - 1))?;
        opened.read_exact(&mut before)?;
        if before[0] == b'\n' {
            let _ = give(&carried, &mut visit);
        }
    }
    Ok(())
}

fn give(line: &[u8], visit: &mut impl FnMut(&str) -> ControlFlow<()>) -> ControlFlow<()> {
    match std::str::from_utf8(line) {
        Ok(line) if !line.is_empty() => visit(line),
        _ => ControlFlow::Continue(()),
    }
}

#[cfg(test)]
mod tests;
