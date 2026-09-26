//! Milestone 46's example: one screen in English, Arabic, and Polish,
//! switched at runtime. The messages are typed functions generated from
//! `locales/*.ftl` (`rustnative_build::compile_messages`), so a missing
//! argument is a compile error. The Arabic screen is laid out right to
//! left, and the total is formatted by the host for each locale.

use rustnative_core::i18n::Message;
use rustnative_core::{
    Component, ComponentContext, Event, LayoutStyle, Locale, Node, NodeId, keys,
};

rustnative_core::messages_mod!();

/// The locales the example ships, with the name each is shown by.
pub const LOCALES: [(&str, &str); 4] =
    [("en", "English"), ("ar", "العربية"), ("pl", "Polski"), ("en-XA", "Pseudo")];

/// The application: a locale switcher over the inbox screen, whose locale
/// it provides.
pub struct Demo {
    locale: Locale,
}

impl Component for Demo {
    type Props = ();
    type Message = ();

    fn new((): ()) -> Self {
        Self { locale: Locale::new("en") }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("demo", [])
    }
    fn update(&mut self, event: Event) {
        if let Event::Click { target } = event {
            if let Some((tag, _)) =
                LOCALES.iter().find(|(tag, _)| target == NodeId::from_key(&format!("locale-{tag}")))
            {
                self.locale = Locale::new(*tag);
            }
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        // The switch reaches only what shows messages: the inbox screen.
        context.provide_env(&keys::LOCALE, self.locale.clone());
        context.provide_env(&keys::LAYOUT_DIRECTION, self.locale.direction());
        let screen = context.child::<Inbox>("inbox");
        let switcher = Node::row(
            "locales",
            LOCALES.iter().map(|(tag, name)| Node::button(format!("locale-{tag}"), *name)),
        );
        Node::column("demo", [switcher, screen])
    }
}

/// The inbox screen: every string a message.
pub struct Inbox {
    count: i64,
}

impl Component for Inbox {
    type Props = ();
    type Message = ();

    fn new((): ()) -> Self {
        Self { count: 3 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("screen", [])
    }
    fn update(&mut self, event: Event) {
        if let Event::Click { target } = event {
            if target == NodeId::from_key("more") {
                self.count += 1;
            } else if target == NodeId::from_key("fewer") {
                self.count = (self.count - 1).max(0);
            }
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let locale = context.env(&keys::LOCALE);
        #[allow(clippy::cast_precision_loss, reason = "a small count")]
        let amount = 12.5 * self.count as f64;
        let total = context.services().locale_service().format_number(&locale, amount, 2);
        Node::column_with_layout(
            "screen",
            [
                Node::label("title", messages::title()),
                Node::label("count", messages::inbox_count(self.count)),
                Node::label("invited", messages::invited("feminine", "Ada")),
                Node::label("total", messages::total(total)),
                Node::row(
                    "actions",
                    [
                        Node::button("more", messages::more()),
                        Node::button("fewer", messages::fewer()),
                    ],
                ),
            ],
            LayoutStyle::new().direction(locale.direction()),
            rustnative_core::ColumnStyle::new(),
        )
    }
}

/// A message's text in `locale` — for tests and tools outside a render.
#[must_use]
pub fn text(message: &Message, locale: &str) -> String {
    message.format(&messages::catalogues(), &Locale::new(locale))
}
