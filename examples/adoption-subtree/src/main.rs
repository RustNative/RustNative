//! The embedded-subtree rung of the adoption ladder (`PLAN.md` Milestone
//! 40) in a Win32 application (`win32`). Linux's counterpart is the
//! `adoption-gtk` example.

#[cfg(windows)]
mod win32;

fn main() {
    #[cfg(windows)]
    win32::main();
    #[cfg(not(windows))]
    eprintln!("adoption-subtree is a Win32 host; on Linux, run the adoption-gtk example");
}
