//! Messages resolved against the locale where they are shown (`PLAN.md`
//! Milestone 46): a switch re-renders exactly the components that show
//! messages; a replaced catalogue does too; the pseudo-locales lengthen and
//! accent the source text.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::sync::Arc;

use rustnative_core::i18n::{Catalogues, Message, PSEUDO_RTL_LOCALE};
use rustnative_core::inspect::{NoBackend, Reply, Request};
use rustnative_core::preview::PSEUDO_LOCALE;
use rustnative_core::{
    Application, Component, ComponentContext, ComponentTree, Event, Locale, Node, Services, Size,
    Window, keys,
};

const EN: &str =
    "title = Inbox\ninbox = { $n ->\n    [one] One message\n   *[other] { $n } messages\n}\n";
const PL: &str = "title = Skrzynka\ninbox = { $n ->\n    [one] Jedna wiadomość\n    [few] { $n } wiadomości\n   *[many] { $n } wiadomości\n}\n";
const AR: &str = "title = البريد\n";

fn catalogues() -> Arc<Catalogues> {
    Arc::new(Catalogues::parse("en", &[("en", EN), ("pl", PL), ("ar", AR)]).unwrap())
}

/// Shows messages.
struct Inbox;

impl Component for Inbox {
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
        Node::label("count", Message::new("inbox").arg("n", 3))
    }
    fn update(&mut self, _: Event) {}
}

/// Shows no message, and holds the inbox.
struct Shell;

impl Component for Shell {
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
        Node::column("shell", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let inbox = context.child::<Inbox>("inbox");
        Node::column("shell", [Node::label("static", "Always the same"), inbox])
    }
}

fn text(tree: &ComponentTree, key: &str) -> String {
    let mut found = String::new();
    tree.view().visit(&mut |node, _, _| {
        if node.id().local_key().as_deref() == Some(key) {
            if let Node::Label(label) = node {
                label.text().clone_into(&mut found);
            }
        }
    });
    found
}

#[test]
fn a_locale_switch_re_renders_only_the_components_that_show_messages() {
    let mut tree =
        ComponentTree::with_services(Shell, Services::default().with_catalogues(catalogues()));
    assert_eq!(text(&tree, "count"), "3 messages");
    tree.set_environment(&keys::LOCALE, Locale::new("pl-PL"));
    assert_eq!(text(&tree, "count"), "3 wiadomości", "the language's catalogue answers a region");
    let rendered: Vec<&str> =
        tree.last_render_log().iter().map(|record| record.path.as_str()).collect();
    assert_eq!(rendered.len(), 1, "only the inbox rendered: {rendered:?}");
    assert!(rendered[0].ends_with("/inbox"));

    tree.set_environment(&keys::LOCALE, Locale::new("ar"));
    assert_eq!(
        text(&tree, "count"),
        "3 messages",
        "a message the locale lacks falls back to the source"
    );
}

#[test]
fn a_replaced_catalogue_reaches_the_running_application() {
    let services = Services::default().with_catalogues(catalogues());
    let mut app =
        Application::with_services(Shell, Window::new("Inbox", Size::new(300, 200)), services);
    app.set_locale(Locale::new("pl"));
    let edited = PL.replace("wiadomości", "listy");
    let reply =
        app.inspect(&Request::SetCatalogue { locale: "pl".into(), ftl: edited }, &NoBackend);
    assert!(matches!(reply, Reply::Ok(_)), "{reply:?}");
    assert_eq!(text(app.components(), "count"), "3 listy");
    let rendered = app.components().last_render_log().len();
    assert_eq!(rendered, 1, "only the component showing messages");
    let broken = app.inspect(
        &Request::SetCatalogue { locale: "pl".into(), ftl: "x = { $n".into() },
        &NoBackend,
    );
    assert!(matches!(broken, Reply::Error(_)));
}

#[test]
fn the_pseudo_locales_lengthen_the_source_text_and_one_mirrors() {
    let mut tree =
        ComponentTree::with_services(Shell, Services::default().with_catalogues(catalogues()));
    tree.set_environment(&keys::LOCALE, Locale::new(PSEUDO_LOCALE));
    let pseudo = text(&tree, "count");
    assert!(pseudo.chars().count() > "3 messages".chars().count(), "{pseudo}");
    assert_ne!(pseudo, "3 messages");
    assert_eq!(Locale::new(PSEUDO_RTL_LOCALE).direction(), rustnative_core::LayoutDirection::Rtl);
}
