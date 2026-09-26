//! Milestone 51 on Windows: crash capture with the tree at failure, the
//! isolated worker's sandbox, industrial services, and accelerator answers.
//! The crash tests run a child copy of this test binary that crashes on
//! purpose, then read what it left behind.

#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed expectation in an integration test is the test failing; the children crash on purpose"
)]

use std::path::PathBuf;
use std::process::Command;

use rustnative_windows::crash;
use rustnative_windows::isolated::{IsolatedWorker, Sandbox};

fn scratch(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("rustnative-m51-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// Runs the child test `name` with `LOCALAPPDATA` at `appdata`.
fn crash_child(name: &str, appdata: &std::path::Path) -> std::process::Output {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env("RUSTNATIVE_CRASH_CHILD", "1")
        .env("LOCALAPPDATA", appdata)
        .output()
        .unwrap()
}

fn is_child() -> bool {
    std::env::var_os("RUSTNATIVE_CRASH_CHILD").is_some()
}

#[test]
fn child_panics() {
    if !is_child() {
        return;
    }
    crash::install("dev.rustnative.crashes", "1.2.3");
    crash::record_tree(|| {
        r#"{"kind":"Column","children":[{"kind":"Button","text":"Save"}]}"#.to_owned()
    });
    panic!("the save button broke");
}

#[test]
fn child_faults() {
    if !is_child() {
        return;
    }
    crash::install("dev.rustnative.crashes", "1.2.3");
    // SAFETY: none; this deliberately reads address zero to raise an
    // access violation, the crash this test is about.
    let value = unsafe { std::ptr::read_volatile(std::ptr::null::<u8>()) };
    std::hint::black_box(value);
}

// req: E-OB-1
#[test]
fn a_panic_leaves_a_report_with_the_tree_and_a_dump() {
    let appdata = scratch("panic");
    let output = crash_child("child_panics", &appdata);
    assert!(!output.status.success(), "the child crashed");
    let reports = crash::reports(&crash::directory_in(&appdata, "dev.rustnative.crashes"));
    let report = reports.first().expect("a report was written");
    assert!(report.message.contains("the save button broke"), "{}", report.message);
    assert_eq!(report.app_version, "1.2.3");
    assert!(
        report.tree.as_deref().is_some_and(|tree| tree.contains("Save")),
        "the tree at failure is kept"
    );
    assert!(
        report.backtrace.contains("child_panics"),
        "symbolicated in process: {}",
        report.backtrace
    );
    let dump = PathBuf::from(report.dump.as_ref().expect("a minidump"));
    assert!(std::fs::metadata(&dump).unwrap().len() > 0);
}

#[test]
fn an_access_violation_leaves_a_report_and_a_dump() {
    let appdata = scratch("fault");
    let output = crash_child("child_faults", &appdata);
    assert!(!output.status.success());
    let reports = crash::reports(&crash::directory_in(&appdata, "dev.rustnative.crashes"));
    let report = reports.first().expect("a report was written");
    assert!(report.message.contains("0xC0000005"), "{}", report.message);
    assert!(report.dump.is_some());
}

// req: C67-1
#[test]
fn an_isolated_worker_talks_over_its_channel_and_cannot_write_the_users_files() {
    // `findstr` echoes each line it reads: a stand-in worker speaking
    // one JSON message per line.
    let mut worker = IsolatedWorker::spawn(r#"findstr.exe "^""#, Sandbox::default()).unwrap();
    worker.send(&serde_json::json!({ "parse": "notes.md" })).unwrap();
    // `findstr` may hold its output until its input ends.
    worker.close_input();
    let echoed: serde_json::Value = worker.recv().unwrap().expect("a reply");
    assert_eq!(echoed["parse"], "notes.md");
    assert!(worker.recv::<serde_json::Value>().unwrap().is_none(), "it ended with its input");

    // At low integrity it cannot write where the person's files are.
    let target = scratch("isolated").join("written-by-worker.txt");
    let mut writer = IsolatedWorker::spawn(
        &format!(r#"cmd.exe /d /c "echo x> "{}" & echo done""#, target.display()),
        Sandbox::default(),
    )
    .unwrap();
    // Its error ("Access is denied.") shares the channel; read to the end.
    let mut lines = Vec::new();
    while let Some(line) = writer.recv_line().unwrap() {
        lines.push(line);
    }
    assert_eq!(lines.last().map(String::as_str), Some("done"), "{lines:?}");
    assert!(!target.exists(), "a low-integrity worker wrote a medium-integrity folder");
}

#[test]
fn a_dropped_worker_is_killed_with_it() {
    let started = std::time::Instant::now();
    let worker = IsolatedWorker::spawn("ping.exe -n 30 127.0.0.1", Sandbox::default()).unwrap();
    drop(worker);
    // Nothing waits for the ping; the job closing ended it.
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[test]
fn printers_are_listed_and_print_to_a_file_where_a_document_writer_exists() {
    use rustnative_core::industrial::{PrintJob, PrintService};
    let printing = rustnative_windows::WindowsPrinting;
    let printers = printing.printers();
    let Some(writer) = printers.iter().find(|name| name.contains("XPS Document Writer")) else {
        eprintln!("no XPS Document Writer on this machine: printing to a file is not checked");
        return;
    };
    let output = scratch("print").join("page.xps");
    printing
        .print(&PrintJob {
            title: "Milestone 51".into(),
            pages: vec![vec!["Hello, printer.".into()]],
            printer: Some(writer.clone()),
            output: Some(output.clone()),
        })
        .unwrap();
    assert!(std::fs::metadata(&output).is_ok_and(|metadata| metadata.len() > 0));
}

#[test]
fn a_missing_serial_port_is_an_error_not_a_hang() {
    use rustnative_core::industrial::{SerialService, SerialSettings};
    let serial = rustnative_windows::WindowsSerial;
    assert!(serial.open("COM255", SerialSettings::default()).is_err());
    assert!(!serial.ports().contains(&"COM255".to_owned()));
}
