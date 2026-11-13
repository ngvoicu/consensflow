//! The transport: the handshake, the frame reader and its dispatch.

use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, RawFd};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::thread;

use cf_proto::bridge::Frame;
use serde_json::Value;

use crate::shutdown::TransportShutdown;
use crate::{Bridge, BridgeBuilder, BridgeError, BridgeInner, ConnectedBridge};

impl BridgeBuilder {
    #[cfg(unix)]
    pub fn connect<R, W>(self, input: R, output: W) -> Result<ConnectedBridge, BridgeError>
    where
        R: Read + Send + AsRawFd + 'static,
        W: Write + Send + AsRawFd + 'static,
    {
        set_nonblocking(input.as_raw_fd())?;
        set_nonblocking(output.as_raw_fd())?;
        let shutdown = Arc::new(TransportShutdown::new());
        self.connect_inner(
            InterruptibleReader::new(input, Arc::clone(&shutdown)),
            InterruptibleWriter::new(output, Arc::clone(&shutdown)),
            shutdown,
        )
    }

    #[cfg(not(unix))]
    pub fn connect<R, W>(self, input: R, output: W) -> Result<ConnectedBridge, BridgeError>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        self.connect_inner(input, output, Arc::new(TransportShutdown::new()))
    }

    #[cfg(test)]
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(super) fn connect_uninterruptible<R, W>(
        self,
        input: R,
        output: W,
    ) -> Result<ConnectedBridge, BridgeError>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        self.connect_inner(input, output, Arc::new(TransportShutdown::new()))
    }

    fn connect_inner<R, W>(
        self,
        input: R,
        output: W,
        shutdown: Arc<TransportShutdown>,
    ) -> Result<ConnectedBridge, BridgeError>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        let mut reader = BufReader::new(input);
        let mut discarding = false;
        let handle_line =
            match read_bounded_line(&mut reader, self.max_frame_bytes, &mut discarding)? {
                BoundedLine::Complete(line) => line,
                BoundedLine::Eof => return Err(BridgeError::Eof),
                BoundedLine::Unterminated => {
                    return Err(BridgeError::InvalidHandle(
                        "handle line is missing its newline".to_string(),
                    ));
                }
                BoundedLine::Overflow => {
                    return Err(BridgeError::InvalidHandle(format!(
                        "handle exceeds maxFrameBytes ({})",
                        self.max_frame_bytes
                    )));
                }
            };
        let handle = serde_json::from_slice(&handle_line)
            .map_err(|error| BridgeError::InvalidHandle(error.to_string()))?;
        let bridge = self.build(output, shutdown);
        bridge.start_reader(reader);
        Ok(ConnectedBridge { handle, bridge })
    }

    #[cfg(unix)]
    pub fn serve<R, W>(self, input: R, output: W, handle: &Value) -> Result<Bridge, BridgeError>
    where
        R: Read + Send + AsRawFd + 'static,
        W: Write + Send + AsRawFd + 'static,
    {
        set_nonblocking(input.as_raw_fd())?;
        set_nonblocking(output.as_raw_fd())?;
        let shutdown = Arc::new(TransportShutdown::new());
        let input = InterruptibleReader::new(input, Arc::clone(&shutdown));
        let output = InterruptibleWriter::new(output, Arc::clone(&shutdown));
        self.serve_inner(input, output, handle, shutdown)
    }

    #[cfg(not(unix))]
    pub fn serve<R, W>(self, input: R, output: W, handle: &Value) -> Result<Bridge, BridgeError>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        self.serve_inner(input, output, handle, Arc::new(TransportShutdown::new()))
    }

    fn serve_inner<R, W>(
        self,
        input: R,
        mut output: W,
        handle: &Value,
        shutdown: Arc<TransportShutdown>,
    ) -> Result<Bridge, BridgeError>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        serde_json::to_writer(&mut output, handle)
            .map_err(|error| BridgeError::InvalidHandle(error.to_string()))?;
        output.write_all(b"\n")?;
        output.flush()?;
        let bridge = self.build(output, shutdown);
        bridge.start_reader(BufReader::new(input));
        Ok(bridge)
    }
}

impl Bridge {
    fn start_reader<R>(&self, reader: BufReader<R>)
    where
        R: Read + Send + 'static,
    {
        self.begin_transport_worker();
        let inner = Arc::downgrade(&self.inner);
        let max_frame_bytes = self.inner.max_frame_bytes;
        let worker = TransportWorker::new(Weak::clone(&inner));
        thread::spawn(move || {
            let _worker = worker;
            let mut reader = reader;
            let mut discarding = false;
            loop {
                let Some(current) = inner.upgrade() else {
                    return;
                };
                if current.closed.load(Ordering::Acquire) {
                    return;
                }
                drop(current);
                let outcome = read_bounded_line(&mut reader, max_frame_bytes, &mut discarding);
                let Some(current) = inner.upgrade() else {
                    return;
                };
                let bridge = Bridge { inner: current };
                match outcome {
                    Ok(BoundedLine::Eof) => {
                        bridge.close_with_eof();
                        return;
                    }
                    Ok(BoundedLine::Complete(line)) => {
                        if !line.is_empty() {
                            bridge.dispatch_line(&line);
                        }
                    }
                    Ok(BoundedLine::Overflow) => {
                        bridge.report(BridgeError::MalformedFrame(
                            "frame exceeds maxFrameBytes".to_string(),
                        ));
                    }
                    Ok(BoundedLine::Unterminated) => {
                        bridge.report(BridgeError::MalformedFrame(
                            "unterminated frame at EOF".to_string(),
                        ));
                        bridge.close_with_eof();
                        return;
                    }
                    Err(error) => {
                        bridge.report(error.into());
                        bridge.close_with_eof();
                        return;
                    }
                }
            }
        });
    }

    fn begin_transport_worker(&self) {
        let mut lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.transport_workers += 1;
    }

    fn dispatch_line(&self, raw: &[u8]) {
        if self.is_closed() {
            return;
        }
        if raw.len() > self.inner.max_frame_bytes {
            self.report(BridgeError::MalformedFrame(
                "frame exceeds maxFrameBytes".to_string(),
            ));
            return;
        }
        let frame = match serde_json::from_slice::<Frame>(raw) {
            Ok(frame) if frame.is_well_formed() => frame,
            Ok(_) => {
                self.report(BridgeError::MalformedFrame(
                    String::from_utf8_lossy(raw).into_owned(),
                ));
                return;
            }
            Err(error) => {
                self.report(BridgeError::MalformedFrame(error.to_string()));
                return;
            }
        };

        let valid_namespace = match frame.kind.as_str() {
            "res" => frame.id.starts_with(self.inner.role.prefix()),
            "req" | "evt" => frame.id.starts_with(self.inner.role.peer().prefix()),
            _ => false,
        };
        if !valid_namespace {
            self.report(BridgeError::MalformedFrame(format!(
                "id {} has the wrong namespace for {}",
                frame.id, frame.kind
            )));
            return;
        }

        match frame.kind.as_str() {
            "req" => self.dispatch_request(frame),
            "res" => self.dispatch_response(frame),
            "evt" => self.dispatch_event(frame),
            _ => unreachable!("validated frame kind"),
        }
    }
}

pub(crate) struct TransportWorker {
    inner: Weak<BridgeInner>,
}

impl TransportWorker {
    pub(crate) fn new(inner: Weak<BridgeInner>) -> Self {
        Self { inner }
    }
}

impl Drop for TransportWorker {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let mut lifecycle = inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.transport_workers = lifecycle.transport_workers.saturating_sub(1);
        if lifecycle.closed {
            inner.closed_ready.notify_all();
        }
    }
}

#[cfg(unix)]
struct InterruptibleReader<R> {
    inner: R,
    shutdown: Arc<TransportShutdown>,
}

#[cfg(unix)]
impl<R> InterruptibleReader<R> {
    fn new(inner: R, shutdown: Arc<TransportShutdown>) -> Self {
        Self { inner, shutdown }
    }
}

#[cfg(unix)]
impl<R: Read> Read for InterruptibleReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.shutdown.is_requested() {
                return Ok(0);
            }
            match self.inner.read(bytes) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    self.shutdown.wait_for_retry();
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
}

#[cfg(unix)]
struct InterruptibleWriter<W> {
    inner: W,
    shutdown: Arc<TransportShutdown>,
}

#[cfg(unix)]
impl<W> InterruptibleWriter<W> {
    fn new(inner: W, shutdown: Arc<TransportShutdown>) -> Self {
        Self { inner, shutdown }
    }
}

#[cfg(unix)]
impl<W: Write> Write for InterruptibleWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        loop {
            if self.shutdown.is_requested() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "bridge transport is closed",
                ));
            }
            match self.inner.write(bytes) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    self.shutdown.wait_for_retry();
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        loop {
            if self.shutdown.is_requested() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "bridge transport is closed",
                ));
            }
            match self.inner.flush() {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    self.shutdown.wait_for_retry();
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn set_nonblocking(file_descriptor: RawFd) -> std::io::Result<()> {
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_NONBLOCK: i32 = 0x800;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const O_NONBLOCK: i32 = 0x0004;

    unsafe extern "C" {
        fn fcntl(file_descriptor: i32, command: i32, ...) -> i32;
    }

    // SAFETY: fcntl only inspects or updates flags for the valid descriptor
    // borrowed from the caller; ownership is unchanged.
    let flags = unsafe { fcntl(file_descriptor, F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: as above, with the flags just read and O_NONBLOCK added.
    if unsafe { fcntl(file_descriptor, F_SETFL, flags | O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

enum BoundedLine {
    Complete(Vec<u8>),
    Overflow,
    Unterminated,
    Eof,
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
    discarding: &mut bool,
) -> std::io::Result<BoundedLine> {
    if *discarding {
        loop {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return Ok(BoundedLine::Eof);
            }
            let newline = available.iter().position(|byte| *byte == b'\n');
            let consumed = newline.map_or(available.len(), |index| index + 1);
            reader.consume(consumed);
            if newline.is_some() {
                *discarding = false;
                break;
            }
        }
    }

    let mut line = Vec::with_capacity(max_bytes.min(8 * 1024));
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(BoundedLine::Eof)
            } else {
                Ok(BoundedLine::Unterminated)
            };
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            if line.len().saturating_add(newline) > max_bytes {
                reader.consume(newline + 1);
                return Ok(BoundedLine::Overflow);
            }
            line.extend_from_slice(&available[..newline]);
            reader.consume(newline + 1);
            trim_line_ending(&mut line);
            return Ok(BoundedLine::Complete(line));
        }

        let available_len = available.len();
        if line.len().saturating_add(available_len) > max_bytes {
            reader.consume(available_len);
            *discarding = true;
            return Ok(BoundedLine::Overflow);
        }
        line.extend_from_slice(available);
        reader.consume(available_len);
    }
}

fn trim_line_ending(line: &mut Vec<u8>) {
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
}
