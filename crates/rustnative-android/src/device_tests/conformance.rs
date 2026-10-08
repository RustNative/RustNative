//! The new-backend checklist's Android rows that need a device
//! (`docs/conformance/new-backend-checklist.md`): cursors per node,
//! per-property mappers, and the panic policy at the JNI boundary.

use rustnative_core::{Application, Component, Cursor, Event, Node, NodeKind, Size, Window};

use super::harness::{Harness, Instrumentation, java, on_main};
use crate::jni_host::{Arg, Class, call_static};
use crate::{MappedProperty, MapperMode, MapperTarget, register_mapper};

struct Panel;

impl Component for Panel {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::button("go", "Go").with_cursor(Cursor::Pointer),
                Node::label("count", "unchanged"),
                Node::button("boom", "Boom"),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        if let Event::Click { target } = event {
            assert!(
                target != rustnative_core::NodeId::from_key("boom"),
                "a deliberate panic in a handler"
            );
        }
    }
}

fn launch() -> Harness {
    Harness::launch(Application::new(Panel, Window::new("Conformance", Size::new(320, 240))))
}

pub(super) fn a_node_s_cursor_is_its_view_s_pointer_icon(_: &Instrumentation) {
    on_main(|| {
        let harness = launch();
        let shows = |key: &str, icon: i32| {
            java(
                Class::Views,
                "showsCursor",
                "(Landroid/view/View;I)Z",
                &[Arg::Obj(&harness.expect(key)), Arg::Int(icon)],
            )
            .bool()
        };
        assert!(
            shows("go", crate::input::pointer_icon(Cursor::Pointer)),
            "the hand over the button"
        );
        assert!(
            !shows("count", crate::input::pointer_icon(Cursor::Pointer)),
            "the label keeps its own"
        );
    });
}

pub(super) fn mappers_extend_and_replace_what_the_backend_applies(_: &Instrumentation) {
    on_main(|| {
        register_mapper(
            MapperTarget::Kind(NodeKind::Button),
            MappedProperty::Text,
            MapperMode::Extend,
            |context| {
                let _ = call_static(
                    Class::Views,
                    "setTooltip",
                    "(Landroid/view/View;Ljava/lang/String;)V",
                    &[Arg::Obj(&context.view.0), Arg::Str("added by a mapper")],
                );
            },
        );
        register_mapper(
            MapperTarget::Key("count".into()),
            MappedProperty::Text,
            MapperMode::Replace,
            |context| {
                let _ = call_static(
                    Class::Views,
                    "setText",
                    "(Landroid/view/View;Ljava/lang/String;)V",
                    &[Arg::Obj(&context.view.0), Arg::Str("drawn my way")],
                );
            },
        );
        let harness = launch();
        let tooltip = java(
            Class::Views,
            "tooltip",
            "(Landroid/view/View;)Ljava/lang/String;",
            &[Arg::Obj(&harness.expect("go"))],
        )
        .string();
        assert_eq!(tooltip.as_deref(), Some("added by a mapper"));
        assert_eq!(harness.text("count").as_deref(), Some("drawn my way"), "replaced");
        assert_eq!(crate::active_mappers().len(), 2, "the inspector lists both");
        crate::clear_mappers();
    });
}

pub(super) fn a_panicking_handler_ends_the_application_not_the_process(_: &Instrumentation) {
    on_main(|| {
        let harness = launch();
        harness.click("boom");
        harness.pump();
        let (ended, error) = harness.ended();
        assert!(ended, "the default policy ends the application");
        let error = error.unwrap_or_default();
        assert!(error.contains("a deliberate panic"), "with the panic's message: {error}");
        // The process — and the JVM — are still here: this test returns.
    });
}

pub(super) fn the_inspector_reads_the_realized_views_over_its_socket(_: &Instrumentation) {
    use rustnative_core::inspect::{Reply, Request, send_request};
    let endpoint = on_main(|| {
        let mut application = Application::new(Panel, Window::new("Inspect", Size::new(320, 240)));
        let endpoint = application.enable_inspection(None).expect("the server starts");
        super::harness::keep(Harness::launch(application));
        endpoint
    });
    // Answered on the main thread when the server wakes the looper, as a
    // request from `rustnative inspect` is.
    let reply = send_request(&endpoint, &Request::Realized { window: None }).expect("an answer");
    let Reply::Ok(value) = reply else { panic!("an answer, not {reply:?}") };
    let objects = value.as_array().cloned().unwrap_or_default();
    let go = objects.iter().find(|object| object["key"] == "go").expect("the button is listed");
    assert_eq!(go["host_type"], "android.widget.Button");
    assert!(go["handle"].as_str().is_some_and(|handle| handle.starts_with("0x")));
    let hello = send_request(&endpoint, &Request::Hello).expect("an answer");
    let Reply::Ok(hello) = hello else { panic!("hello") };
    assert_eq!(hello["backend"], "android-views");
    on_main(super::harness::stop_running);
}
