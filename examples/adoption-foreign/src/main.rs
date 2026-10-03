//! Embedding outward (`PLAN.md` Milestone 40) into a Win32 application:
//! the host and its controls are Windows' (`win32`). Linux's counterpart is
//! the `adoption-gtk` example.

#[cfg(windows)]
mod win32;

fn main() {
    #[cfg(windows)]
    win32::main();
    #[cfg(not(windows))]
    eprintln!("adoption-foreign hosts Win32 controls; on Linux, run the adoption-gtk example");
}
