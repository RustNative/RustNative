//! The framework's declarative UI tree: [`Node`] and its per-kind payloads.
//!
//! A [`Node`] tree is what a [`crate::Component`] returns from `view`/
//! `render`. It is pure data — no native object, no reconciliation state —
//! which is what lets [`crate::reconcile::TreeSnapshot`] treat it as the
//! single source of truth to diff against on every render (see
//! `crate::reconcile` for the next stage of the pipeline).

use std::collections::HashMap;

use crate::animation::{AnimatedProperty, Transition};
use crate::command::CommandId;
use crate::control::{CalendarDate, Control};
use crate::event::AccessibilityInfo;
use crate::graphics::{DrawList, ImageData};
use crate::identity::NodeId;
use crate::input::Scalar;
use crate::input::{Cursor, InputInterest};
use crate::layout::{ColumnStyle, EdgeInsets, LayoutStyle, Overflow, RowStyle, SizeMode};
use crate::style::{ControlState, DeclarationSet, StateStyles, VisualStyle};
use crate::virtualization::{Axis, VirtualListStyle};

/// The same field of whichever node kind `$node` is.
macro_rules! node_field {
    ($node:expr, $field:ident) => {
        match $node {
            Node::Label(node) => &node.$field,
            Node::Button(node) => &node.$field,
            Node::TextInput(node) => &node.$field,
            Node::TabBar(node) => &node.$field,
            Node::Control(node) => &node.$field,
            Node::Canvas(node) => &node.$field,
            Node::Surface(node) => &node.$field,
            Node::Column(node) => &node.$field,
            Node::Row(node) => &node.$field,
        }
    };
    (mut $node:expr, $field:ident) => {
        match $node {
            Node::Label(node) => &mut node.$field,
            Node::Button(node) => &mut node.$field,
            Node::TextInput(node) => &mut node.$field,
            Node::TabBar(node) => &mut node.$field,
            Node::Control(node) => &mut node.$field,
            Node::Canvas(node) => &mut node.$field,
            Node::Surface(node) => &mut node.$field,
            Node::Column(node) => &mut node.$field,
            Node::Row(node) => &mut node.$field,
        }
    };
}

/// The framework's declarative UI tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A static, non-interactive text label.
    Label(Label),
    /// An activatable push button.
    Button(Button),
    /// An editable single-line text field.
    TextInput(TextInput),
    /// Custom drawing: a [`DrawList`] realized by the platform's 2D API
    /// (see [`crate::graphics`]).
    Canvas(Canvas),
    /// A bare platform surface an application renders into itself (see
    /// [`crate::graphics`]).
    Surface(Surface),
    /// A strip of tabs, one selected (see [`Node::tab_bar`]).
    TabBar(TabBar),
    /// A native control: a check box, slider, select, date picker, and the
    /// rest of [`Control`] (see [`Node::control`]).
    Control(ControlNode),
    /// A container that lays its children out vertically.
    Column(Column),
    /// A container that lays its children out horizontally.
    Row(Row),
}

impl Node {
    /// Creates a [`Label`] node with the default layout.
    /// Creates a static text node.
    ///
    /// `key` is the node's identity. It only has to be unique among the
    /// nodes one component renders — see [`crate::identity`] for why keys
    /// are scoped that way rather than globally.
    ///
    /// # Example
    ///
    /// ```
    /// use rustnative_core::{AccessibilityRole, Node};
    ///
    /// let label = Node::label("total", "Total: 42");
    /// // A label describes itself: static text, and not a keyboard stop.
    /// assert_eq!(label.accessibility().role(), AccessibilityRole::Label);
    /// assert!(!label.accessibility().is_focusable());
    ///
    /// // The same node in markup:
    /// let markup = rustnative_core::rsx! { <Label key="total" text="Total: 42" /> };
    /// assert_eq!(markup, label);
    /// ```
    pub fn label(key: impl AsRef<str>, text: impl Into<String>) -> Self {
        Self::Label(Label::new(NodeId::from_key(key.as_ref()), text, LayoutStyle::default()))
    }

    /// Creates a [`Label`] node with an explicit layout.
    pub fn label_with_layout(
        key: impl AsRef<str>,
        text: impl Into<String>,
        layout: LayoutStyle,
    ) -> Self {
        Self::Label(Label::new(NodeId::from_key(key.as_ref()), text, layout))
    }

    /// Creates a [`Button`] node with the default layout.
    /// Creates an activatable button.
    ///
    /// Buttons default to being keyboard-focusable and to
    /// [`AccessibilityRole::Button`]; use [`Self::with_accessibility`] where
    /// that is wrong, or where the announced name should differ from the
    /// visible caption.
    ///
    /// # Example
    ///
    /// ```
    /// use rustnative_core::{AccessibilityInfo, AccessibilityRole, Node};
    ///
    /// let submit = Node::button("submit", "Submit");
    /// assert!(submit.accessibility().is_focusable());
    ///
    /// // A decorative button, out of the tab order:
    /// let decorative = Node::button("chevron", ">")
    ///     .with_accessibility(AccessibilityInfo::new(AccessibilityRole::None));
    /// assert!(!decorative.accessibility().is_focusable());
    ///
    /// // Or one whose caption is an icon and whose announced name is not:
    /// let icon = Node::button("delete", "\u{1F5D1}").with_accessibility(
    ///     AccessibilityInfo::new(AccessibilityRole::Button)
    ///         .name("Delete this item")
    ///         .focusable(true),
    /// );
    /// assert_eq!(icon.accessibility().name_hint(), Some("Delete this item"));
    ///
    /// // The same three in markup:
    /// use rustnative_core::rsx;
    /// assert_eq!(rsx! { <Button key="submit" text="Submit" /> }, submit);
    /// assert_eq!(
    ///     rsx! { <Button key="chevron" text=">" accessibility={AccessibilityInfo::new(AccessibilityRole::None)} /> },
    ///     decorative,
    /// );
    /// let named = AccessibilityInfo::new(AccessibilityRole::Button).name("Delete this item").focusable(true);
    /// assert_eq!(rsx! { <Button key="delete" text="\u{1F5D1}" accessibility={named} /> }, icon);
    /// ```
    ///
    /// [`AccessibilityRole::Button`]: crate::AccessibilityRole::Button
    pub fn button(key: impl AsRef<str>, text: impl Into<String>) -> Self {
        Self::Button(Button::new(NodeId::from_key(key.as_ref()), text, LayoutStyle::default()))
    }

    /// Creates a [`Button`] node with an explicit layout.
    pub fn button_with_layout(
        key: impl AsRef<str>,
        text: impl Into<String>,
        layout: LayoutStyle,
    ) -> Self {
        Self::Button(Button::new(NodeId::from_key(key.as_ref()), text, layout))
    }

    /// Creates a canvas that draws `draw_list`.
    ///
    /// A canvas is a leaf: it takes part in layout like any node (give it a
    /// size — it has no intrinsic one), redraws when its draw list changes
    /// and only then, and reports pointer input with the
    /// [hit region](DrawList::hit_region) it landed in. See
    /// [`crate::graphics`] for when to reach for one.
    ///
    /// # Example
    ///
    /// ```
    /// use rustnative_core::{Color, DrawList, LayoutStyle, Node, Paint, RectF, SizeMode};
    ///
    /// let swatch = Node::canvas(
    ///     "swatch",
    ///     DrawList::new()
    ///         .fill_ellipse(RectF::new(0.0, 0.0, 40.0, 40.0), Paint::color(Color::rgb(200, 40, 40))),
    ///     LayoutStyle::new().width(SizeMode::Fixed(40)).height(SizeMode::Fixed(40)),
    /// );
    /// assert!(matches!(swatch, Node::Canvas(_)));
    ///
    /// // The same canvas in markup:
    /// let markup = rustnative_core::rsx! {
    ///     <Canvas
    ///         key="swatch"
    ///         draw_list={DrawList::new().fill_ellipse(RectF::new(0.0, 0.0, 40.0, 40.0), Paint::color(Color::rgb(200, 40, 40)))}
    ///         width={SizeMode::Fixed(40)}
    ///         height={SizeMode::Fixed(40)}
    ///     />
    /// };
    /// assert_eq!(markup, swatch);
    /// ```
    pub fn canvas(key: impl AsRef<str>, draw_list: DrawList, layout: LayoutStyle) -> Self {
        Self::Canvas(Canvas::new(NodeId::from_key(key.as_ref()), draw_list, layout))
    }

    /// Creates a native surface: a platform window the framework positions
    /// and sizes, and never paints.
    ///
    /// The application draws into it with its own GPU API, attached through
    /// the handle its backend gives out, and learns its size from
    /// [`crate::Event::SurfaceResized`]. See [`crate::graphics`].
    pub fn native_surface(key: impl AsRef<str>, layout: LayoutStyle) -> Self {
        Self::Surface(Surface::new(
            NodeId::from_key(key.as_ref()),
            SurfaceContent::Rendered,
            layout,
        ))
    }

    /// Adopts a foreign native object — a control the framework did not
    /// write — as a leaf of the tree (embedding outward, `PLAN.md`
    /// Milestone 40).
    ///
    /// `kind` names a factory the application registered on its backend
    /// (on Windows, `rustnative_windows::register_foreign`), which creates the
    /// object inside the window the framework gives it. The framework then
    /// measures it (the factory's preferred size), lays it out, clips it,
    /// and destroys it on the same rules as any object it realized itself —
    /// unless the factory declares it borrowed, when it is only hidden and
    /// handed back. Its accessibility is the object's own.
    ///
    /// ```
    /// use rustnative_core::{LayoutStyle, Node, SizeMode};
    ///
    /// let calendar = Node::foreign("date", "month-calendar", LayoutStyle::new().width(SizeMode::Auto));
    /// assert_eq!(calendar.foreign_kind(), Some("month-calendar"));
    ///
    /// // The same node in markup:
    /// let markup = rustnative_core::rsx! {
    ///     <Foreign key="date" kind="month-calendar" width={SizeMode::Auto} />
    /// };
    /// assert_eq!(markup, calendar);
    /// ```
    pub fn foreign(key: impl AsRef<str>, kind: impl Into<String>, layout: LayoutStyle) -> Self {
        Self::Surface(Surface::new(
            NodeId::from_key(key.as_ref()),
            SurfaceContent::Foreign(kind.into()),
            layout,
        ))
    }

    /// The factory kind of a [`Self::foreign`] node.
    #[must_use]
    pub fn foreign_kind(&self) -> Option<&str> {
        match self {
            Self::Surface(surface) => match surface.content() {
                SurfaceContent::Foreign(kind) => Some(kind),
                SurfaceContent::Rendered => None,
            },
            _ => None,
        }
    }

    /// Creates a strip of tabs labelled `labels`, with `selected` chosen.
    ///
    /// Realized as the platform's own tab control, so it looks, sounds (to
    /// a screen reader), and behaves like every other tab strip on the
    /// system. Choosing a tab raises [`crate::Event::TabSelected`]; the
    /// component answers by rendering the new selection, and — to keep every
    /// tab's content alive while another is shown — renders each tab's
    /// content with [`Self::hidden`] set on all but the selected one.
    ///
    /// # Example
    ///
    /// ```
    /// use rustnative_core::{LayoutStyle, Node};
    ///
    /// let tabs = Node::tab_bar("sections", ["General", "Advanced"], 1, LayoutStyle::new());
    /// let Node::TabBar(bar) = &tabs else { panic!("a tab bar") };
    /// assert_eq!(bar.tabs().selected(), 1);
    ///
    /// // The same tab bar in markup:
    /// let markup = rustnative_core::rsx! { <TabBar key="sections" labels={["General", "Advanced"]} selected=1 /> };
    /// assert_eq!(markup, tabs);
    /// ```
    pub fn tab_bar(
        key: impl AsRef<str>,
        labels: impl IntoIterator<Item = impl Into<String>>,
        selected: usize,
        layout: LayoutStyle,
    ) -> Self {
        Self::TabBar(TabBar::new(
            NodeId::from_key(key.as_ref()),
            Tabs::new(labels, selected),
            layout,
        ))
    }

    /// Returns this node's tabs, if it is a tab bar.
    #[must_use]
    pub fn tabs(&self) -> Option<&Tabs> {
        match self {
            Self::TabBar(bar) => Some(bar.tabs()),
            _ => None,
        }
    }

    /// Creates a native control (see [`crate::control`]), sized by its
    /// natural size.
    ///
    /// # Example
    ///
    /// ```
    /// use rustnative_core::{Control, Node};
    ///
    /// let remember = Node::checkbox("remember", "Remember me", true);
    /// assert_eq!(
    ///     remember.control_state(),
    ///     Some(&Control::Checkbox { label: "Remember me".into(), checked: true })
    /// );
    ///
    /// // The same control in markup:
    /// let markup = rustnative_core::rsx! { <Checkbox key="remember" label="Remember me" checked=true /> };
    /// assert_eq!(markup, remember);
    /// ```
    pub fn control(key: impl AsRef<str>, control: Control) -> Self {
        Self::control_with_layout(key, control, LayoutStyle::new())
    }

    /// Creates a native control with an explicit layout.
    pub fn control_with_layout(
        key: impl AsRef<str>,
        control: Control,
        layout: LayoutStyle,
    ) -> Self {
        let accessibility = control.accessibility();
        Self::Control(ControlNode::new(NodeId::from_key(key.as_ref()), control, layout))
            .with_accessibility(accessibility)
    }

    /// A check box (see [`Control::Checkbox`]).
    pub fn checkbox(key: impl AsRef<str>, label: impl Into<String>, checked: bool) -> Self {
        Self::control(key, Control::Checkbox { label: label.into(), checked })
    }

    /// A radio button (see [`Control::Radio`]).
    pub fn radio(key: impl AsRef<str>, label: impl Into<String>, selected: bool) -> Self {
        Self::control(key, Control::Radio { label: label.into(), selected })
    }

    /// An on/off switch (see [`Control::Toggle`]).
    pub fn toggle(key: impl AsRef<str>, label: impl Into<String>, on: bool) -> Self {
        Self::control(key, Control::Toggle { label: label.into(), on })
    }

    /// A slider over `min..=max` (see [`Control::Slider`]).
    pub fn slider(key: impl AsRef<str>, value: i64, min: i64, max: i64) -> Self {
        Self::control(key, Control::Slider { value, min, max })
    }

    /// A progress bar; `None` while how far is unknown (see
    /// [`Control::Progress`]).
    pub fn progress(key: impl AsRef<str>, percent: Option<u8>) -> Self {
        Self::control(key, Control::Progress { percent: percent.map(|p| p.min(100)) })
    }

    /// A drop-down choice among `options` (see [`Control::Select`]).
    pub fn select(
        key: impl AsRef<str>,
        options: impl IntoIterator<Item = impl Into<String>>,
        selected: Option<usize>,
    ) -> Self {
        Self::control(
            key,
            Control::Select { options: options.into_iter().map(Into::into).collect(), selected },
        )
    }

    /// A visible list to choose from (see [`Control::ListBox`]).
    pub fn list_box(
        key: impl AsRef<str>,
        items: impl IntoIterator<Item = impl Into<String>>,
        selected: Option<usize>,
    ) -> Self {
        Self::control(
            key,
            Control::ListBox { items: items.into_iter().map(Into::into).collect(), selected },
        )
    }

    /// A date picker (see [`Control::DatePicker`]).
    pub fn date_picker(key: impl AsRef<str>, date: CalendarDate) -> Self {
        Self::control(key, Control::DatePicker { date })
    }

    /// A number stepped over `min..=max` (see [`Control::Spinner`]).
    pub fn spinner(key: impl AsRef<str>, value: i64, min: i64, max: i64) -> Self {
        Self::control(key, Control::Spinner { value, min, max })
    }

    /// A horizontal rule (see [`Control::Separator`]). It has no natural
    /// width: give it `SizeMode::Fill`, or place it where the cross axis
    /// stretches.
    pub fn separator(key: impl AsRef<str>) -> Self {
        Self::control(key, Control::Separator)
    }

    /// A link (see [`Control::Link`]).
    pub fn link(key: impl AsRef<str>, text: impl Into<String>) -> Self {
        Self::control(key, Control::Link { text: text.into() })
    }

    /// Several lines of editable text (see [`Control::MultilineText`]).
    pub fn multiline_text(key: impl AsRef<str>, value: impl Into<String>) -> Self {
        Self::control(key, Control::MultilineText { value: value.into() })
    }

    /// A picture (see [`Control::Image`]).
    pub fn image(key: impl AsRef<str>, image: ImageData) -> Self {
        Self::control(key, Control::Image { image })
    }

    /// Creates a grid: children placed in `grid`'s rows and columns, each
    /// where its [`LayoutStyle::grid`] says, or in the next free cell (see
    /// [`crate::layout::GridStyle`]).
    ///
    /// ```
    /// use rustnative_core::{GridPlacement, GridStyle, LayoutStyle, Node, Track};
    ///
    /// let tracks = GridStyle::new([Track::Fixed(120), Track::Fraction(1)]).gap(8);
    /// let form = Node::grid(
    ///     "form",
    ///     tracks.clone(),
    ///     LayoutStyle::new(),
    ///     [
    ///         Node::label("name-label", "Name"),
    ///         Node::text_input("name", ""),
    ///         Node::label_with_layout("note", "Required", LayoutStyle::new().grid(GridPlacement::at(1, 1))),
    ///     ],
    /// );
    ///
    /// // The same grid in markup:
    /// let markup = rustnative_core::rsx! {
    ///     <Grid key="form" tracks={tracks}>
    ///         <Label key="name-label" text="Name" />
    ///         <TextInput key="name" value="" />
    ///         <Label key="note" text="Required" grid={GridPlacement::at(1, 1)} />
    ///     </Grid>
    /// };
    /// assert_eq!(markup, form);
    /// ```
    pub fn grid(
        key: impl AsRef<str>,
        grid: crate::layout::GridStyle,
        layout: LayoutStyle,
        children: impl IntoIterator<Item = Self>,
    ) -> Self {
        let mut column = Column::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            ColumnStyle::new().padding(EdgeInsets::all(0)).gap(0),
            layout,
        );
        column.grid = Some(grid);
        Self::Column(column)
    }

    /// Returns this node's grid tracks, if it is a grid.
    #[must_use]
    pub fn grid_style(&self) -> Option<&crate::layout::GridStyle> {
        match self {
            Self::Column(column) => column.grid(),
            _ => None,
        }
    }

    /// Returns this node's control, if it is one.
    #[must_use]
    pub fn control_state(&self) -> Option<&Control> {
        match self {
            Self::Control(node) => Some(node.control()),
            _ => None,
        }
    }

    /// Returns this node's draw list, if it is a canvas.
    #[must_use]
    pub fn draw_list(&self) -> Option<&DrawList> {
        match self {
            Self::Canvas(canvas) => Some(canvas.draw_list()),
            _ => None,
        }
    }

    /// Creates a [`TextInput`] node with the default layout.
    pub fn text_input(key: impl AsRef<str>, value: impl Into<String>) -> Self {
        Self::TextInput(TextInput::new(
            NodeId::from_key(key.as_ref()),
            value,
            LayoutStyle::default(),
        ))
    }

    /// Creates a [`TextInput`] node with an explicit layout.
    pub fn text_input_with_layout(
        key: impl AsRef<str>,
        value: impl Into<String>,
        layout: LayoutStyle,
    ) -> Self {
        Self::TextInput(TextInput::new(NodeId::from_key(key.as_ref()), value, layout))
    }

    /// Creates a [`Column`] node with the default layout and column style.
    pub fn column(key: impl AsRef<str>, children: impl IntoIterator<Item = Node>) -> Self {
        Self::Column(Column::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            ColumnStyle::default(),
            LayoutStyle::default(),
        ))
    }

    /// Creates a [`Column`] node with an explicit layout and column style.
    pub fn column_with_layout(
        key: impl AsRef<str>,
        children: impl IntoIterator<Item = Node>,
        layout: LayoutStyle,
        style: ColumnStyle,
    ) -> Self {
        Self::Column(Column::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            style,
            layout,
        ))
    }

    /// Creates a [`Row`] node with the default layout and row style.
    pub fn row(key: impl AsRef<str>, children: impl IntoIterator<Item = Node>) -> Self {
        Self::Row(Row::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            RowStyle::default(),
            LayoutStyle::default(),
        ))
    }

    /// Creates a [`Row`] node with an explicit layout and row style.
    pub fn row_with_layout(
        key: impl AsRef<str>,
        children: impl IntoIterator<Item = Node>,
        layout: LayoutStyle,
        style: RowStyle,
    ) -> Self {
        Self::Row(Row::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            style,
            layout,
        ))
    }

    /// Returns `self` with its accessibility metadata replaced.
    #[must_use]
    pub fn with_accessibility(self, accessibility: AccessibilityInfo) -> Self {
        match self {
            Self::Label(mut node) => {
                node.accessibility = accessibility;
                Self::Label(node)
            }
            Self::Button(mut node) => {
                node.accessibility = accessibility;
                Self::Button(node)
            }
            Self::TextInput(mut node) => {
                node.accessibility = accessibility;
                Self::TextInput(node)
            }
            Self::TabBar(mut node) => {
                node.accessibility = accessibility;
                Self::TabBar(node)
            }
            Self::Control(mut node) => {
                node.accessibility = accessibility;
                Self::Control(node)
            }
            Self::Canvas(mut node) => {
                node.accessibility = accessibility;
                Self::Canvas(node)
            }
            Self::Surface(mut node) => {
                node.accessibility = accessibility;
                Self::Surface(node)
            }
            Self::Column(mut node) => {
                node.accessibility = accessibility;
                Self::Column(node)
            }
            Self::Row(mut node) => {
                node.accessibility = accessibility;
                Self::Row(node)
            }
        }
    }

    /// Applies a node-level visual override. The active theme supplies any
    /// unspecified values when the backend resolves the style.
    #[must_use]
    pub fn with_style(self, style: VisualStyle) -> Self {
        match self {
            Self::Label(mut node) => {
                node.visual_style = style;
                Self::Label(node)
            }
            Self::Button(mut node) => {
                node.visual_style = style;
                Self::Button(node)
            }
            Self::TextInput(mut node) => {
                node.visual_style = style;
                Self::TextInput(node)
            }
            Self::TabBar(mut node) => {
                node.visual_style = style;
                Self::TabBar(node)
            }
            Self::Control(mut node) => {
                node.visual_style = style;
                Self::Control(node)
            }
            Self::Canvas(mut node) => {
                node.visual_style = style;
                Self::Canvas(node)
            }
            Self::Surface(mut node) => {
                node.visual_style = style;
                Self::Surface(node)
            }
            Self::Column(mut node) => {
                node.visual_style = style;
                Self::Column(node)
            }
            Self::Row(mut node) => {
                node.visual_style = style;
                Self::Row(node)
            }
        }
    }

    /// Returns this node's visual style override.
    #[must_use]
    pub fn visual_style(&self) -> &VisualStyle {
        match self {
            Self::Label(node) => &node.visual_style,
            Self::Button(node) => &node.visual_style,
            Self::TextInput(node) => &node.visual_style,
            Self::TabBar(node) => &node.visual_style,
            Self::Control(node) => &node.visual_style,
            Self::Canvas(node) => &node.visual_style,
            Self::Surface(node) => &node.visual_style,
            Self::Column(node) => &node.visual_style,
            Self::Row(node) => &node.visual_style,
        }
    }

    /// Marks this node (and, for containers, its native realization) as
    /// disabled. A disabled control stops accepting native input focus and
    /// input events, is skipped by Tab/Shift+Tab traversal, and is styled
    /// using the theme's `ControlState::Disabled` variant.
    #[must_use]
    pub fn disabled(self, disabled: bool) -> Self {
        match self {
            Self::Label(mut node) => {
                node.disabled = disabled;
                Self::Label(node)
            }
            Self::Button(mut node) => {
                node.disabled = disabled;
                Self::Button(node)
            }
            Self::TextInput(mut node) => {
                node.disabled = disabled;
                Self::TextInput(node)
            }
            Self::TabBar(mut node) => {
                node.disabled = disabled;
                Self::TabBar(node)
            }
            Self::Control(mut node) => {
                node.disabled = disabled;
                Self::Control(node)
            }
            Self::Canvas(mut node) => {
                node.disabled = disabled;
                Self::Canvas(node)
            }
            Self::Surface(mut node) => {
                node.disabled = disabled;
                Self::Surface(node)
            }
            Self::Column(mut node) => {
                node.disabled = disabled;
                Self::Column(node)
            }
            Self::Row(mut node) => {
                node.disabled = disabled;
                Self::Row(node)
            }
        }
    }

    /// Declares which advanced input (pointer, wheel, gestures, drops,
    /// gamepad) this node wants delivered to it; see [`InputInterest`] for
    /// why that is opt-in.
    #[must_use]
    pub fn with_input(mut self, input: InputInterest) -> Self {
        match &mut self {
            Self::Label(node) => node.input = input,
            Self::Button(node) => node.input = input,
            Self::TextInput(node) => node.input = input,
            Self::TabBar(node) => node.input = input,
            Self::Control(node) => node.input = input,
            Self::Canvas(node) => node.input = input,
            Self::Surface(node) => node.input = input,
            Self::Column(node) => node.input = input,
            Self::Row(node) => node.input = input,
        }
        self
    }

    /// Returns which advanced input this node wants.
    #[must_use]
    pub fn input(&self) -> InputInterest {
        match self {
            Self::Label(node) => node.input,
            Self::Button(node) => node.input,
            Self::TextInput(node) => node.input,
            Self::TabBar(node) => node.input,
            Self::Control(node) => node.input,
            Self::Canvas(node) => node.input,
            Self::Surface(node) => node.input,
            Self::Column(node) => node.input,
            Self::Row(node) => node.input,
        }
    }

    /// Declares how `property` moves when a render changes it.
    ///
    /// The backend animates from the value on screen to the new one; an
    /// interrupted transition retargets rather than jumping. See
    /// [`crate::animation`] for what animating does *not* do: cause a
    /// rerender.
    ///
    /// # Example
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use rustnative_core::{AnimatedProperty, Node, Transition};
    ///
    /// // Wherever layout puts this panel next, it slides there.
    /// let panel = Node::column("panel", []).with_transition(
    ///     AnimatedProperty::Position,
    ///     Transition::new(Duration::from_millis(150)),
    /// );
    /// assert_eq!(panel.transitions().len(), 1);
    ///
    /// // The same panel in markup:
    /// let slide = (AnimatedProperty::Position, Transition::new(Duration::from_millis(150)));
    /// let markup = rustnative_core::rsx! { <Column key="panel" transition={slide}></Column> };
    /// assert_eq!(markup, panel);
    /// ```
    #[must_use]
    pub fn with_transition(mut self, property: AnimatedProperty, transition: Transition) -> Self {
        let transitions = self.transitions_mut();
        transitions.retain(|declared| declared.property != property);
        transitions.push(NodeTransition { property, transition });
        self
    }

    /// Returns this node's declared transitions.
    #[must_use]
    pub fn transitions(&self) -> &[NodeTransition] {
        match self {
            Self::Label(node) => &node.transitions,
            Self::Button(node) => &node.transitions,
            Self::TextInput(node) => &node.transitions,
            Self::TabBar(node) => &node.transitions,
            Self::Control(node) => &node.transitions,
            Self::Canvas(node) => &node.transitions,
            Self::Surface(node) => &node.transitions,
            Self::Column(node) => &node.transitions,
            Self::Row(node) => &node.transitions,
        }
    }

    fn transitions_mut(&mut self) -> &mut Vec<NodeTransition> {
        match self {
            Self::Label(node) => &mut node.transitions,
            Self::Button(node) => &mut node.transitions,
            Self::TextInput(node) => &mut node.transitions,
            Self::TabBar(node) => &mut node.transitions,
            Self::Control(node) => &mut node.transitions,
            Self::Canvas(node) => &mut node.transitions,
            Self::Surface(node) => &mut node.transitions,
            Self::Column(node) => &mut node.transitions,
            Self::Row(node) => &mut node.transitions,
        }
    }

    /// Sets this node's opacity, from `0.0` (invisible) to `1.0`.
    ///
    /// Realized natively rather than by painting: a backend makes the
    /// node's own object translucent, so its content — including native
    /// controls it contains — fades as one.
    #[must_use]
    pub fn with_opacity(self, opacity: f32) -> Self {
        let opacity = Scalar::new(opacity.clamp(0.0, 1.0));
        match self {
            Self::Label(mut node) => {
                node.opacity = opacity;
                Self::Label(node)
            }
            Self::Button(mut node) => {
                node.opacity = opacity;
                Self::Button(node)
            }
            Self::TextInput(mut node) => {
                node.opacity = opacity;
                Self::TextInput(node)
            }
            Self::TabBar(mut node) => {
                node.opacity = opacity;
                Self::TabBar(node)
            }
            Self::Control(mut node) => {
                node.opacity = opacity;
                Self::Control(node)
            }
            Self::Canvas(mut node) => {
                node.opacity = opacity;
                Self::Canvas(node)
            }
            Self::Surface(mut node) => {
                node.opacity = opacity;
                Self::Surface(node)
            }
            Self::Column(mut node) => {
                node.opacity = opacity;
                Self::Column(node)
            }
            Self::Row(mut node) => {
                node.opacity = opacity;
                Self::Row(node)
            }
        }
    }

    /// Returns this node's opacity.
    #[must_use]
    pub fn opacity(&self) -> f32 {
        match self {
            Self::Label(node) => node.opacity.get(),
            Self::Button(node) => node.opacity.get(),
            Self::TextInput(node) => node.opacity.get(),
            Self::TabBar(node) => node.opacity.get(),
            Self::Control(node) => node.opacity.get(),
            Self::Canvas(node) => node.opacity.get(),
            Self::Surface(node) => node.opacity.get(),
            Self::Column(node) => node.opacity.get(),
            Self::Row(node) => node.opacity.get(),
        }
    }

    /// Returns whether this node is disabled.
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        match self {
            Self::Label(node) => node.disabled,
            Self::Button(node) => node.disabled,
            Self::TextInput(node) => node.disabled,
            Self::TabBar(node) => node.disabled,
            Self::Control(node) => node.disabled,
            Self::Canvas(node) => node.disabled,
            Self::Surface(node) => node.disabled,
            Self::Column(node) => node.disabled,
            Self::Row(node) => node.disabled,
        }
    }

    /// Returns this node's accessibility metadata.
    #[must_use]
    pub fn accessibility(&self) -> &AccessibilityInfo {
        match self {
            Self::Label(node) => node.accessibility(),
            Self::Button(node) => node.accessibility(),
            Self::TextInput(node) => node.accessibility(),
            Self::TabBar(node) => node.accessibility(),
            Self::Control(node) => node.accessibility(),
            Self::Canvas(node) => node.accessibility(),
            Self::Surface(node) => node.accessibility(),
            Self::Column(node) => node.accessibility(),
            Self::Row(node) => node.accessibility(),
        }
    }

    /// Mutable access for the component runtime's scoping pass, which
    /// rewrites relationship keys exactly as it rewrites node keys.
    pub(crate) fn accessibility_mut(&mut self) -> &mut AccessibilityInfo {
        match self {
            Self::Label(node) => &mut node.accessibility,
            Self::Button(node) => &mut node.accessibility,
            Self::TextInput(node) => &mut node.accessibility,
            Self::TabBar(node) => &mut node.accessibility,
            Self::Control(node) => &mut node.accessibility,
            Self::Canvas(node) => &mut node.accessibility,
            Self::Surface(node) => &mut node.accessibility,
            Self::Column(node) => &mut node.accessibility,
            Self::Row(node) => &mut node.accessibility,
        }
    }

    /// Returns this node's identity.
    #[must_use]
    pub fn id(&self) -> NodeId {
        match self {
            Self::Label(label) => label.id(),
            Self::Button(button) => button.id(),
            Self::TextInput(input) => input.id(),
            Self::TabBar(input) => input.id(),
            Self::Control(input) => input.id(),
            Self::Canvas(input) => input.id(),
            Self::Surface(input) => input.id(),
            Self::Column(column) => column.id(),
            Self::Row(row) => row.id(),
        }
    }

    /// Returns this node's kind.
    #[must_use]
    pub fn kind(&self) -> NodeKind {
        match self {
            Self::Label(_) => NodeKind::Label,
            Self::Button(_) => NodeKind::Button,
            Self::TextInput(_) => NodeKind::TextInput,
            Self::TabBar(_) => NodeKind::TabBar,
            Self::Control(_) => NodeKind::Control,
            Self::Canvas(_) => NodeKind::Canvas,
            Self::Surface(_) => NodeKind::Surface,
            Self::Column(_) => NodeKind::Column,
            Self::Row(_) => NodeKind::Row,
        }
    }

    /// Returns this node's layout style.
    #[must_use]
    pub fn layout(&self) -> LayoutStyle {
        match self {
            Self::Label(label) => label.layout(),
            Self::Button(button) => button.layout(),
            Self::TextInput(input) => input.layout(),
            Self::TabBar(input) => input.layout(),
            Self::Control(input) => input.layout(),
            Self::Canvas(input) => input.layout(),
            Self::Surface(input) => input.layout(),
            Self::Column(column) => column.layout(),
            Self::Row(row) => row.layout(),
        }
    }

    /// Returns this node's column style, if it is a [`Column`].
    #[must_use]
    pub fn column_style(&self) -> Option<ColumnStyle> {
        match self {
            Self::Column(column) => Some(column.style()),
            _ => None,
        }
    }

    /// Returns this node's row style, if it is a [`Row`].
    #[must_use]
    pub fn row_style(&self) -> Option<RowStyle> {
        match self {
            Self::Row(row) => Some(row.style()),
            _ => None,
        }
    }

    /// Creates a scrollable container that realizes only the items
    /// currently in view.
    ///
    /// The container itself is an ordinary [`Column`] (or [`Row`], for
    /// [`Axis::Horizontal`]) with [`Overflow::Scroll`] and no padding or
    /// gap — see [`crate::virtualization`] for why a virtual list is a
    /// container with an item count rather than a list runtime of its own.
    ///
    /// `children` are the items realized right now, each tagged with
    /// [`Self::with_item_index`]. A backend recomputes which items those
    /// should be as the list scrolls and reports the change as
    /// [`Event::VisibleRangeChanged`](crate::Event::VisibleRangeChanged).
    ///
    /// # Example
    ///
    /// ```
    /// use rustnative_core::{ItemExtent, Node, Overflow, VirtualListStyle, VirtualRange};
    ///
    /// let range = VirtualRange { first: 40, last_exclusive: 52 };
    /// let rows = range.indices().map(|index| {
    ///     Node::label(&format!("row-{index}"), format!("Row {index}")).with_item_index(index)
    /// });
    ///
    /// let list = Node::virtual_list(
    ///     "rows",
    ///     VirtualListStyle::new(100_000, ItemExtent::Fixed(24)),
    ///     rows,
    /// );
    ///
    /// // 100,000 items, twelve nodes.
    /// assert_eq!(list.virtualization().expect("a virtual list").item_count, 100_000);
    /// assert_eq!(list.column_style().expect("a vertical list is a column").overflow,
    ///            Overflow::Scroll);
    ///
    /// // The same list in markup:
    /// let markup = rustnative_core::rsx! {
    ///     <VirtualList
    ///         key="rows"
    ///         list={VirtualListStyle::new(100_000, ItemExtent::Fixed(24))}
    ///         height={rustnative_core::SizeMode::Fill}
    ///     >
    ///         for index in range.indices() {
    ///             <Label key={format!("row-{index}")} text={format!("Row {index}")} item_index={index} />
    ///         }
    ///     </VirtualList>
    /// };
    /// assert_eq!(markup, list);
    /// ```
    pub fn virtual_list(
        key: impl AsRef<str>,
        style: VirtualListStyle,
        children: impl IntoIterator<Item = Self>,
    ) -> Self {
        Self::virtual_list_with_layout(
            key,
            style,
            LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fill),
            children,
        )
    }

    /// Creates a virtual list with an explicit layout, for a list that
    /// should be a fixed size rather than fill its parent.
    ///
    /// A virtual list is sized by its parent, never by its items (that is
    /// what lets it hold more items than fit on screen), so an `Auto`
    /// size along its axis gives it no length at all: give it `Fill` or
    /// `Fixed`.
    pub fn virtual_list_with_layout(
        key: impl AsRef<str>,
        style: VirtualListStyle,
        layout: LayoutStyle,
        children: impl IntoIterator<Item = Self>,
    ) -> Self {
        let id = NodeId::from_key(key.as_ref());
        let children = children.into_iter().collect::<Vec<_>>();
        let mut node = match style.axis {
            Axis::Vertical => Self::Column(Column::new(
                id,
                children,
                ColumnStyle::new().padding(EdgeInsets::all(0)).gap(0).overflow(Overflow::Scroll),
                layout,
            )),
            Axis::Horizontal => Self::Row(Row::new(
                id,
                children,
                RowStyle::new().padding(EdgeInsets::all(0)).gap(0).overflow(Overflow::Scroll),
                layout,
            )),
        };
        match &mut node {
            Self::Column(column) => column.virtualization = Some(style),
            Self::Row(row) => row.virtualization = Some(style),
            _ => unreachable!("a virtual list is built as a column or a row above"),
        }
        node
    }

    /// Hides this node (and everything inside it) or shows it again.
    ///
    /// A hidden node stays in the tree — its native objects, its component
    /// state, and its tasks all live on — but takes no space in layout, is
    /// skipped by keyboard focus, and is absent from the accessibility
    /// tree. It is how a [`crate::NavigationStack`] keeps the screens below
    /// the top one alive, and how tabs keep every tab's content mounted.
    #[must_use]
    pub fn hidden(mut self, hidden: bool) -> Self {
        *self.hidden_mut() = hidden;
        self
    }

    /// Returns whether this node is hidden (see [`Self::hidden`]).
    #[must_use]
    pub fn is_hidden(&self) -> bool {
        match self {
            Self::Label(node) => node.hidden,
            Self::Button(node) => node.hidden,
            Self::TextInput(node) => node.hidden,
            Self::TabBar(node) => node.hidden,
            Self::Control(node) => node.hidden,
            Self::Canvas(node) => node.hidden,
            Self::Surface(node) => node.hidden,
            Self::Column(node) => node.hidden,
            Self::Row(node) => node.hidden,
        }
    }

    pub(crate) fn hidden_mut(&mut self) -> &mut bool {
        match self {
            Self::Label(node) => &mut node.hidden,
            Self::Button(node) => &mut node.hidden,
            Self::TextInput(node) => &mut node.hidden,
            Self::TabBar(node) => &mut node.hidden,
            Self::Control(node) => &mut node.hidden,
            Self::Canvas(node) => &mut node.hidden,
            Self::Surface(node) => &mut node.hidden,
            Self::Column(node) => &mut node.hidden,
            Self::Row(node) => &mut node.hidden,
        }
    }

    /// Declares this node's shared identity for matched-geometry
    /// transitions (`C25`): when a render removes a node with this
    /// identity and adds another with it, the new node moves and resizes
    /// from where the old one was — a thumbnail growing into a detail view.
    ///
    /// The motion is the node's [`AnimatedProperty::Position`] transition if
    /// it declares one, otherwise a short ease; with reduced motion it is
    /// instant. See [`crate::animation::matched_geometry`].
    ///
    /// ```
    /// use rustnative_core::Node;
    ///
    /// let thumbnail = Node::label("thumb-7", "Photo 7").with_shared_id("photo-7");
    /// assert_eq!(thumbnail.shared_id(), Some(rustnative_core::NodeId::from_key("photo-7")));
    ///
    /// // The same node in markup:
    /// let markup = rustnative_core::rsx! { <Label key="thumb-7" text="Photo 7" shared_id="photo-7" /> };
    /// assert_eq!(markup, thumbnail);
    /// ```
    #[must_use]
    pub fn with_shared_id(mut self, shared: impl AsRef<str>) -> Self {
        let shared = NodeId::from_key(shared.as_ref());
        match &mut self {
            Self::Label(node) => node.shared_id = Some(shared),
            Self::Button(node) => node.shared_id = Some(shared),
            Self::TextInput(node) => node.shared_id = Some(shared),
            Self::TabBar(node) => node.shared_id = Some(shared),
            Self::Control(node) => node.shared_id = Some(shared),
            Self::Canvas(node) => node.shared_id = Some(shared),
            Self::Surface(node) => node.shared_id = Some(shared),
            Self::Column(node) => node.shared_id = Some(shared),
            Self::Row(node) => node.shared_id = Some(shared),
        }
        self
    }

    /// This node's shared identity, if it declares one (see
    /// [`Node::with_shared_id`]).
    #[must_use]
    pub fn shared_id(&self) -> Option<NodeId> {
        match self {
            Self::Label(node) => node.shared_id,
            Self::Button(node) => node.shared_id,
            Self::TextInput(node) => node.shared_id,
            Self::TabBar(node) => node.shared_id,
            Self::Control(node) => node.shared_id,
            Self::Canvas(node) => node.shared_id,
            Self::Surface(node) => node.shared_id,
            Self::Column(node) => node.shared_id,
            Self::Row(node) => node.shared_id,
        }
    }

    /// Sets the pointer cursor shown over this node (see [`Cursor`]).
    #[must_use]
    pub fn with_cursor(mut self, cursor: Cursor) -> Self {
        match &mut self {
            Self::Label(node) => node.cursor = Some(cursor),
            Self::Button(node) => node.cursor = Some(cursor),
            Self::TextInput(node) => node.cursor = Some(cursor),
            Self::TabBar(node) => node.cursor = Some(cursor),
            Self::Control(node) => node.cursor = Some(cursor),
            Self::Canvas(node) => node.cursor = Some(cursor),
            Self::Surface(node) => node.cursor = Some(cursor),
            Self::Column(node) => node.cursor = Some(cursor),
            Self::Row(node) => node.cursor = Some(cursor),
        }
        self
    }

    /// The pointer cursor declared for this node, if any.
    #[must_use]
    pub fn cursor(&self) -> Option<Cursor> {
        match self {
            Self::Label(node) => node.cursor,
            Self::Button(node) => node.cursor,
            Self::TextInput(node) => node.cursor,
            Self::TabBar(node) => node.cursor,
            Self::Control(node) => node.cursor,
            Self::Canvas(node) => node.cursor,
            Self::Surface(node) => node.cursor,
            Self::Column(node) => node.cursor,
            Self::Row(node) => node.cursor,
        }
    }

    /// Binds this node to command `id` (see [`crate::command`]): activating
    /// it invokes the command, and it is disabled whenever the command is.
    #[must_use]
    pub fn with_command(mut self, id: CommandId) -> Self {
        *self.command_mut() = Some(id);
        self
    }

    /// The command this node is bound to.
    #[must_use]
    pub fn command(&self) -> Option<CommandId> {
        match self {
            Self::Label(node) => node.command,
            Self::Button(node) => node.command,
            Self::TextInput(node) => node.command,
            Self::TabBar(node) => node.command,
            Self::Control(node) => node.command,
            Self::Canvas(node) => node.command,
            Self::Surface(node) => node.command,
            Self::Column(node) => node.command,
            Self::Row(node) => node.command,
        }
    }

    fn command_mut(&mut self) -> &mut Option<CommandId> {
        match self {
            Self::Label(node) => &mut node.command,
            Self::Button(node) => &mut node.command,
            Self::TextInput(node) => &mut node.command,
            Self::TabBar(node) => &mut node.command,
            Self::Control(node) => &mut node.command,
            Self::Canvas(node) => &mut node.command,
            Self::Surface(node) => &mut node.command,
            Self::Column(node) => &mut node.command,
            Self::Row(node) => &mut node.command,
        }
    }

    /// Disables every node bound to a command the registry reports
    /// disabled — the pass that makes "disabled everywhere at once" true.
    pub(crate) fn apply_command_states(&mut self, enabled: &dyn Fn(CommandId) -> bool) {
        if let Some(id) = self.command() {
            if !enabled(id) {
                *self = std::mem::replace(self, Self::label("", "")).disabled(true);
            }
        }
        match self {
            Self::Column(node) => {
                node.children.iter_mut().for_each(|child| child.apply_command_states(enabled));
            }
            Self::Row(node) => {
                node.children.iter_mut().for_each(|child| child.apply_command_states(enabled));
            }
            _ => {}
        }
    }

    /// Returns this node's virtual-list declaration, if it is one.
    #[must_use]
    pub fn virtualization(&self) -> Option<VirtualListStyle> {
        match self {
            Self::Column(column) => column.virtualization,
            Self::Row(row) => row.virtualization,
            _ => None,
        }
    }

    /// Tags this node as the realization of item `index` of the virtual
    /// list containing it.
    ///
    /// This is what lets a list place an item at the offset its index
    /// implies rather than at the position it happens to occupy among its
    /// realized siblings, and what lets scroll anchoring find an item again
    /// after the data under it changed.
    #[must_use]
    pub fn with_item_index(mut self, index: usize) -> Self {
        match &mut self {
            Self::Label(node) => node.item_index = Some(index),
            Self::Button(node) => node.item_index = Some(index),
            Self::TextInput(node) => node.item_index = Some(index),
            Self::TabBar(node) => node.item_index = Some(index),
            Self::Control(node) => node.item_index = Some(index),
            Self::Canvas(node) => node.item_index = Some(index),
            Self::Surface(node) => node.item_index = Some(index),
            Self::Column(node) => node.item_index = Some(index),
            Self::Row(node) => node.item_index = Some(index),
        }
        self
    }

    /// Returns which item of its virtual list this node realizes.
    #[must_use]
    pub fn item_index(&self) -> Option<usize> {
        match self {
            Self::Label(node) => node.item_index,
            Self::Button(node) => node.item_index,
            Self::TextInput(node) => node.item_index,
            Self::TabBar(node) => node.item_index,
            Self::Control(node) => node.item_index,
            Self::Canvas(node) => node.item_index,
            Self::Surface(node) => node.item_index,
            Self::Column(node) => node.item_index,
            Self::Row(node) => node.item_index,
        }
    }

    /// Walks this node and every descendant depth-first, calling `visitor`
    /// with each node, its parent's id (`None` for the root), and its index
    /// among its siblings.
    pub fn visit(&self, visitor: &mut impl FnMut(&Node, Option<NodeId>, usize)) {
        self.visit_with_parent(None, 0, visitor);
    }

    /// Returns whether `target` identifies this node or any descendant.
    #[must_use]
    pub fn contains_id(&self, target: NodeId) -> bool {
        let mut found = false;
        self.visit(&mut |node, _, _| {
            if node.id() == target {
                found = true;
            }
        });
        found
    }

    fn visit_with_parent(
        &self,
        parent: Option<NodeId>,
        index: usize,
        visitor: &mut impl FnMut(&Node, Option<NodeId>, usize),
    ) {
        visitor(self, parent, index);

        match self {
            Self::Column(column) => {
                for (index, child) in column.children().iter().enumerate() {
                    child.visit_with_parent(Some(column.id()), index, visitor);
                }
            }
            Self::Row(row) => {
                for (index, child) in row.children().iter().enumerate() {
                    child.visit_with_parent(Some(row.id()), index, visitor);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn validate_unique_ids(&self) -> Result<(), TreeError> {
        let mut count = HashMap::new();
        self.visit(&mut |node, _, _| {
            *count.entry(node.id()).or_insert(0usize) += 1;
        });

        if let Some((id, _)) = count.into_iter().find(|(_, count)| *count > 1) {
            return Err(TreeError::DuplicateNodeId(id));
        }

        Ok(())
    }

    /// Used only by the component runtime while scoping a freshly-rendered
    /// view's local keys into framework-wide identities (see
    /// `crate::component::tree::scope_component_node_ids`). Application code
    /// never needs to — and cannot, since node identity is otherwise
    /// immutable once constructed — assign a node's id directly.
    pub(crate) fn set_id(&mut self, id: NodeId) {
        match self {
            Self::Label(node) => node.id = id,
            Self::Button(node) => node.id = id,
            Self::TextInput(node) => node.id = id,
            Self::TabBar(node) => node.id = id,
            Self::Control(node) => node.id = id,
            Self::Canvas(node) => node.id = id,
            Self::Surface(node) => node.id = id,
            Self::Column(node) => node.id = id,
            Self::Row(node) => node.id = id,
        }
    }
}

/// The style spellings (`PLAN.md` 2.14, Milestone 58): utility classes
/// and declaration blocks, and the typed state styles they lower beside.
impl Node {
    /// Styles this node with utility classes — Tailwind CSS v4's vocabulary,
    /// checked at compile time by [`crate::classes!`] (whose own example
    /// resolves a class string and its typed spelling side by side; this
    /// one is single-spelling: resolution happens in a render):
    ///
    /// ```
    /// use rustnative_core::{Node, classes};
    ///
    /// let save = Node::button("save", "Save").with_class(classes!("px-4 bg-blue-500 hover:bg-blue-600"));
    /// assert_eq!(save.declarations().len(), 1);
    ///
    /// // The same node in markup:
    /// let markup = rustnative_core::rsx! {
    ///     <Button key="save" text="Save" class="px-4 bg-blue-500 hover:bg-blue-600" />
    /// };
    /// assert_eq!(markup, save);
    /// ```
    ///
    /// The argument is a [`DeclarationSet`] rather than a string: a plain
    /// `&str` could not be checked until run time, and an unknown class must
    /// be a compile error (2.14's first rule), so the builder spelling is
    /// `.with_class(classes!("…"))`. The declarations are folded into the
    /// node's typed properties after each render, against the theme's
    /// tokens and the environment, and again whenever either changes. Later
    /// calls add to earlier ones; a later declaration of a property wins.
    #[must_use]
    pub fn with_class(mut self, classes: DeclarationSet) -> Self {
        node_field!(mut &mut self, declarations).push(classes);
        self
    }

    /// Styles this node with a declaration block ([`crate::styles!`]) —
    /// the same model as [`Self::with_class`], spelled as declarations.
    #[must_use]
    pub fn with_declarations(self, declarations: DeclarationSet) -> Self {
        self.with_class(declarations)
    }

    /// The class and declaration sets on this node, in the order they were
    /// added.
    #[must_use]
    pub fn declarations(&self) -> &[DeclarationSet] {
        node_field!(self, declarations)
    }

    /// Sets the style layered over this node's own while it is in `state` —
    /// the typed spelling of a state variant (`hover:bg-blue-600`).
    /// [`ControlState::Normal`] is the node's own style, so it merges into
    /// [`Self::with_style`]'s.
    #[must_use]
    pub fn with_state_style(mut self, state: ControlState, style: VisualStyle) -> Self {
        if let Some(slot) = node_field!(mut &mut self, state_styles).get_or_insert(state) {
            *slot = style;
            return self;
        }
        let merged = self.visual_style().merge(&style);
        self.with_style(merged)
    }

    /// This node's own state styles.
    #[must_use]
    pub fn state_styles(&self) -> &StateStyles {
        node_field!(self, state_styles)
    }

    pub(crate) fn state_styles_mut(&mut self) -> &mut StateStyles {
        node_field!(mut self, state_styles)
    }

    pub(crate) fn visual_style_mut(&mut self) -> &mut VisualStyle {
        node_field!(mut self, visual_style)
    }

    pub(crate) fn layout_mut(&mut self) -> &mut LayoutStyle {
        node_field!(mut self, layout)
    }

    pub(crate) fn opacity_mut(&mut self) -> &mut Scalar {
        node_field!(mut self, opacity)
    }

    /// Runs `apply` on a container's padding, gap, cross-axis alignment,
    /// and overflow; does nothing for a leaf.
    pub(crate) fn with_container_fields(
        &mut self,
        apply: impl FnOnce(&mut EdgeInsets, &mut i32, &mut crate::layout::Alignment, &mut Overflow),
    ) {
        match self {
            Self::Column(node) => {
                let style = &mut node.style;
                apply(
                    &mut style.padding,
                    &mut style.gap,
                    &mut style.align_items,
                    &mut style.overflow,
                );
            }
            Self::Row(node) => {
                let style = &mut node.style;
                apply(
                    &mut style.padding,
                    &mut style.gap,
                    &mut style.align_items,
                    &mut style.overflow,
                );
            }
            _ => {}
        }
    }

    /// A container's children; empty for a leaf.
    pub(crate) fn child_nodes(&self) -> &[Node] {
        match self {
            Self::Column(node) => node.children(),
            Self::Row(node) => node.children(),
            _ => &[],
        }
    }

    /// A container's children, mutably; empty for a leaf.
    pub(crate) fn child_nodes_mut(&mut self) -> &mut [Node] {
        match self {
            Self::Column(node) => node.children_mut(),
            Self::Row(node) => node.children_mut(),
            _ => &mut [],
        }
    }
}

/// Anything that can stand in child position: one [`Node`], or any
/// collection of them (`Vec<Node>`, `Option<Node>`, an iterator).
///
/// This is how the markup syntax's `{expr}` child accepts "a node or many"
/// without a second node kind: it only ever extends the parent's
/// `Vec<Node>`. The builder syntax does not need it — a builder call takes
/// `IntoIterator<Item = Node>` directly.
pub trait IntoChildren {
    /// Appends `self` to `children`.
    fn extend_into(self, children: &mut Vec<Node>);
}

impl IntoChildren for Node {
    fn extend_into(self, children: &mut Vec<Node>) {
        children.push(self);
    }
}

impl<I: IntoIterator<Item = Node>> IntoChildren for I {
    fn extend_into(self, children: &mut Vec<Node>) {
        children.extend(self);
    }
}

/// One declared transition: how a property moves when it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeTransition {
    /// The property it applies to.
    pub property: AnimatedProperty,
    /// How it moves.
    pub transition: Transition,
}

/// A UI node's realization kind, independent of any single node instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// See [`Node::Label`].
    Label,
    /// See [`Node::Button`].
    Button,
    /// See [`Node::TextInput`].
    TextInput,
    /// See [`Node::Canvas`].
    Canvas,
    /// See [`Node::Surface`].
    Surface,
    /// See [`Node::TabBar`].
    TabBar,
    /// See [`Node::Control`].
    Control,
    /// See [`Node::Column`].
    Column,
    /// See [`Node::Row`].
    Row,
}

/// Encodes the small amount of state genuinely shared by every leaf/
/// container node payload (`id`, `layout`, `accessibility`, `visual_style`,
/// `disabled`) without introducing a base "class" via a trait object, which
/// would give every node kind a payload-independent way to be constructed
/// with mismatched invariants. Each generated type keeps its own
/// kind-specific payload field(s) (`text`/`value`/`children`) as a plain,
/// explicit struct field instead.
macro_rules! leaf_node {
    (
        $name:ident,
        $payload_field:ident : $payload:ty,
        $payload_param:ident,
        role = $role:ident,
        focusable = $focusable:literal
    ) => {
        /// A leaf UI node (see [`Node`]).
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name {
            id: NodeId,
            $payload_field: $payload,
            layout: LayoutStyle,
            accessibility: AccessibilityInfo,
            visual_style: VisualStyle,
            disabled: bool,
            input: InputInterest,
            opacity: Scalar,
            transitions: Vec<NodeTransition>,
            item_index: Option<usize>,
            hidden: bool,
            command: Option<CommandId>,
            cursor: Option<Cursor>,
            shared_id: Option<NodeId>,
            declarations: Vec<DeclarationSet>,
            state_styles: StateStyles,
        }

        impl $name {
            fn new(id: NodeId, $payload_param: impl Into<$payload>, layout: LayoutStyle) -> Self {
                Self {
                    id,
                    $payload_field: $payload_param.into(),
                    layout,
                    // A node's accessibility metadata starts out
                    // describing what the node *is*, rather than empty.
                    //
                    // Every leaf kind used to default to
                    // `AccessibilityRole::None` and `focusable: false`,
                    // which made the portable semantic model convey nothing
                    // unless an application filled it in by hand for every
                    // single node — and since a backend can only realize
                    // what the model says, that is the root of the standards
                    // audit's P1.16 finding that accessibility "is not
                    // actually fully realized". A button is an activatable
                    // control and a keyboard stop; a label is static text
                    // and is not; a text field is both editable and a
                    // keyboard stop. Those are properties of the node kind,
                    // known here, so they are the defaults here.
                    //
                    // `with_accessibility` still replaces the whole value,
                    // so an application that wants a decorative button
                    // outside the tab order, or a label with an overridden
                    // announced name, says so explicitly.
                    accessibility: AccessibilityInfo::new(crate::event::AccessibilityRole::$role)
                        .focusable($focusable),
                    visual_style: VisualStyle::default(),
                    disabled: false,
                    input: InputInterest::new(),
                    opacity: Scalar::ONE,
                    transitions: Vec::new(),
                    item_index: None,
                    hidden: false,
                    command: None,
                    cursor: None,
                    shared_id: None,
                    declarations: Vec::new(),
                    state_styles: StateStyles::new(),
                }
            }

            /// Returns this node's identity.
            pub fn id(&self) -> NodeId {
                self.id
            }

            /// Returns this node's layout style.
            pub fn layout(&self) -> LayoutStyle {
                self.layout
            }

            /// Returns this node's accessibility metadata.
            pub fn accessibility(&self) -> &AccessibilityInfo {
                &self.accessibility
            }
        }
    };
}

leaf_node!(Label, text: String, text, role = Label, focusable = false);
leaf_node!(Button, text: String, text, role = Button, focusable = true);
leaf_node!(TextInput, value: String, value, role = TextInput, focusable = true);
leaf_node!(Canvas, draw_list: DrawList, draw_list, role = Canvas, focusable = false);
leaf_node!(Surface, content: SurfaceContent, content, role = Group, focusable = false);
leaf_node!(TabBar, tabs: Tabs, tabs, role = TabList, focusable = true);
leaf_node!(ControlNode, control: Control, control, role = Group, focusable = false);

impl ControlNode {
    /// Returns the control.
    #[must_use]
    pub fn control(&self) -> &Control {
        &self.control
    }
}

/// The labels of a [`TabBar`] and which one is selected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Tabs {
    labels: Vec<String>,
    selected: usize,
}

impl Tabs {
    /// Tabs labelled `labels`, with `selected` chosen (clamped to the last).
    #[must_use]
    pub fn new(labels: impl IntoIterator<Item = impl Into<String>>, selected: usize) -> Self {
        let labels = labels.into_iter().map(Into::into).collect::<Vec<_>>();
        let selected = selected.min(labels.len().saturating_sub(1));
        Self { labels, selected }
    }

    /// The tab labels, in order.
    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// Which tab is selected.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }
}

impl TabBar {
    /// Returns the tabs.
    #[must_use]
    pub fn tabs(&self) -> &Tabs {
        &self.tabs
    }
}

impl Label {
    /// Returns the label's text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Button {
    /// Returns the button's text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// What fills a [`Surface`] node: pixels the application renders, or a
/// native object the application supplies.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum SurfaceContent {
    /// The framework creates a bare native surface; the application renders
    /// into it ([`Node::native_surface`]).
    #[default]
    Rendered,
    /// A foreign native object — a control the framework did not write —
    /// supplied by the factory the application registered on its backend
    /// under this kind ([`Node::foreign`]).
    Foreign(String),
}

impl Surface {
    /// What fills the surface.
    #[must_use]
    pub fn content(&self) -> &SurfaceContent {
        &self.content
    }
}

impl Canvas {
    /// Returns what the canvas draws.
    #[must_use]
    pub fn draw_list(&self) -> &DrawList {
        &self.draw_list
    }
}

impl TextInput {
    /// Returns the text field's current value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// A macro-free way to keep [`Column`] and [`Row`] — the two container node
/// payloads — from drifting apart while still being distinct types (see the
/// same reasoning on `crate::layout::ColumnStyle`/`RowStyle`).
macro_rules! container_node {
    ($name:ident, $style:ty) => {
        /// A container UI node (see [`Node`]).
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name {
            id: NodeId,
            children: Vec<Node>,
            style: $style,
            layout: LayoutStyle,
            accessibility: AccessibilityInfo,
            visual_style: VisualStyle,
            disabled: bool,
            input: InputInterest,
            opacity: Scalar,
            transitions: Vec<NodeTransition>,
            item_index: Option<usize>,
            virtualization: Option<VirtualListStyle>,
            grid: Option<crate::layout::GridStyle>,
            hidden: bool,
            command: Option<CommandId>,
            cursor: Option<Cursor>,
            shared_id: Option<NodeId>,
            declarations: Vec<DeclarationSet>,
            state_styles: StateStyles,
        }

        impl $name {
            fn new(id: NodeId, children: Vec<Node>, style: $style, layout: LayoutStyle) -> Self {
                Self {
                    id,
                    children,
                    style,
                    layout,
                    accessibility: AccessibilityInfo::new(crate::event::AccessibilityRole::Group),
                    visual_style: VisualStyle::default(),
                    disabled: false,
                    input: InputInterest::new(),
                    opacity: Scalar::ONE,
                    transitions: Vec::new(),
                    item_index: None,
                    virtualization: None,
                    grid: None,
                    hidden: false,
                    command: None,
                    cursor: None,
                    shared_id: None,
                    declarations: Vec::new(),
                    state_styles: StateStyles::new(),
                }
            }

            /// Returns this node's identity.
            pub fn id(&self) -> NodeId {
                self.id
            }

            /// Returns this container's virtual-list declaration, if it has
            /// one (see [`Node::virtual_list`]).
            pub fn virtualization(&self) -> Option<VirtualListStyle> {
                self.virtualization
            }

            /// Returns this container's grid tracks, if it is a grid (see
            /// [`Node::grid`]).
            pub fn grid(&self) -> Option<&crate::layout::GridStyle> {
                self.grid.as_ref()
            }

            /// Returns this container's children.
            pub fn children(&self) -> &[Node] {
                &self.children
            }

            /// Mutable child access for the component runtime's node-id
            /// scoping pass (`crate::component::tree::scope_component_node_ids`),
            /// which must rewrite each authored node's id in place after a
            /// component's `view`/`render` returns it. Not exposed further
            /// than `pub(crate)`: application code never needs to mutate an
            /// already-constructed `Node`.
            pub(crate) fn children_mut(&mut self) -> &mut [Node] {
                &mut self.children
            }

            /// Returns this container's layout-specific style.
            pub fn style(&self) -> $style {
                self.style
            }

            /// Returns this node's layout style.
            pub fn layout(&self) -> LayoutStyle {
                self.layout
            }

            /// Returns this node's accessibility metadata.
            pub fn accessibility(&self) -> &AccessibilityInfo {
                &self.accessibility
            }
        }
    };
}

container_node!(Column, ColumnStyle);
container_node!(Row, RowStyle);

/// A structural problem detected while validating a raw [`Node`] tree's
/// identities, independent of the component runtime (see
/// `crate::component::RenderError` for the analogous, component-runtime-
/// aware error this feeds into).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// Two nodes in the same tree share a [`NodeId`].
    DuplicateNodeId(NodeId),
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateNodeId(id) => write!(f, "duplicate UI node id: {}", id.get()),
        }
    }
}

impl std::error::Error for TreeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::SizeMode;

    #[test]
    fn button_defaults_to_focusable_accessible_control() {
        let button = Node::button("go", "Go");
        assert_eq!(button.accessibility().role(), crate::event::AccessibilityRole::Button);
        assert!(
            button.accessibility().is_focusable(),
            "a button is a keyboard stop unless an application says otherwise"
        );
    }

    #[test]
    fn every_node_kind_defaults_to_the_role_that_describes_it() {
        use crate::event::AccessibilityRole;
        assert_eq!(Node::label("l", "x").accessibility().role(), AccessibilityRole::Label);
        assert_eq!(Node::button("b", "x").accessibility().role(), AccessibilityRole::Button);
        assert_eq!(Node::text_input("t", "x").accessibility().role(), AccessibilityRole::TextInput);
        assert_eq!(
            Node::column("c", [] as [Node; 0]).accessibility().role(),
            AccessibilityRole::Group
        );
        assert_eq!(
            Node::row("r", [] as [Node; 0]).accessibility().role(),
            AccessibilityRole::Group
        );
    }

    #[test]
    fn only_interactive_node_kinds_default_to_being_keyboard_stops() {
        assert!(!Node::label("l", "x").accessibility().is_focusable());
        assert!(Node::button("b", "x").accessibility().is_focusable());
        assert!(Node::text_input("t", "x").accessibility().is_focusable());
        assert!(!Node::column("c", [] as [Node; 0]).accessibility().is_focusable());
    }

    #[test]
    fn with_accessibility_still_fully_replaces_the_default() {
        let decorative = Node::button("b", "x").with_accessibility(
            AccessibilityInfo::new(crate::event::AccessibilityRole::None).focusable(false),
        );
        assert!(
            !decorative.accessibility().is_focusable(),
            "an application must be able to take a button out of the tab order"
        );
    }

    #[test]
    fn disabled_flag_round_trips() {
        let button = Node::button("go", "Go").disabled(true);
        assert!(button.is_disabled());
        let button = button.disabled(false);
        assert!(!button.is_disabled());
    }

    #[test]
    fn duplicate_local_keys_are_detected() {
        let tree = Node::column("root", [Node::label("dup", "a"), Node::label("dup", "b")]);
        assert!(matches!(tree.validate_unique_ids(), Err(TreeError::DuplicateNodeId(_))));
    }

    #[test]
    fn distinct_keys_reused_across_unrelated_subtrees_are_fine_before_scoping() {
        // `Node::label` alone (pre-component-scoping) intentionally allows
        // the same *raw* key to appear in two places that are not siblings
        // of one another structurally at this layer; scoping in
        // `crate::component::tree` is what makes reuse across independent
        // components safe. This just documents that `Node` itself performs
        // no scoping — see `component::tree::scope_component_node_ids`.
        let tree = Node::column(
            "root",
            [
                Node::column("a", [Node::label("submit", "A submit")]),
                Node::column("b", [Node::label("submit", "B submit")]),
            ],
        );
        assert!(matches!(tree.validate_unique_ids(), Err(TreeError::DuplicateNodeId(_))));
    }

    #[test]
    fn container_style_accessors_round_trip() {
        let column = Node::column_with_layout(
            "root",
            [],
            LayoutStyle::new().width(SizeMode::Fixed(10)),
            ColumnStyle::new().gap(4),
        );
        assert_eq!(column.column_style().unwrap().gap, 4);
        assert!(column.row_style().is_none());
    }
}
