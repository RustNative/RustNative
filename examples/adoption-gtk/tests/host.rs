//! The GTK adoption ladder, under test (Milestone 40): the host embeds the
//! subtree, the subtree adopts the host's calendar, a click in the subtree
//! changes it, and dropping the subtree hands the calendar back.

#![cfg(target_os = "linux")]

#[test]
fn a_gtk_application_hosts_a_subtree_that_adopts_its_widget() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        eprintln!("skipped: no display (run under tools/linux-session.sh)");
        return;
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_adoption-gtk"))
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
