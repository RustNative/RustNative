//! The inspection protocol answered by the Windows backend over the real
//! transport (`PLAN.md` Milestone 44): a client on another thread, the
//! server waking the UI thread's loop, and the answers read from the native
//! objects — plus the overlay window.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use rustnative_core::inspect::{Endpoint, Reply, Request, send_request};
use rustnative_core::{Application, Component, Event, Node, Size, Window, WindowId};
use serde_json::{Value, json};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, GetWindowTextW, WS_EX_LAYERED, WS_EX_TRANSPARENT,
};

use super::harness::NativeHarness;

struct Counter {
    count: u32,
}

impl Component for Counter {
    type Props = ();
    type Message = u32;
    fn new((): ()) -> Self {
        Self { count: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "root",
            [Node::label("count", format!("{}", self.count)), Node::button("increment", "More")],
        )
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { .. }) {
            self.count += 1;
        }
    }
    fn message(&mut self, count: u32) {
        self.count = count;
    }
    fn inspect(&self) -> Option<Value> {
        Some(json!({ "count": self.count }))
    }
    fn edit(&self, field: &str, value: &Value) -> Option<u32> {
        (field == "count").then(|| value.as_u64().and_then(|v| u32::try_from(v).ok())).flatten()
    }
}

fn pump_until<T>(harness: &mut NativeHarness, receiver: &Receiver<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        harness.pump();
        if let Ok(value) = receiver.try_recv() {
            return value;
        }
        assert!(Instant::now() < deadline, "the inspector's request was not answered");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Sends `request` from another thread, as the CLI would, and pumps the UI
/// thread until it is answered.
fn ask(harness: &mut NativeHarness, endpoint: &Endpoint, request: Request) -> Value {
    let (sender, receiver) = mpsc::channel();
    let endpoint = endpoint.clone();
    std::thread::spawn(move || sender.send(send_request(&endpoint, &request).unwrap()));
    match pump_until(harness, &receiver) {
        Reply::Ok(value) => value,
        Reply::Error(error) => panic!("the inspector refused: {error}"),
    }
}

fn text(hwnd: windows_sys::Win32::Foundation::HWND) -> String {
    let mut buffer = [0u16; 64];
    // SAFETY: `hwnd` is a live control; the buffer's length is passed.
    let length = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), 64) };
    String::from_utf16_lossy(&buffer[..usize::try_from(length).unwrap()])
}

#[test]
fn native_inspection_answers_from_the_native_objects_over_the_transport() {
    let mut application =
        Application::new(Counter::new(()), Window::new("Inspect", Size::new(320, 200)));
    let endpoint = application.enable_inspection(None).unwrap();
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    let hello = ask(&mut harness, &endpoint, Request::Hello);
    assert_eq!(hello["backend"], json!("windows"));

    let realized = ask(&mut harness, &endpoint, Request::Realized { window: None });
    let count = realized
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["key"] == json!("count"))
        .unwrap()
        .clone();
    assert_eq!(count["host_type"], json!("STATIC"));
    assert!(count["handle"].as_str().unwrap().starts_with("0x"));
    assert!(count["rect"][2].as_i64().unwrap() > 0, "{count}");

    let layout = ask(
        &mut harness,
        &endpoint,
        Request::ExplainLayout { node: "increment".into(), window: None },
    );
    assert_eq!(layout["realized"], json!(true));

    // An edit is a message: it renders, and the control shows it.
    let path = application_path(&mut harness, &endpoint);
    let _ = ask(
        &mut harness,
        &endpoint,
        Request::SetState { path, field: "count".into(), value: json!(5), window: None },
    );
    harness.pump();
    assert_eq!(text(harness.expect_control(WindowId::PRIMARY, "count")), "5");

    let lifetimes = ask(&mut harness, &endpoint, Request::Lifetimes);
    assert!(lifetimes["live"].as_u64().unwrap() >= 3, "{lifetimes}");
    assert!(
        lifetimes["recent"].as_array().unwrap().iter().any(|event| event["host_type"] == "BUTTON")
    );

    let capabilities = ask(&mut harness, &endpoint, Request::Capabilities);
    assert_eq!(capabilities["backend"], json!("windows"));
    assert!(capabilities["style"].as_array().unwrap().iter().any(|row| {
        row["what"] == "box-shadow" && row["why"].as_str().unwrap().starts_with("unavailable")
    }));
    assert!(ask(&mut harness, &endpoint, Request::Mappers).is_array());
}

fn application_path(harness: &mut NativeHarness, endpoint: &Endpoint) -> String {
    let components = ask(harness, endpoint, Request::Components { window: None });
    components[0]["path"].as_str().unwrap().to_owned()
}

#[test]
fn native_overlay_is_a_click_through_canvas_over_the_window() {
    let mut application =
        Application::new(Counter::new(()), Window::new("Overlay", Size::new(320, 200)));
    let endpoint = application.enable_inspection(None).unwrap();
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let root = harness.hwnd(WindowId::PRIMARY);
    assert!(super::inspect::overlay_of(root).is_none());

    let _ = ask(
        &mut harness,
        &endpoint,
        Request::Overlay { mode: Some(rustnative_core::inspect::OverlayMode::Layout) },
    );
    harness.pump();
    let overlay = super::inspect::overlay_of(root).expect("the overlay is shown");
    // SAFETY: `overlay` is live.
    let style = u32::try_from(unsafe { GetWindowLongPtrW(overlay, GWL_EXSTYLE) }).unwrap();
    assert_eq!(style & (WS_EX_LAYERED | WS_EX_TRANSPARENT), WS_EX_LAYERED | WS_EX_TRANSPARENT);
    let list = super::graphics::canvas::draw_list_of(overlay).unwrap();
    assert_eq!(list.commands().len(), 6, "an outline and a label for each of three nodes");

    let _ = ask(&mut harness, &endpoint, Request::Overlay { mode: None });
    harness.pump();
    assert!(super::inspect::overlay_of(root).is_none(), "hiding it destroys the window");
}
