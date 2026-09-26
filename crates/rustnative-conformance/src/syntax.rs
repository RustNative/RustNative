//! Shared fixtures for the syntax-equivalence suite: the values every
//! spelling of every case uses, and the component a component element
//! composes.

use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, Alignment, AnimatedProperty, Color, CommandId, Component,
    Constraints, Cursor, DrawList, EdgeInsets, Event, InputInterest, ItemExtent, LayoutDirection,
    Node, Overflow, Paint, RectF, SizeMode, Transition, VirtualListStyle, VisualStyle,
};

/// The command every `command` case binds.
pub const SAVE: CommandId = CommandId::new("conformance.save");

/// A draw list for the canvas case.
#[must_use]
pub fn drawing() -> DrawList {
    DrawList::new().fill_rect(RectF::new(0.0, 0.0, 10.0, 10.0), Paint::color(Color::rgb(1, 2, 3)))
}

/// A two-by-one picture for the image case.
///
/// # Panics
///
/// Never: the pixel buffer matches the size.
#[must_use]
pub fn picture() -> rustnative_core::ImageData {
    rustnative_core::ImageData::rgba(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255], false)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// The accessibility value of the modifier case.
#[must_use]
pub fn accessibility() -> AccessibilityInfo {
    AccessibilityInfo::new(AccessibilityRole::Button).name("Save the document").focusable(true)
}

/// The visual style of the modifier case.
#[must_use]
pub fn visual() -> VisualStyle {
    VisualStyle::new().foreground(Color::rgb(10, 20, 30)).border_radius(4)
}

/// The input interest of the modifier case.
#[must_use]
pub const fn interest() -> InputInterest {
    InputInterest::new().pointer().wheel()
}

/// The transition of the modifier case.
#[must_use]
pub const fn slide() -> (AnimatedProperty, Transition) {
    (AnimatedProperty::Position, Transition::new(std::time::Duration::from_millis(150)))
}

/// The layout values of the layout case.
#[must_use]
pub const fn constraints() -> Constraints {
    Constraints::new().with_min_width(10).with_max_width(300)
}

/// A virtual list declaration.
#[must_use]
pub const fn list() -> VirtualListStyle {
    VirtualListStyle::new(1_000, ItemExtent::Fixed(24))
}

/// An application's own modifier, reached from markup through `..{f}`.
#[must_use]
pub fn emphasized(node: Node) -> Node {
    node.with_opacity(0.75).with_cursor(Cursor::Pointer)
}

/// Values every layout case uses.
pub mod values {
    use super::{Alignment, EdgeInsets, LayoutDirection, Overflow, SizeMode};

    /// A grid's tracks: a fixed label column and a filling field column.
    #[must_use]
    pub fn grid_tracks() -> rustnative_core::GridStyle {
        rustnative_core::GridStyle::new([
            rustnative_core::Track::Fixed(120),
            rustnative_core::Track::Fraction(1),
        ])
        .gap(8)
    }

    /// A fixed width.
    pub const WIDTH: SizeMode = SizeMode::Fixed(120);
    /// A fill height.
    pub const HEIGHT: SizeMode = SizeMode::Fill;
    /// A margin.
    pub const MARGIN: EdgeInsets = EdgeInsets::logical(1, 2, 3, 4);
    /// A padding.
    pub const PADDING: EdgeInsets = EdgeInsets::all(8);
    /// An alignment.
    pub const CENTER: Alignment = Alignment::Center;
    /// An overflow.
    pub const SCROLL: Overflow = Overflow::Scroll;
    /// A direction.
    pub const RTL: LayoutDirection = LayoutDirection::Rtl;
}

/// The props of [`Card`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CardProps {
    /// Its title.
    pub title: String,
    /// Whether it is highlighted.
    pub highlighted: bool,
}

/// The component a component element composes.
#[derive(Debug)]
pub struct Card {
    props: CardProps,
}

impl Component for Card {
    type Props = CardProps;
    type Message = ();
    fn new(props: CardProps) -> Self {
        Self { props }
    }
    fn props(&self) -> &CardProps {
        &self.props
    }
    fn set_props(&mut self, props: CardProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::label("title", self.props.title.clone()).disabled(!self.props.highlighted)
    }
    fn update(&mut self, _event: Event) {}
}
