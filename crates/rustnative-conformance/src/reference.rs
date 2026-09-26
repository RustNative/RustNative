//! The reference screen: one ordinary settings screen — a heading, wrapping
//! prose, a tab strip, labelled fields, and actions — that the layout
//! conformance suite checks at several text scales, pseudo-localized, and
//! mirrored (`PLAN.md` Milestone 41), and that `examples/reference-app`
//! runs on a real host for the published comparison.

use std::sync::OnceLock;

use rustnative_core::i18n::{Catalogues, Message};
use rustnative_core::layout::LayoutDirection;
use rustnative_core::preview::PSEUDO_LOCALE;
use rustnative_core::{
    Alignment, ColumnStyle, Component, EdgeInsets, Event, LayoutStyle, Node, RowStyle, SizeMode,
};

/// How the reference screen is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Variant {
    /// Every string pseudo-localized.
    pub pseudo: bool,
    /// Laid out right to left.
    pub right_to_left: bool,
}

/// The reference screen.
#[derive(Debug)]
pub struct ReferenceScreen {
    variant: Variant,
}

/// The screen's strings: a catalogue, so the pseudo-localized variant goes
/// through the same message resolution an application's does (Milestone
/// 46's pseudo-locale).
const STRINGS: &str = "heading = Account settings
intro = Changes to your name and address are saved when you choose Save. Nothing is shared until you confirm.
profile = Profile
privacy = Privacy
full-name = Full name
email = Email address
name-value = Ada Lovelace
email-value = ada@example.com
cancel = Cancel
save = Save
";

fn catalogues() -> &'static Catalogues {
    static CATALOGUES: OnceLock<Catalogues> = OnceLock::new();
    CATALOGUES.get_or_init(|| Catalogues::parse("en", &[("en", STRINGS)]).unwrap_or_default())
}

impl ReferenceScreen {
    fn text(&self, id: &'static str) -> String {
        let locale =
            rustnative_core::Locale::new(if self.variant.pseudo { PSEUDO_LOCALE } else { "en" });
        Message::new(id).format(catalogues(), &locale)
    }
}

impl Component for ReferenceScreen {
    type Props = Variant;
    type Message = ();

    fn new(variant: Variant) -> Self {
        Self { variant }
    }
    fn props(&self) -> &Variant {
        &self.variant
    }
    fn set_props(&mut self, variant: Variant) {
        self.variant = variant;
    }

    fn view(&self) -> Node {
        let field = |key: &str, label: &'static str, value: &'static str| {
            Node::column_with_layout(
                format!("{key}-row"),
                [
                    Node::label(format!("{key}-label"), self.text(label)),
                    Node::text_input(format!("{key}-input"), self.text(value)),
                ],
                LayoutStyle::default(),
                ColumnStyle::new().padding(EdgeInsets::all(0)).gap(4),
            )
        };
        let direction =
            if self.variant.right_to_left { LayoutDirection::Rtl } else { LayoutDirection::Ltr };
        Node::column_with_layout(
            "screen",
            [
                Node::label("heading", self.text("heading")),
                Node::label("intro", self.text("intro")),
                Node::tab_bar(
                    "sections",
                    [self.text("profile"), self.text("privacy")],
                    0,
                    LayoutStyle::default(),
                ),
                field("name", "full-name", "name-value"),
                field("email", "email", "email-value"),
                Node::row_with_layout(
                    "actions",
                    [
                        Node::button("cancel", self.text("cancel")),
                        Node::button("save", self.text("save")),
                    ],
                    LayoutStyle::default(),
                    RowStyle::new()
                        .padding(EdgeInsets::all(0))
                        .gap(8)
                        .align_items(Alignment::Center),
                ),
            ],
            LayoutStyle::new().height(SizeMode::Fill).direction(direction),
            ColumnStyle::new().padding(EdgeInsets::all(16)).gap(12),
        )
    }

    fn update(&mut self, _: Event) {}
}
