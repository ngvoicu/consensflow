//! A window's pseudoconsole on Windows, and its program, started in the window's job.
//!
//! portable-pty starts a program as it is, outside any job, and keeps its pseudoconsole to
//! itself: the window's job could only be joined once the program ran, and a launcher (a
//! `.cmd` shim, an npm wrapper) could start a child in that moment, which the job never
//! held: its inventories did not list it and the close did not end it (Microsoft's rule: a
//! process that joins a job brings none of the children it already has). Here the program
//! is started with the job in its attribute list (`PROC_THREAD_ATTRIBUTE_JOB_LIST`): the
//! system puts it in the job as it makes it, before its first instruction, and whatever it
//! starts is the job's from the first. A job that cannot take it fails the start, and the
//! window does not open.
//!
//! The rest is portable-pty 0.9's recipe, kept as it was, so that a window behaves as it
//! did: the console host is the `conpty.dll` Windows' search for libraries finds (the one
//! the app ships, beside its `OpenConsole.exe`: `find_libraries_in` in
//! `app/src-tauri/src/portable.rs`), or else the system's own; the console is made with the
//! flags portable-pty gives it; the program has none of the host's handles (its standard
//! ones are none, and nothing is inherited); its environment is the command's whole, each
//! value as the system keeps it, Unicode or not ([`environment`]); its folder the command's (or the user's,
//! where that is no folder), and its command line is quoted as portable-pty quotes it.
//!
//! **The close.** Before Windows 11 24H2 a console that is closed waits for its host, and
//! the host waits to write what the program printed: a close while the output is held open
//! and unread waited for ever. So what is read of the output goes before the console does:
//! a window that fails as it opens lets go of its reader before it is ended (`pty`), and the
//! master closes its ends of the pipes before the console ([`Master`]); a window that opened
//! has its output read to its end, whatever its close (`stream_output`). And before 24H2 a
//! console made to take its cursor from its terminal (`PSEUDOCONSOLE_INHERIT_CURSOR`) asks
//! where the cursor is as it starts and reads its input until it is told: a window closed
//! before its terminal answered waited for ever. A pane is a new window, whose cursor starts
//! at the top left, so its console is not made to ask; and a pane lets go of its input
//! before its console is closed (`pty`'s `Pane`), so a console that waits on its input is
//! told it has ended.
//!
//! What is made answers portable-pty's own traits, so a pane holds its master and its
//! child as it does on Unix.

use std::ffi::{c_void, OsStr, OsString};
use std::fs::File;
use std::io;
use std::marker::PhantomData;
use std::mem::{size_of, size_of_val};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use portable_pty::{
    Child, ChildKiller, CommandBuilder, ExitStatus, MasterPty, PtyPair, PtySize, SlavePty,
};
use windows_sys::core::HRESULT;
use windows_sys::Win32::Foundation::{
    FreeLibrary, HANDLE, INVALID_HANDLE_VALUE, S_OK, WAIT_FAILED, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Console::{COORD, HPCON};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, INFINITE,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_JOB_LIST,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

use crate::job_object::JobObject;
use environment::environment_block;

mod environment;

/// The two flags portable-pty gives a console besides the cursor it has it inherit, which
/// Windows' headers do not name: a resize that does not redraw what scrolled away, and
/// the keys passed on as Windows' input records, which the window's input is written for.
/// The cursor is not inherited here (the module's **The close**).
const PSEUDOCONSOLE_RESIZE_QUIRK: u32 = 0x2;
const PSEUDOCONSOLE_WIN32_INPUT_MODE: u32 = 0x4;

/// The pseudoconsoles of a window: its program is started in `job`.
pub(super) struct ConPty {
    job: Arc<JobObject>,
}

impl ConPty {
    pub(super) fn joining(job: Arc<JobObject>) -> Self {
        Self { job }
    }

    /// A pseudoconsole of `size`: its master, which the window reads and writes and
    /// resizes, and the side its program is started on, which is let go once it has been.
    pub(super) fn openpty(&self, size: PtySize) -> io::Result<PtyPair> {
        let host = Host::get()?;
        // What the window types, which the console reads; and what the console writes.
        let (input_read, input_write) = pipe()?;
        let (output_read, output_write) = pipe()?;
        let console = Arc::new(Console::new(host, size, &input_read, &output_write)?);
        // The console holds the ends it was given, as copies of its own: these go, so
        // that the output ends when the console host has gone.
        drop((input_read, output_write));
        Ok(PtyPair {
            slave: Box::new(Slave {
                console: Arc::clone(&console),
                job: Arc::clone(&self.job),
            }),
            master: Box::new(Master {
                output: Output::new(output_read, &console),
                input: Mutex::new(Some(Input::new(input_write, &console))),
                size: Mutex::new(size),
                console,
            }),
        })
    }
}

type Create = unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut HPCON) -> HRESULT;
type Resize = unsafe extern "system" fn(HPCON, COORD) -> HRESULT;
type Close = unsafe extern "system" fn(HPCON);

/// The console host's three functions, from the library that has them.
struct Host {
    create: Create,
    resize: Resize,
    close: Close,
}

impl Host {
    /// The `conpty.dll` Windows finds where it looks for libraries (the app names the
    /// folder its own is in), or else the system's, which `kernel32.dll` has since
    /// Windows 10 1809. Loaded once, for the life of the app.
    fn get() -> io::Result<&'static Self> {
        static HOST: OnceLock<Option<Host>> = OnceLock::new();
        HOST.get_or_init(|| Self::load("conpty.dll").or_else(|| Self::load("kernel32.dll")))
            .as_ref()
            .ok_or_else(|| {
                io::Error::other("this Windows has no pseudoconsole: Windows 10 1809 or later")
            })
    }

    /// The three functions of `library`, which stays loaded for the life of the process
    /// once they are taken from it; one that lacks any of them is let go.
    fn load(library: &str) -> Option<Self> {
        let name = wide(OsStr::new(library));
        // SAFETY: a NUL-terminated name.
        let module = unsafe { LoadLibraryW(name.as_ptr()) };
        if module.is_null() {
            return None;
        }
        let function = |name: &std::ffi::CStr| {
            // SAFETY: the module is loaded, and the name is NUL-terminated.
            unsafe { GetProcAddress(module, name.as_ptr().cast()) }
        };
        let (Some(create), Some(resize), Some(close)) = (
            function(c"CreatePseudoConsole"),
            function(c"ResizePseudoConsole"),
            function(c"ClosePseudoConsole"),
        ) else {
            // SAFETY: loaded above, and nothing taken from it is kept.
            unsafe { FreeLibrary(module) };
            return None;
        };
        // SAFETY: each is the function Windows declares under its name, whose signature
        // is the one its type says (`consoleapi.h`); a pointer to a function is one size.
        unsafe {
            Some(Self {
                create: std::mem::transmute::<unsafe extern "system" fn() -> isize, Create>(create),
                resize: std::mem::transmute::<unsafe extern "system" fn() -> isize, Resize>(resize),
                close: std::mem::transmute::<unsafe extern "system" fn() -> isize, Close>(close),
            })
        }
    }
}

/// The file the console host of the windows was loaded from: the `conpty.dll` Windows
/// found, which stays loaded once it is the host ([`Host::load`]), or else the system's
/// own, in `kernel32.dll`. What a test of a key is of: each host makes its own keys of
/// the same bytes.
#[cfg(test)]
pub(crate) fn host_library() -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};

    Host::get()?;
    let named = |library: &str| {
        let name = wide(OsStr::new(library));
        // SAFETY: a NUL-terminated name; the call only asks.
        let module = unsafe { GetModuleHandleW(name.as_ptr()) };
        (!module.is_null()).then_some(module)
    };
    let module = named("conpty.dll")
        .or_else(|| named("kernel32.dll"))
        .ok_or_else(io::Error::last_os_error)?;
    let mut file = vec![0u16; 32_768];
    // SAFETY: a loaded module, and a buffer of the length given.
    let length = unsafe { GetModuleFileNameW(module, file.as_mut_ptr(), 32_768) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    file.truncate(length as usize);
    Ok(PathBuf::from(OsString::from_wide(&file)))
}

/// A pipe, neither of whose ends a program started inherits.
fn pipe() -> io::Result<(File, File)> {
    let (mut read, mut write) = (ptr::null_mut(), ptr::null_mut());
    // SAFETY: two writable handles; no attributes, so neither end is inheritable; the
    // system's size.
    if unsafe { CreatePipe(&mut read, &mut write, ptr::null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both handles were just made, are open, and are owned by nothing else.
    Ok(unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) })
}

/// A pseudoconsole, closed when the last of its master and its program's side goes:
/// its host then ends, and with it the output.
struct Console {
    handle: HPCON,
    host: &'static Host,
    #[cfg(test)]
    id: u64,
}

impl Console {
    fn new(host: &'static Host, size: PtySize, input: &File, output: &File) -> io::Result<Self> {
        let mut handle: HPCON = 0;
        // SAFETY: both handles are open for the call, which takes copies of its own;
        // `handle` is writable.
        let made = unsafe {
            (host.create)(
                coord(size),
                input.as_raw_handle(),
                output.as_raw_handle(),
                PSEUDOCONSOLE_RESIZE_QUIRK | PSEUDOCONSOLE_WIN32_INPUT_MODE,
                &mut handle,
            )
        };
        if made != S_OK {
            return Err(io::Error::other(format!(
                "failed to create a pseudoconsole: HRESULT {made:#x}"
            )));
        }
        Ok(Self {
            handle,
            host,
            #[cfg(test)]
            id: closes::next(),
        })
    }

    fn resize(&self, size: PtySize) -> io::Result<()> {
        // SAFETY: the console is open until drop.
        let resized = unsafe { (self.host.resize)(self.handle, coord(size)) };
        if resized != S_OK {
            return Err(io::Error::other(format!(
                "failed to resize the pseudoconsole to {}x{}: HRESULT {resized:#x}",
                size.cols, size.rows
            )));
        }
        Ok(())
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        #[cfg(test)]
        closes::record(self.id, closes::Closed::Console);
        // SAFETY: the console was made by `Console::new` and is closed once, here.
        unsafe { (self.host.close)(self.handle) };
    }
}

fn coord(size: PtySize) -> COORD {
    COORD {
        X: i16::try_from(size.cols).unwrap_or(i16::MAX),
        Y: i16::try_from(size.rows).unwrap_or(i16::MAX),
    }
}

/// What a window reads, writes and resizes. The console is its last field, so that the
/// ends of the pipes go before it is closed: a host left with output to write then fails
/// to write it, where one whose output is held open waited, and the close with it.
/// (portable-pty closed the console first.)
struct Master {
    output: Output,
    input: Mutex<Option<Input>>,
    size: Mutex<PtySize>,
    console: Arc<Console>,
}

/// An end of the console's output: the master's, or a reader's copy of it.
struct Output {
    file: File,
    #[cfg(test)]
    console: u64,
}

impl Output {
    fn new(file: File, _console: &Console) -> Self {
        Self {
            file,
            #[cfg(test)]
            console: _console.id,
        }
    }

    fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
            #[cfg(test)]
            console: self.console,
        })
    }
}

impl io::Read for Output {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.file.read(bytes)
    }
}

#[cfg(test)]
impl Drop for Output {
    fn drop(&mut self) {
        closes::record(self.console, closes::Closed::Output);
    }
}

/// The end of the console's input the window writes to.
struct Input {
    file: File,
    #[cfg(test)]
    console: u64,
}

impl Input {
    fn new(file: File, _console: &Console) -> Self {
        Self {
            file,
            #[cfg(test)]
            console: _console.id,
        }
    }
}

impl io::Write for Input {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
impl Drop for Input {
    fn drop(&mut self) {
        closes::record(self.console, closes::Closed::Input);
    }
}

impl MasterPty for Master {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        self.console.resize(size)?;
        *self.size.lock().unwrap_or_else(PoisonError::into_inner) = size;
        Ok(())
    }

    fn get_size(&self) -> anyhow::Result<PtySize> {
        Ok(*self.size.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn io::Read + Send>> {
        Ok(Box::new(self.output.try_clone()?))
    }

    fn take_writer(&self) -> anyhow::Result<Box<dyn io::Write + Send>> {
        let input = self
            .input
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(|| anyhow::anyhow!("writer already taken"))?;
        Ok(Box::new(input))
    }
}

/// The side of the console a window's program is started on, in the window's job.
struct Slave {
    console: Arc<Console>,
    job: Arc<JobObject>,
}

impl SlavePty for Slave {
    fn spawn_command(
        &self,
        command: CommandBuilder,
    ) -> anyhow::Result<Box<dyn Child + Send + Sync>> {
        Ok(Box::new(spawn(&command, &self.console, &self.job)?))
    }
}

/// Starts `command` on `console`, in `job` from its first instruction.
fn spawn(command: &CommandBuilder, console: &Console, job: &JobObject) -> io::Result<Program> {
    let argv = command.get_argv();
    let named = argv.first().ok_or_else(|| io::Error::other("no program"))?;
    // Before the search for its extensions, which would read a path cut at a NUL as
    // another program.
    whole(named, "program")?;
    let program = program_of(named, command);
    let application = whole(&program, "program")?;
    let mut line = command_line(&program, &argv[1..])?;
    let environment = environment_block(command);
    // The folder asked for, before it is chosen: one cut at a NUL is no folder, and the
    // program would start in another.
    if let Some(asked) = command.get_cwd() {
        whole(asked, "folder")?;
    }
    let folder = folder_of(command).map(|folder| wide(folder.as_os_str()));

    let jobs = [job.handle()];
    let mut attributes = Attributes::new(2)?;
    attributes.console(console)?;
    attributes.jobs(&jobs)?;

    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    // None of the host's standard handles: the console is the program's.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    startup.lpAttributeList = attributes.list();
    let mut started = PROCESS_INFORMATION::default();
    // SAFETY: the program and the folder are NUL-terminated, the command line is a
    // NUL-terminated buffer the call may write to, and the environment a block of
    // NUL-terminated wide strings ended by an empty one; the attribute list, and the
    // console and the job it names, are alive and unmoved until the call has returned;
    // nothing is inherited.
    let made = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            folder
                .as_ref()
                .map_or(ptr::null(), |folder| folder.as_ptr()),
            &startup.StartupInfo,
            &mut started,
        )
    };
    if made == 0 {
        let error = io::Error::last_os_error();
        return Err(io::Error::new(
            error.kind(),
            format!(
                "could not start `{}` in `{}` in its window's job: {error}",
                Path::new(&program).display(),
                folder_of(command).unwrap_or_default().display()
            ),
        ));
    }
    // SAFETY: the process's handle and its first thread's, just made and ours alone.
    let (process, _thread) = unsafe {
        (
            OwnedHandle::from_raw_handle(started.hProcess),
            OwnedHandle::from_raw_handle(started.hThread),
        )
    };
    #[cfg(test)]
    std::thread::sleep(HOLD_STARTED.get());
    Ok(Program {
        process: Arc::new(process),
        pid: started.dwProcessId,
    })
}

#[cfg(test)]
thread_local! {
    /// How long a test holds the start of a program on this thread once it has been made,
    /// where portable-pty's started program used to wait to be put in its job: what the
    /// program starts in that time is the job's all the same.
    static HOLD_STARTED: std::cell::Cell<std::time::Duration> =
        const { std::cell::Cell::new(std::time::Duration::ZERO) };
}

/// A list of the attributes a program is started with. What it names is borrowed for as
/// long as the list is, for the list holds where it is, not a copy.
struct Attributes<'a> {
    /// Words, so that the list is aligned.
    list: Vec<usize>,
    named: PhantomData<&'a ()>,
}

impl<'a> Attributes<'a> {
    fn new(count: u32) -> io::Result<Self> {
        let mut bytes = 0;
        // SAFETY: no list: the call only says how large one must be (and fails, as it
        // does when it is asked so).
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), count, 0, &mut bytes) };
        let mut list = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        // SAFETY: the buffer is writable for `bytes` and more.
        if unsafe {
            InitializeProcThreadAttributeList(list.as_mut_ptr().cast(), count, 0, &mut bytes)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            list,
            named: PhantomData,
        })
    }

    fn list(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.list.as_mut_ptr().cast()
    }

    /// The console the program is started on: the attribute's value is the console's
    /// handle itself.
    fn console(&mut self, console: &'a Console) -> io::Result<()> {
        self.set(
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
            console.handle as *const c_void,
            size_of::<HPCON>(),
        )
    }

    /// The jobs the program is put in as it is made, in their order.
    fn jobs(&mut self, jobs: &'a [HANDLE]) -> io::Result<()> {
        self.set(
            PROC_THREAD_ATTRIBUTE_JOB_LIST,
            jobs.as_ptr().cast(),
            size_of_val(jobs),
        )
    }

    fn set(&mut self, attribute: u32, value: *const c_void, bytes: usize) -> io::Result<()> {
        // SAFETY: the list was initialized for as many attributes as are set; what `value`
        // points to (or is) outlives the list, which its lifetime holds it to.
        let set = unsafe {
            UpdateProcThreadAttribute(
                self.list(),
                0,
                attribute as usize,
                value,
                bytes,
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if set == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Attributes<'_> {
    fn drop(&mut self) {
        // SAFETY: the list was initialized by `Attributes::new` and is deleted once, here.
        unsafe { DeleteProcThreadAttributeList(self.list()) };
    }
}

/// A window's program, by the handle it was started with, which keeps its number from
/// being another's while the pane holds it.
#[derive(Debug)]
struct Program {
    process: Arc<OwnedHandle>,
    pid: u32,
}

impl Child for Program {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        status(&self.process, 0)
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        status(&self.process, INFINITE)?
            .ok_or_else(|| io::Error::other("the program was waited for and had not ended"))
    }

    fn process_id(&self) -> Option<u32> {
        Some(self.pid)
    }

    fn as_raw_handle(&self) -> Option<RawHandle> {
        Some(self.process.as_raw_handle())
    }
}

impl ChildKiller for Program {
    fn kill(&mut self) -> io::Result<()> {
        end(&self.process)
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(Killer(Arc::clone(&self.process)))
    }
}

/// What ends a window's program from elsewhere.
#[derive(Debug)]
struct Killer(Arc<OwnedHandle>);

impl ChildKiller for Killer {
    fn kill(&mut self) -> io::Result<()> {
        end(&self.0)
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(Killer(Arc::clone(&self.0)))
    }
}

/// How the process ended, once it has, waited for `millis`.
fn status(process: &OwnedHandle, millis: u32) -> io::Result<Option<ExitStatus>> {
    // SAFETY: the handle is open while `process` is, and has every right to the process.
    match unsafe { WaitForSingleObject(process.as_raw_handle(), millis) } {
        WAIT_OBJECT_0 => {}
        WAIT_FAILED => return Err(io::Error::last_os_error()),
        _ => return Ok(None),
    }
    let mut code = 0;
    // SAFETY: as above, and `code` is writable.
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Some(ExitStatus::with_exit_code(code)))
}

/// Ends the process. One that has gone cannot be ended again, and is not an error.
fn end(process: &OwnedHandle) -> io::Result<()> {
    // SAFETY: the handle is open while `process` is, and has every right to the process.
    if unsafe { TerminateProcess(process.as_raw_handle(), 1) } != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match status(process, 0) {
        Ok(Some(_)) => Ok(()),
        _ => Err(error),
    }
}

/// The program portable-pty would start for `program`, which the table has made sure is
/// a whole path, so that no folder of `PATH` changes it: the path itself where it is a
/// file, else the first with an extension of `PATHEXT` (each in place of the path's own)
/// that is, else the path as it is.
fn program_of(program: &OsStr, command: &CommandBuilder) -> OsString {
    let path = Path::new(program);
    if path.exists() || command.get_env("PATH").is_none() {
        return program.to_owned();
    }
    let extensions = command.get_env("PATHEXT").unwrap_or(OsStr::new(".EXE"));
    std::env::split_paths(extensions)
        .filter_map(|extension| {
            let extension = extension.to_str()?.strip_prefix('.')?.to_owned();
            Some(path.with_extension(extension))
        })
        .find(|candidate| candidate.exists())
        .map_or_else(|| program.to_owned(), PathBuf::into_os_string)
}

/// The folder the program starts in: the command's where it is one, else the user's
/// (`USERPROFILE`) where that is, else the host's own (none named). A relative one is
/// taken from the host's.
fn folder_of(command: &CommandBuilder) -> Option<PathBuf> {
    let folder = command
        .get_cwd()
        .map(Path::new)
        .filter(|folder| folder.is_dir())
        .or_else(|| {
            command
                .get_env("USERPROFILE")
                .map(Path::new)
                .filter(|folder| folder.is_dir())
        })?;
    if folder.is_relative() {
        if let Ok(host) = std::env::current_dir() {
            return Some(host.join(folder));
        }
    }
    Some(folder.to_path_buf())
}

/// The command line: the program and each argument quoted as a program's C runtime
/// reads it back (`ArgvQuote`, which portable-pty's is), and a NUL.
fn command_line(program: &OsStr, arguments: &[OsString]) -> io::Result<Vec<u16>> {
    let mut line = Vec::new();
    append_quoted(program, &mut line);
    for argument in arguments {
        if argument.encode_wide().any(|unit| unit == 0) {
            return Err(io::Error::other(format!(
                "invalid encoding for command line argument {argument:?}"
            )));
        }
        line.push(u16::from(b' '));
        append_quoted(argument, &mut line);
    }
    line.push(0);
    Ok(line)
}

fn append_quoted(argument: &OsStr, line: &mut Vec<u16>) {
    let special = |unit: u16| b" \t\n\x0b\"".map(u16::from).contains(&unit);
    if !argument.is_empty() && !argument.encode_wide().any(special) {
        line.extend(argument.encode_wide());
        return;
    }
    let (quote, backslash) = (u16::from(b'"'), u16::from(b'\\'));
    line.push(quote);
    let units: Vec<u16> = argument.encode_wide().collect();
    let mut at = 0;
    while at < units.len() {
        let mut backslashes = 0;
        while at < units.len() && units[at] == backslash {
            at += 1;
            backslashes += 1;
        }
        if at == units.len() {
            // Before the closing quote, each is doubled.
            line.extend(std::iter::repeat_n(backslash, backslashes * 2));
            break;
        }
        if units[at] == quote {
            // Before a quote, each is doubled and the quote escaped.
            line.extend(std::iter::repeat_n(backslash, backslashes * 2 + 1));
        } else {
            line.extend(std::iter::repeat_n(backslash, backslashes));
        }
        line.push(units[at]);
        at += 1;
    }
    line.push(quote);
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

/// `text` NUL-terminated, refused where it holds a NUL of its own, which would end it
/// early and name something else: the `what` it is says which.
fn whole(text: &OsStr, what: &str) -> io::Result<Vec<u16>> {
    if text.encode_wide().any(|unit| unit == 0) {
        return Err(io::Error::other(format!(
            "invalid encoding for the {what} {text:?}: it holds a NUL"
        )));
    }
    Ok(wide(text))
}

/// What a test reads of the order a console and the ends of its output were closed in.
#[cfg(test)]
pub(super) mod closes {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in crate::pty) enum Closed {
        Output,
        Input,
        Console,
    }

    static NEXT: AtomicU64 = AtomicU64::new(1);
    static CLOSED: Mutex<Vec<(u64, Closed)>> = Mutex::new(Vec::new());

    /// The number the next console made is given.
    pub(in crate::pty) fn upcoming() -> u64 {
        NEXT.load(Ordering::SeqCst)
    }

    pub(super) fn next() -> u64 {
        NEXT.fetch_add(1, Ordering::SeqCst)
    }

    pub(super) fn record(console: u64, closed: Closed) {
        CLOSED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((console, closed));
    }

    /// What of `console` has been closed, in order.
    pub(in crate::pty) fn of(console: u64) -> Vec<Closed> {
        CLOSED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(id, _)| *id == console)
            .map(|(_, closed)| *closed)
            .collect()
    }
}

#[cfg(test)]
mod tests;
