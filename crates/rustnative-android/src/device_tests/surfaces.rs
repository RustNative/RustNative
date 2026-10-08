//! Surfaces beyond the window, and background work (Phase 6): what the
//! application sends is read back from the system's own records.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rustnative_core::surfaces::{
    JumpTask, OngoingActivity, SurfaceCommand, TileState, TrayMenuItem, WidgetContent,
};
use rustnative_data::Constraints;

use super::harness::{Instrumentation, java};
use crate::jni_host::{Arg, Class};
use crate::services::{AndroidWork, job_id};
use crate::surfaces::{apply_one, ongoing_id};

fn wait(what: &str, done: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn a_widget_keeps_what_the_application_sent(_: &Instrumentation) {
    apply_one(SurfaceCommand::UpdateWidget {
        id: "glance".into(),
        content: WidgetContent {
            title: "Today".into(),
            lines: vec!["3 tasks".into(), "1 overdue".into()],
            actions: vec![TrayMenuItem::new("open", "Open")],
        },
    });
    let stored = java(
        Class::Services,
        "widgetContent",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[Arg::Str("glance")],
    )
    .string()
    .unwrap_or_default();
    assert_eq!(stored, "Today\u{1}3 tasks\u{2}1 overdue\u{1}open\u{2}Open");
}

pub(super) fn a_tile_keeps_its_state(_: &Instrumentation) {
    apply_one(SurfaceCommand::UpdateTile {
        id: "toggle".into(),
        state: TileState { label: "Focus".into(), subtitle: Some("On".into()), active: true },
    });
    let state = java(
        Class::Services,
        "tileState",
        "(Ljava/lang/String;)[Ljava/lang/String;",
        &[Arg::Str("toggle")],
    )
    .strings();
    assert_eq!(state, ["Focus", "On", "1"]);
}

pub(super) fn an_ongoing_activity_shows_until_it_ends(instrumentation: &Instrumentation) {
    instrumentation.shell(&format!("pm grant {} android.permission.POST_NOTIFICATIONS", package()));
    let id = ongoing_id("upload");
    let showing =
        move || java(Class::Services, "notificationShowing", "(I)Z", &[Arg::Int(id)]).bool();
    apply_one(SurfaceCommand::Ongoing {
        id: "upload".into(),
        activity: OngoingActivity {
            title: "Uploading".into(),
            body: "2 of 5".into(),
            progress: Some(0.4),
            actions: vec![TrayMenuItem::new("cancel", "Cancel")],
        },
    });
    wait("the ongoing notification", showing);
    apply_one(SurfaceCommand::EndOngoing { id: "upload".into() });
    wait("the ongoing notification to go", move || !showing());
}

pub(super) fn the_jump_list_becomes_launcher_shortcuts(_: &Instrumentation) {
    apply_one(SurfaceCommand::JumpList(vec![
        JumpTask { label: "New note".into(), arguments: "new".into() },
        JumpTask { label: "Search".into(), arguments: "search".into() },
    ]));
    let labels = java(Class::Services, "shortcuts", "()[Ljava/lang/String;", &[]).strings();
    assert_eq!(labels, ["New note", "Search"]);
}

static RAN: AtomicBool = AtomicBool::new(false);

pub(super) fn a_constrained_job_runs_through_job_scheduler(instrumentation: &Instrumentation) {
    RAN.store(false, Ordering::SeqCst);
    AndroidWork::register("suite", || {
        RAN.store(true, Ordering::SeqCst);
        false
    });
    AndroidWork::schedule("suite", Constraints { network: true, charging: false, deadline: None })
        .expect("scheduled");
    assert!(AndroidWork::is_pending("suite"), "the job waits for its constraints");
    // Forced, as `adb shell cmd jobscheduler run -f` does: the scheduler
    // starts `RnJobService`, which runs the registered handler.
    instrumentation.shell(&format!("cmd jobscheduler run -f {} {}", package(), job_id("suite")));
    wait("the job to run", || RAN.load(Ordering::SeqCst));
    wait("the job to finish", || !AndroidWork::is_pending("suite"));
}

fn package() -> String {
    java(Class::Services, "packageName", "()Ljava/lang/String;", &[]).string().unwrap_or_default()
}
