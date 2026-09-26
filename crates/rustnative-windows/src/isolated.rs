//! Isolated worker processes (`PLAN.md` Milestone 51, `C67`): untrusted or
//! fragile work — a parser for files from the internet, a plug-in — runs
//! in a child process that can reach almost nothing, talking to the
//! application over a typed channel.
//!
//! The sandbox, on Windows:
//!
//! - **Low integrity.** The child runs with a low-integrity token, so it
//!   cannot write to the user's files, the registry's user hive, or any
//!   process at a higher level; it can only write where low-integrity
//!   processes may (`LocalLow`).
//! - **A job object.** The job kills the child when the worker is dropped
//!   (it cannot outlive its owner), caps its memory, and forbids it from
//!   touching other processes' windows, the clipboard, and system settings.
//! - **A channel.** Standard input and output are anonymous pipes carrying
//!   one JSON message per line.
//!
//! A crash is reported to the owner (`recv` ends), which restarts it by its
//! supervision policy (`rustnative_durable::supervise`).

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;

use serde::Serialize;
use serde::de::DeserializeOwned;
use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, LocalFree, SetHandleInformation,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows_sys::Win32::Security::{
    DuplicateTokenEx, SECURITY_ATTRIBUTES, SID_AND_ATTRIBUTES, SecurityImpersonation,
    SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenIntegrityLevel, TokenPrimary,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOB_OBJECT_UILIMIT_DESKTOP,
    JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
    JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES, JOB_OBJECT_UILIMIT_READCLIPBOARD,
    JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicUIRestrictions, JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::SystemServices::SE_GROUP_INTEGRITY;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CreateProcessAsUserW, GetCurrentProcess, OpenProcessToken,
    PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOW, TerminateProcess,
};

/// What the sandbox allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sandbox {
    /// The most memory the worker may commit, in bytes.
    pub memory: usize,
}

impl Default for Sandbox {
    fn default() -> Self {
        Self { memory: 256 * 1024 * 1024 }
    }
}

/// A running isolated worker.
pub struct IsolatedWorker {
    process: HANDLE,
    job: HANDLE,
    input: std::fs::File,
    output: BufReader<std::fs::File>,
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

fn check(ok: i32, what: &str) -> std::io::Result<()> {
    if ok == 0 {
        Err(std::io::Error::other(format!("{what}: {}", std::io::Error::last_os_error())))
    } else {
        Ok(())
    }
}

/// A duplicate of this process's token, lowered to low integrity.
fn low_integrity_token() -> std::io::Result<HANDLE> {
    let mut own: HANDLE = std::ptr::null_mut();
    // SAFETY: the current-process pseudo-handle and an out-pointer.
    let ok = unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, &raw mut own)
    };
    check(ok, "OpenProcessToken")?;
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: `own` is a token handle this function opened.
    let duplicated = unsafe {
        DuplicateTokenEx(
            own,
            TOKEN_ADJUST_DEFAULT | TOKEN_ASSIGN_PRIMARY | TOKEN_DUPLICATE | TOKEN_QUERY,
            std::ptr::null(),
            SecurityImpersonation,
            TokenPrimary,
            &raw mut token,
        )
    };
    // SAFETY: closing the handle opened above.
    unsafe { CloseHandle(own) };
    check(duplicated, "DuplicateTokenEx")?;
    let mut sid = std::ptr::null_mut();
    // S-1-16-4096: the low mandatory level.
    let text = wide(OsStr::new("S-1-16-4096"));
    // SAFETY: a NUL-terminated SID string and an out-pointer; the SID is
    // freed with LocalFree below.
    let ok = unsafe { ConvertStringSidToSidW(text.as_ptr(), &raw mut sid) };
    check(ok, "ConvertStringSidToSidW")?;
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid,
            Attributes: u32::try_from(SE_GROUP_INTEGRITY).unwrap_or(0x20),
        },
    };
    // SAFETY: `token` is ours; `label` and its SID are valid for the call.
    let set = unsafe {
        SetTokenInformation(
            token,
            TokenIntegrityLevel,
            std::ptr::from_ref(&label).cast(),
            u32::try_from(std::mem::size_of::<TOKEN_MANDATORY_LABEL>()).unwrap_or(0),
        )
    };
    // SAFETY: freeing the SID ConvertStringSidToSidW allocated.
    unsafe { LocalFree(sid.cast()) };
    check(set, "SetTokenInformation")?;
    Ok(token)
}

fn job(sandbox: Sandbox) -> std::io::Result<HANDLE> {
    // SAFETY: no attributes, no name.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: zeroed is a valid initial value for these plain structs.
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
    limits.ProcessMemoryLimit = sandbox.memory;
    // SAFETY: `job` is ours; the struct is valid for its size.
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap_or(0),
        )
    };
    check(ok, "SetInformationJobObject(limits)")?;
    let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
        UIRestrictionsClass: JOB_OBJECT_UILIMIT_HANDLES
            | JOB_OBJECT_UILIMIT_READCLIPBOARD
            | JOB_OBJECT_UILIMIT_WRITECLIPBOARD
            | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
            | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
            | JOB_OBJECT_UILIMIT_GLOBALATOMS
            | JOB_OBJECT_UILIMIT_DESKTOP
            | JOB_OBJECT_UILIMIT_EXITWINDOWS,
    };
    // SAFETY: as above.
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JobObjectBasicUIRestrictions,
            std::ptr::from_ref(&ui).cast(),
            u32::try_from(std::mem::size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>()).unwrap_or(0),
        )
    };
    check(ok, "SetInformationJobObject(ui)")?;
    Ok(job)
}

fn pipe(inherit_read: bool) -> std::io::Result<(HANDLE, HANDLE)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write): (HANDLE, HANDLE) = (std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: out-pointers and a valid attributes struct.
    let ok = unsafe { CreatePipe(&raw mut read, &raw mut write, &raw const attributes, 0) };
    check(ok, "CreatePipe")?;
    // The parent's end must not be inherited.
    let parent_end = if inherit_read { write } else { read };
    // SAFETY: a handle this function created.
    let ok = unsafe { SetHandleInformation(parent_end, HANDLE_FLAG_INHERIT, 0) };
    check(ok, "SetHandleInformation")?;
    Ok((read, write))
}

impl IsolatedWorker {
    /// Starts `command_line` (the executable and its arguments) in the
    /// sandbox.
    ///
    /// # Errors
    ///
    /// The sandbox or the process could not be set up.
    pub fn spawn(command_line: &str, sandbox: Sandbox) -> std::io::Result<Self> {
        let token = low_integrity_token()?;
        let job = job(sandbox)?;
        let (child_in, parent_in) = pipe(true)?;
        let (parent_out, child_out) = pipe(false)?;
        // SAFETY: zeroed is valid for STARTUPINFOW before its size is set.
        let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup.cb = u32::try_from(std::mem::size_of::<STARTUPINFOW>()).unwrap_or(0);
        startup.dwFlags = STARTF_USESTDHANDLES;
        startup.hStdInput = child_in;
        startup.hStdOutput = child_out;
        startup.hStdError = child_out;
        // SAFETY: as above, for the out-structure.
        let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let mut command = wide(OsStr::new(command_line));
        // SAFETY: every pointer is valid for the call; the command line is
        // a mutable NUL-terminated buffer as the API requires.
        let created = unsafe {
            CreateProcessAsUserW(
                token,
                std::ptr::null(),
                command.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                CREATE_SUSPENDED | CREATE_NO_WINDOW,
                std::ptr::null(),
                std::ptr::null(),
                &raw const startup,
                &raw mut process,
            )
        };
        // SAFETY: the child's ends and the token belong to this function.
        unsafe {
            CloseHandle(child_in);
            CloseHandle(child_out);
            CloseHandle(token);
        }
        check(created, "CreateProcessAsUserW")?;
        // In the job before its first instruction runs.
        // SAFETY: both handles are ours.
        let assigned = unsafe { AssignProcessToJobObject(job, process.hProcess) };
        if assigned == 0 {
            // SAFETY: terminating the suspended child we just created.
            unsafe {
                TerminateProcess(process.hProcess, 1);
            }
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: resuming and releasing the thread handle we own.
        unsafe {
            ResumeThread(process.hThread);
            CloseHandle(process.hThread);
        }
        // SAFETY: the parent's pipe ends are ours, handed to `File` once.
        let (input, output) = unsafe {
            (std::fs::File::from_raw_handle(parent_in), std::fs::File::from_raw_handle(parent_out))
        };
        Ok(Self { process: process.hProcess, job, input, output: BufReader::new(output) })
    }

    /// Sends a message.
    ///
    /// # Errors
    ///
    /// The worker is gone.
    pub fn send<T: Serialize>(&mut self, message: &T) -> std::io::Result<()> {
        let line = serde_json::to_string(message).map_err(std::io::Error::other)?;
        writeln!(self.input, "{line}")?;
        self.input.flush()
    }

    /// Receives a message; `None` when the worker has ended (or crashed).
    ///
    /// # Errors
    ///
    /// A malformed message.
    pub fn recv<T: DeserializeOwned>(&mut self) -> std::io::Result<Option<T>> {
        let mut line = String::new();
        if self.output.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        serde_json::from_str(line.trim_end()).map(Some).map_err(std::io::Error::other)
    }

    /// Receives the next raw line (for workers that do not speak JSON).
    ///
    /// # Errors
    ///
    /// The pipe failed.
    pub fn recv_line(&mut self) -> std::io::Result<Option<String>> {
        let mut line = String::new();
        if self.output.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end().to_owned()))
    }

    /// Closes the input (the worker sees end of input).
    pub fn close_input(&mut self) {
        let _ = self.input.flush();
        // Replace the pipe with a null handle file: dropping it closes it.
        if let Ok(null) = std::fs::File::open("NUL") {
            drop(std::mem::replace(&mut self.input, null));
        }
    }
}

impl Drop for IsolatedWorker {
    fn drop(&mut self) {
        // Closing the job kills the worker (KILL_ON_JOB_CLOSE).
        // SAFETY: handles this value owns, closed once.
        unsafe {
            CloseHandle(self.job);
            CloseHandle(self.process);
        }
    }
}
