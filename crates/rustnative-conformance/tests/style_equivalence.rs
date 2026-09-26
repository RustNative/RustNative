//! The style equivalence suite (`PLAN.md` Milestone 58): every documented
//! style property, spelled as a utility class and as typed properties,
//! resolves to the same node — and the conditional spellings follow the
//! environment and the theme without a re-render of their component.
//!
//! A property added to the vocabulary without a case here fails
//! `every_property_has_a_case`.

use rustnative_core::environment::keys;
use rustnative_core::layout::{Alignment, Constraints, Overflow};
use rustnative_core::style::decl::StyleProperty;
use rustnative_core::{
    Color, ColorScheme, ColumnStyle, Component, ComponentContext, ComponentTree, ControlState,
    EdgeInsets, Event, LayoutStyle, Node, PointerPrecision, SizeMode, StyleOverride, StyleValue,
    Theme, Typography, VisualStyle, classes, styles,
};

/// Renders a fixed node.
struct Show(Node);

impl Component for Show {
    type Props = Node;
    type Message = ();
    fn new(node: Node) -> Self {
        Self(node)
    }
    fn props(&self) -> &Node {
        &self.0
    }
    fn set_props(&mut self, node: Node) {
        self.0 = node;
    }
    fn view(&self) -> Node {
        self.0.clone()
    }
    fn update(&mut self, _: Event) {}
}

fn resolved(node: Node) -> Node {
    ComponentTree::new(Show(node)).view()
}

/// The typed properties of two nodes are equal (the resolved one still
/// carries its declarations; that is the only difference allowed).
#[track_caller]
fn assert_typed_eq(utility: &Node, typed: &Node) {
    assert_eq!(utility.visual_style(), typed.visual_style(), "visual style");
    assert_eq!(utility.state_styles(), typed.state_styles(), "state styles");
    assert_eq!(utility.layout(), typed.layout(), "layout");
    assert_eq!(utility.column_style(), typed.column_style(), "container style");
    assert_eq!(utility.opacity().to_bits(), typed.opacity().to_bits(), "opacity");
    assert_eq!(utility.is_hidden(), typed.is_hidden(), "visibility");
}

fn typography(size: u16, weight: u16, family: &str) -> Typography {
    Typography { family: family.to_owned(), size, weight }
}

fn label() -> Node {
    Node::label("l", "text")
}

fn column(style: ColumnStyle) -> Node {
    Node::column_with_layout("c", [], LayoutStyle::default(), style)
}

fn color(name: &str) -> Color {
    match Theme::default().tokens().resolve(&StyleValue::Token(name.to_owned().into())) {
        Some(StyleValue::Color(color)) => color,
        other => panic!("{name}: {other:?}"),
    }
}

/// Every property: (the property, its utility spelling, its declaration
/// spelling, and the typed node both must equal).
#[allow(clippy::too_many_lines, reason = "one case per property: the table is the suite")]
fn cases() -> Vec<(StyleProperty, Node, Node, Node)> {
    let d = Typography::default();
    let layout = LayoutStyle::default;
    vec![
        (
            StyleProperty::Foreground,
            label().with_class(classes!("text-red-500")),
            label().with_declarations(styles!("color: var(--color-red-500)")),
            label().with_style(VisualStyle::new().foreground(color("color-red-500"))),
        ),
        (
            StyleProperty::Background,
            label().with_class(classes!("bg-[#1e90ff]")),
            label().with_declarations(styles!("background: #1e90ff")),
            label().with_style(VisualStyle::new().background(Color::rgb(0x1e, 0x90, 0xff))),
        ),
        (
            StyleProperty::BorderColor,
            label().with_class(classes!("border-black/50")),
            label().with_declarations(styles!("border-color: rgb(0 0 0 / 0.5)")),
            label().with_style(VisualStyle::new().border(Color::rgba(0, 0, 0, 128))),
        ),
        (
            StyleProperty::BorderRadius,
            label().with_class(classes!("rounded-lg")),
            label().with_declarations(styles!("border-radius: 0.5rem")),
            label().with_style(VisualStyle::new().border_radius(8)),
        ),
        (
            StyleProperty::FontSize,
            label().with_class(classes!("text-sm")),
            label().with_declarations(styles!("font-size: 14px")),
            label().with_style(VisualStyle::new().typography(typography(14, d.weight, &d.family))),
        ),
        (
            StyleProperty::FontWeight,
            label().with_class(classes!("font-bold")),
            label().with_declarations(styles!("font-weight: bold")),
            label().with_style(VisualStyle::new().typography(typography(d.size, 700, &d.family))),
        ),
        (
            StyleProperty::FontFamily,
            label().with_class(classes!("font-mono")),
            label().with_declarations(styles!("font-family: ui-monospace, monospace")),
            label().with_style(VisualStyle::new().typography(typography(
                d.size,
                d.weight,
                "monospace",
            ))),
        ),
        (
            StyleProperty::Shadow,
            label().with_class(classes!("shadow-none")),
            label().with_declarations(styles!("box-shadow: none")),
            label().with_style(VisualStyle::new().shadow(Vec::new())),
        ),
        (
            StyleProperty::PaddingTop,
            column(ColumnStyle::new()).with_class(classes!("pt-2")),
            column(ColumnStyle::new()).with_declarations(styles!("padding-top: 8px")),
            column(ColumnStyle::new().padding(EdgeInsets {
                top: 8,
                end: 24,
                bottom: 24,
                start: 24,
            })),
        ),
        (
            StyleProperty::PaddingEnd,
            column(ColumnStyle::new()).with_class(classes!("pe-2")),
            column(ColumnStyle::new()).with_declarations(styles!("padding-right: 8px")),
            column(ColumnStyle::new().padding(EdgeInsets {
                top: 24,
                end: 8,
                bottom: 24,
                start: 24,
            })),
        ),
        (
            StyleProperty::PaddingBottom,
            label().with_class(classes!("pb-1")),
            label().with_declarations(styles!("padding-bottom: 0.25rem")),
            label().with_style(VisualStyle::new().padding(EdgeInsets {
                top: 0,
                end: 0,
                bottom: 4,
                start: 0,
            })),
        ),
        (
            StyleProperty::PaddingStart,
            column(ColumnStyle::new()).with_class(classes!("ps-0")),
            column(ColumnStyle::new()).with_declarations(styles!("padding-inline-start: 0")),
            column(ColumnStyle::new().padding(EdgeInsets {
                top: 24,
                end: 24,
                bottom: 24,
                start: 0,
            })),
        ),
        (
            StyleProperty::MarginTop,
            label().with_class(classes!("-mt-1")),
            label().with_declarations(styles!("margin-top: -4px")),
            Node::label_with_layout(
                "l",
                "text",
                layout().margin(EdgeInsets { top: -4, end: 0, bottom: 0, start: 0 }),
            ),
        ),
        (
            StyleProperty::MarginEnd,
            label().with_class(classes!("me-3")),
            label().with_declarations(styles!("margin-inline-end: 12px")),
            Node::label_with_layout(
                "l",
                "text",
                layout().margin(EdgeInsets { top: 0, end: 12, bottom: 0, start: 0 }),
            ),
        ),
        (
            StyleProperty::MarginBottom,
            label().with_class(classes!("mb-px")),
            label().with_declarations(styles!("margin-bottom: 1px")),
            Node::label_with_layout(
                "l",
                "text",
                layout().margin(EdgeInsets { top: 0, end: 0, bottom: 1, start: 0 }),
            ),
        ),
        (
            StyleProperty::MarginStart,
            label().with_class(classes!("ms-[10px]")),
            label().with_declarations(styles!("margin-left: 10px")),
            Node::label_with_layout(
                "l",
                "text",
                layout().margin(EdgeInsets { top: 0, end: 0, bottom: 0, start: 10 }),
            ),
        ),
        (
            StyleProperty::Width,
            label().with_class(classes!("w-64")),
            label().with_declarations(styles!("width: 16rem")),
            Node::label_with_layout("l", "text", layout().width(SizeMode::Fixed(256))),
        ),
        (
            StyleProperty::Height,
            label().with_class(classes!("h-full")),
            label().with_declarations(styles!("height: 100%")),
            Node::label_with_layout("l", "text", layout().height(SizeMode::Fill)),
        ),
        (
            StyleProperty::MinWidth,
            label().with_class(classes!("min-w-10")),
            label().with_declarations(styles!("min-width: 40px")),
            Node::label_with_layout(
                "l",
                "text",
                layout().constraints(Constraints::new().with_min_width(40)),
            ),
        ),
        (
            StyleProperty::MinHeight,
            label().with_class(classes!("min-h-[3px]")),
            label().with_declarations(styles!("min-height: 3px")),
            Node::label_with_layout(
                "l",
                "text",
                layout().constraints(Constraints::new().with_min_height(3)),
            ),
        ),
        (
            StyleProperty::MaxWidth,
            label().with_class(classes!("max-w-md")),
            label().with_declarations(styles!("max-width: 28rem")),
            Node::label_with_layout(
                "l",
                "text",
                layout().constraints(Constraints::new().with_max_width(448)),
            ),
        ),
        (
            StyleProperty::MaxHeight,
            label().with_class(classes!("max-h-4")),
            label().with_declarations(styles!("max-height: calc(var(--spacing) * 4)")),
            Node::label_with_layout(
                "l",
                "text",
                layout().constraints(Constraints::new().with_max_height(16)),
            ),
        ),
        (
            StyleProperty::Gap,
            column(ColumnStyle::new()).with_class(classes!("gap-2")),
            column(ColumnStyle::new()).with_declarations(styles!("gap: 8px")),
            column(ColumnStyle::new().gap(8)),
        ),
        (
            StyleProperty::AlignItems,
            column(ColumnStyle::new()).with_class(classes!("items-center")),
            column(ColumnStyle::new()).with_declarations(styles!("align-items: center")),
            column(ColumnStyle::new().align_items(Alignment::Center)),
        ),
        (
            StyleProperty::AlignSelf,
            label().with_class(classes!("self-end")),
            label().with_declarations(styles!("align-self: flex-end")),
            Node::label_with_layout("l", "text", layout().align_self(Alignment::End)),
        ),
        (
            StyleProperty::Overflow,
            column(ColumnStyle::new()).with_class(classes!("overflow-scroll")),
            column(ColumnStyle::new()).with_declarations(styles!("overflow: auto")),
            column(ColumnStyle::new().overflow(Overflow::Scroll)),
        ),
        (
            StyleProperty::Opacity,
            label().with_class(classes!("opacity-50")),
            label().with_declarations(styles!("opacity: 50%")),
            label().with_opacity(0.5),
        ),
        (
            StyleProperty::Display,
            label().with_class(classes!("hidden")),
            label().with_declarations(styles!("display: none")),
            label().hidden(true),
        ),
    ]
}

#[test]
fn every_property_resolves_equally_in_both_spellings() {
    for (property, utility, declarations, typed) in cases() {
        let utility = resolved(utility);
        let declarations = resolved(declarations);
        let typed = resolved(typed);
        println!("{property}");
        assert_typed_eq(&utility, &typed);
        assert_typed_eq(&declarations, &typed);
    }
}

#[test]
fn every_property_has_a_case() {
    let covered: Vec<StyleProperty> = cases().into_iter().map(|(property, ..)| property).collect();
    for property in StyleProperty::ALL {
        assert!(covered.contains(property), "`{property}` has no equivalence case");
    }
}

#[test]
fn a_state_variant_is_the_typed_state_style() {
    let utility =
        resolved(Node::button("b", "Go").with_class(classes!("bg-white hover:bg-[#ff0000]")));
    let typed = Node::button("b", "Go")
        .with_style(VisualStyle::new().background(Color::rgb(255, 255, 255)))
        .with_state_style(
            ControlState::Hovered,
            VisualStyle::new().background(Color::rgb(255, 0, 0)),
        );
    assert_typed_eq(&utility, &resolved(typed));
    // And the backend's resolution paints it in that state only.
    let theme = Theme::default();
    let style = |node: &Node, state| {
        let authored = StyleOverride::new(node.visual_style().clone())
            .with_states(node.state_styles().clone());
        theme.resolve(node.kind(), state, &authored).properties().background_override()
    };
    assert_eq!(style(&utility, ControlState::Hovered), Some(Color::rgb(255, 0, 0)));
    assert_eq!(style(&utility, ControlState::Normal), Some(Color::rgb(255, 255, 255)));
}

#[test]
fn environment_variants_follow_the_environment_without_re_rendering() {
    let mut tree = ComponentTree::new(Show(label().with_class(classes!(
        "text-black dark:text-white md:w-10 rtl:ms-1 motion-reduce:hidden pointer-coarse:h-12"
    ))));
    let foreground = |tree: &ComponentTree| tree.view().visual_style().foreground_override();
    assert_eq!(foreground(&tree), Some(Color::rgb(0, 0, 0)));

    tree.set_environment(&keys::COLOR_SCHEME, ColorScheme::Dark);
    assert_eq!(foreground(&tree), Some(Color::rgb(255, 255, 255)));
    assert!(
        tree.last_render_log().is_empty(),
        "a scheme switch re-resolves; it does not re-render"
    );

    tree.set_environment(&keys::WINDOW_WIDTH, 700);
    assert_eq!(tree.view().layout().width, SizeMode::Fill, "a label fills below `md`");
    tree.set_environment(&keys::WINDOW_WIDTH, 800);
    assert_eq!(tree.view().layout().width, SizeMode::Fixed(40));

    tree.set_environment(&keys::LAYOUT_DIRECTION, rustnative_core::layout::LayoutDirection::Rtl);
    assert_eq!(tree.view().layout().margin.start, 4);

    tree.set_environment(
        &keys::REDUCED_MOTION,
        rustnative_core::animation::MotionPreference::Reduced,
    );
    assert!(tree.view().is_hidden());

    tree.set_environment(&keys::POINTER, PointerPrecision::Coarse);
    assert_eq!(tree.view().layout().height, SizeMode::Fixed(48));

    tree.set_environment(&keys::COLOR_SCHEME, ColorScheme::Light);
    assert_eq!(foreground(&tree), Some(Color::rgb(0, 0, 0)));
}

#[test]
fn rem_follows_the_text_scale() {
    let mut tree = ComponentTree::new(Show(label().with_class(classes!("w-4"))));
    assert_eq!(tree.view().layout().width, SizeMode::Fixed(16));
    tree.set_environment(&keys::TEXT_SCALE, rustnative_core::input::Scalar::new(1.5));
    assert_eq!(tree.view().layout().width, SizeMode::Fixed(24));
}

#[test]
fn a_theme_switch_re_resolves_every_token() {
    let mut tree = ComponentTree::new(Show(label().with_class(classes!("bg-blue-500 p-2"))));
    assert_eq!(tree.view().visual_style().background_override(), Some(color("color-blue-500")));
    let brand = Color::rgb(1, 2, 3);
    let theme = Theme::default().with_token("color-blue-500", StyleValue::Color(brand)).with_token(
        "spacing",
        StyleValue::Length(rustnative_core::style::decl::Length::Px(
            rustnative_core::style::decl::Fixed::from_int(5),
        )),
    );
    tree.set_theme(theme).unwrap();
    let view = tree.view();
    assert_eq!(view.visual_style().background_override(), Some(brand));
    assert_eq!(view.visual_style().padding_override(), Some(EdgeInsets::all(10)));
}

/// A subtree's provided scheme decides its own `dark:` classes.
#[test]
fn provided_environment_scopes_conditions() {
    struct Inner;
    impl Component for Inner {
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
            Node::label("inner", "x").with_class(classes!("text-black dark:text-white"))
        }
        fn update(&mut self, _: Event) {}
    }
    struct Outer;
    impl Component for Outer {
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
            Node::column("outer", [])
        }
        fn update(&mut self, _: Event) {}
        fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
            context.provide_env(&keys::COLOR_SCHEME, ColorScheme::Dark);
            let inner = context.child::<Inner>("inner");
            Node::column(
                "outer",
                [Node::label("own", "y").with_class(classes!("text-black dark:text-white")), inner],
            )
        }
    }
    let tree = ComponentTree::new(Outer);
    let Node::Column(outer) = tree.view() else { panic!("a column") };
    assert_eq!(outer.children()[0].visual_style().foreground_override(), Some(Color::rgb(0, 0, 0)));
    let inner = &outer.children()[1];
    assert_eq!(inner.visual_style().foreground_override(), Some(Color::rgb(255, 255, 255)));
}
