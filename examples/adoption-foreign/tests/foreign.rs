//! Embedding outward, under test (`PLAN.md` Milestone 40): the application
//! adopts a month calendar (owned) and a date picker (borrowed), checks they
//! are laid out at their factories' sizes, removes both, and checks the
//! calendar is destroyed and the picker handed back.

#![cfg(windows)]

#[test]
fn a_rust_native_application_hosts_foreign_controls() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_adoption-foreign"))
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
