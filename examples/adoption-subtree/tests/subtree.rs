//! The embedded-subtree rung, under test (`PLAN.md` Milestone 40): the
//! host application embeds the subtree, clicks the embedded button through
//! plain Win32, resizes itself, and drops the subtree — and checks each
//! step from the host's side.

#![cfg(windows)]

#[test]
fn a_win32_application_hosts_a_rust_native_subtree() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_adoption-subtree"))
        .arg("--self-test")
        .output()
        .expect("the host runs");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok");
}
