//! Two devices edit one document offline and converge when they reconnect
//! (`PLAN.md` Milestone 55's first "done when").

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::sync::{Arc, Mutex};

use collab_notes::{Doc, Editor, EditorProps, merge_policy};
use rustnative_core::{Component, Services, Size, Theme, Window};
use rustnative_headless::{HeadlessApp, Query};
use rustnative_sync::{InMemory, SyncServer};

fn device(replica: u64, transport: &InMemory) -> HeadlessApp {
    let props =
        EditorProps { replica, doc: "shopping".into(), transport: Arc::new(transport.clone()) };
    HeadlessApp::launch_with(
        Window::new("Notes", Size::new(400, 300)),
        Services::default(),
        Theme::default(),
        move || Editor::new(props.clone()),
    )
}

fn text(app: &HeadlessApp) -> String {
    app.find(&Query::key("text"))
        .map(|node| node.text.clone().unwrap_or_default())
        .unwrap_or_default()
}

fn sync(app: &mut HeadlessApp) {
    app.click(&Query::key("sync")).unwrap();
    app.settle();
}

#[test]
fn offline_edits_on_two_devices_converge() {
    let server = Arc::new(Mutex::new(SyncServer::new().collection::<Doc>(merge_policy(), 1)));
    let (laptop_link, phone_link) =
        (InMemory::new(Arc::clone(&server)), InMemory::new(Arc::clone(&server)));
    let mut laptop = device(1, &laptop_link);
    let mut phone = device(2, &phone_link);

    laptop.set_text(&Query::key("text"), "milk").unwrap();
    sync(&mut laptop);
    sync(&mut phone);
    assert_eq!(text(&phone), "milk");

    // Both go offline and edit the same line.
    laptop_link.set_online(false);
    phone_link.set_online(false);
    laptop.set_text(&Query::key("text"), "oat milk").unwrap();
    phone.set_text(&Query::key("text"), "milk, bread").unwrap();
    sync(&mut laptop);
    assert!(text(&laptop).contains("oat milk"), "offline, the device keeps its own edit");
    assert!(
        laptop
            .find(&Query::text("Offline — edits are kept (the server is unreachable: offline)"))
            .is_ok()
    );

    // Back online: both syncs, in either order, end in the same text.
    laptop_link.set_online(true);
    phone_link.set_online(true);
    sync(&mut phone);
    sync(&mut laptop);
    sync(&mut phone);
    assert_eq!(text(&laptop), text(&phone), "converged");
    assert_eq!(text(&laptop), "oat milk, bread", "both edits kept");
}
