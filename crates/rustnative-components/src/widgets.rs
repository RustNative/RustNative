//! The composite components (`PLAN.md` Milestone 48).
//!
//! Every one is a [`Component`] with a `…Props` type, so it is composed in
//! the builder syntax with `context.child_with_props` and in markup as an
//! element (`<TextField key="name" label="Name" value={name} />`). Values a
//! component changes are *bound*: the parent passes a [`Store`], the
//! component writes to it, and the parent reads it with
//! `context.select` — two-way binding with no callback plumbing. Actions
//! are commands (`rustnative_core::command`): a button bound to a command
//! invokes it, and the component that declared the command handles it.
//!
//! Every style is written against the semantic role tokens
//! (`components.css`, `docs/tokens.md`), never against a brand value.

use std::collections::BTreeSet;
use std::time::Duration;

use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, CommandId, Component, ComponentContext, Event, ImageData,
    ItemExtent, KeyCode, LayoutStyle, LiveRegion, Node, NodeId, SizeMode, Store, SuspendRule,
    VirtualListStyle, VirtualRange, classes,
};

use crate::behaviour::{ListSelection, Outcome, TreeItem, TreeNav};
use crate::idioms::{IDIOMS, Idioms};

fn clicked(event: &Event, key: &str) -> bool {
    matches!(event, Event::Click { target } if *target == NodeId::from_key(key))
}

fn store<T: 'static>(name: &str, value: T) -> Store<T> {
    Store::new(name, value)
}

macro_rules! stateless {
    ($name:ident, $props:ident) => {
        impl Component for $name {
            type Props = $props;
            type Message = ();
            fn new(props: $props) -> Self {
                Self { props }
            }
            fn props(&self) -> &$props {
                &self.props
            }
            fn set_props(&mut self, props: $props) {
                self.props = props;
            }
            fn view(&self) -> Node {
                self.render_view()
            }
            fn update(&mut self, _: Event) {}
        }
    };
}

// ---------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------

/// How prominent a button is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// The host's own button.
    #[default]
    Secondary,
    /// The one main action: the accent role.
    Primary,
    /// An action that destroys something: the danger role.
    Destructive,
}

/// A button bound to a command.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ActionButtonProps {
    /// Its text.
    pub text: String,
    /// Its prominence.
    pub variant: ButtonVariant,
    /// The command it invokes.
    pub command: Option<CommandId>,
}

/// A button bound to a command, in one of three variants.
#[derive(Debug)]
pub struct ActionButton {
    props: ActionButtonProps,
}

impl ActionButton {
    fn render_view(&self) -> Node {
        let button = Node::button("button", self.props.text.clone());
        let button = match self.props.variant {
            ButtonVariant::Secondary => button,
            ButtonVariant::Primary => {
                button.with_class(classes!("bg-accent text-on-accent font-semibold"))
            }
            ButtonVariant::Destructive => {
                button.with_class(classes!("bg-danger text-on-danger font-semibold"))
            }
        };
        match self.props.command {
            Some(command) => button.with_command(command),
            None => button,
        }
    }
}
stateless!(ActionButton, ActionButtonProps);

// ---------------------------------------------------------------------
// Fields
// ---------------------------------------------------------------------

/// A labelled text field with an error line.
#[derive(Debug, Clone, PartialEq)]
pub struct TextFieldProps {
    /// Its label.
    pub label: String,
    /// The bound text.
    pub value: Store<String>,
    /// The error to show, if any — a form's field error.
    pub error: Option<String>,
}

impl Default for TextFieldProps {
    fn default() -> Self {
        Self { label: String::new(), value: store("text", String::new()), error: None }
    }
}

/// A labelled text field bound to a store, showing an error when given.
#[derive(Debug)]
pub struct TextField {
    props: TextFieldProps,
}

impl Component for TextField {
    type Props = TextFieldProps;
    type Message = ();
    fn new(props: TextFieldProps) -> Self {
        Self { props }
    }
    fn props(&self) -> &TextFieldProps {
        &self.props
    }
    fn set_props(&mut self, props: TextFieldProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("field", [])
    }
    fn update(&mut self, event: Event) {
        if let Event::TextChanged { value, .. } = event {
            self.props.value.set(value);
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let value = context.select(&self.props.value, String::clone);
        let mut input = Node::text_input("input", value).with_accessibility(
            AccessibilityInfo::new(AccessibilityRole::TextInput)
                .focusable(true)
                .labelled_by("label")
                .required(false),
        );
        let mut children = vec![Node::label("label", self.props.label.clone())];
        if let Some(error) = &self.props.error {
            input = input.with_accessibility(
                AccessibilityInfo::new(AccessibilityRole::TextInput)
                    .focusable(true)
                    .labelled_by("label")
                    .described_by("error"),
            );
            children.push(input);
            children.push(
                Node::label("error", error.clone())
                    .with_class(classes!("text-danger"))
                    .with_accessibility(
                        AccessibilityInfo::new(AccessibilityRole::Alert).live(LiveRegion::Polite),
                    ),
            );
        } else {
            children.push(input);
        }
        Node::column("field", children)
    }
}

/// A search field bound to a query.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchFieldProps {
    /// The bound query.
    pub query: Store<String>,
    /// What it searches, for its accessible name.
    pub label: String,
}

impl Default for SearchFieldProps {
    fn default() -> Self {
        Self { query: store("query", String::new()), label: "Search".into() }
    }
}

/// A search field: a text field and a clear button.
#[derive(Debug)]
pub struct SearchField {
    props: SearchFieldProps,
}

impl Component for SearchField {
    type Props = SearchFieldProps;
    type Message = ();
    fn new(props: SearchFieldProps) -> Self {
        Self { props }
    }
    fn props(&self) -> &SearchFieldProps {
        &self.props
    }
    fn set_props(&mut self, props: SearchFieldProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::row("search", [])
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::TextChanged { value, .. } => self.props.query.set(value),
            _ if clicked(&event, "clear") => self.props.query.set(String::new()),
            Event::KeyDown { key: KeyCode::Escape, .. } => self.props.query.set(String::new()),
            _ => {}
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let query = context.select(&self.props.query, String::clone);
        let empty = query.is_empty();
        let field = Node::text_input("query", query).with_accessibility(
            AccessibilityInfo::new(AccessibilityRole::TextInput)
                .name(self.props.label.clone())
                .focusable(true),
        );
        Node::row("search", [field, Node::button("clear", "Clear").disabled(empty)])
    }
}

/// A group of radio buttons choosing one option.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioGroupProps {
    /// The group's label.
    pub label: String,
    /// The options.
    pub options: Vec<String>,
    /// The bound choice.
    pub selected: Store<usize>,
}

impl Default for RadioGroupProps {
    fn default() -> Self {
        Self { label: String::new(), options: Vec::new(), selected: store("choice", 0) }
    }
}

/// Radio buttons, one chosen, in a labelled group.
#[derive(Debug)]
pub struct RadioGroup {
    props: RadioGroupProps,
}

impl Component for RadioGroup {
    type Props = RadioGroupProps;
    type Message = ();
    fn new(props: RadioGroupProps) -> Self {
        Self { props }
    }
    fn props(&self) -> &RadioGroupProps {
        &self.props
    }
    fn set_props(&mut self, props: RadioGroupProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("group", [])
    }
    fn update(&mut self, event: Event) {
        if let Event::Toggled { target, on: true } = event {
            if let Some(index) = (0..self.props.options.len())
                .find(|index| target == NodeId::from_key(&format!("option-{index}")))
            {
                self.props.selected.set(index);
            }
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let selected = context.select(&self.props.selected, |selected| *selected);
        let count = u32::try_from(self.props.options.len()).unwrap_or(u32::MAX);
        let options = self.props.options.iter().enumerate().map(|(index, option)| {
            let radio = Node::radio(format!("option-{index}"), option.clone(), index == selected);
            let info = radio
                .accessibility()
                .clone()
                .position_in_set(u32::try_from(index + 1).unwrap_or(0), count);
            radio.with_accessibility(info)
        });
        Node::column(
            "group",
            std::iter::once(Node::label("label", self.props.label.clone())).chain(options),
        )
        .with_accessibility(
            AccessibilityInfo::new(AccessibilityRole::Group).name(self.props.label.clone()),
        )
    }
}

// ---------------------------------------------------------------------
// Containers
// ---------------------------------------------------------------------

/// A card: a titled surface.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CardProps {
    /// Its title.
    pub title: String,
    /// Its content.
    pub body: Vec<Node>,
}

/// A titled surface grouping its content.
#[derive(Debug)]
pub struct Card {
    props: CardProps,
}

impl Card {
    fn render_view(&self) -> Node {
        Node::column(
            "card",
            std::iter::once(
                Node::label("title", self.props.title.clone())
                    .with_class(classes!("font-semibold"))
                    .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Heading {
                        level: 3,
                    })),
            )
            .chain(self.props.body.iter().cloned()),
        )
        .with_class(classes!("bg-surface text-on-surface border-border rounded-lg p-3"))
        .with_accessibility(
            AccessibilityInfo::new(AccessibilityRole::Group).name(self.props.title.clone()),
        )
    }
}
stateless!(Card, CardProps);

/// How a badge reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    /// Neutral.
    #[default]
    Neutral,
    /// Needs attention.
    Accent,
    /// Something is wrong.
    Danger,
}

/// A badge.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BadgeProps {
    /// Its text.
    pub text: String,
    /// Its tone.
    pub tone: Tone,
}

/// A short status word or count.
#[derive(Debug)]
pub struct Badge {
    props: BadgeProps,
}

impl Badge {
    fn render_view(&self) -> Node {
        let label = Node::label("badge", self.props.text.clone());
        match self.props.tone {
            Tone::Neutral => label.with_class(classes!("bg-subtle text-on-surface rounded px-2")),
            Tone::Accent => label.with_class(classes!("bg-accent text-on-accent rounded px-2")),
            Tone::Danger => label.with_class(classes!("bg-danger text-on-danger rounded px-2")),
        }
    }
}
stateless!(Badge, BadgeProps);

/// An avatar.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AvatarProps {
    /// Whose it is.
    pub name: String,
    /// Their picture; without one, their initials.
    pub image: Option<ImageData>,
}

/// A person's picture, or their initials.
#[derive(Debug)]
pub struct Avatar {
    props: AvatarProps,
}

impl Avatar {
    /// The initials shown without a picture.
    #[must_use]
    pub fn initials(name: &str) -> String {
        name.split_whitespace()
            .filter_map(|word| word.chars().next())
            .take(2)
            .flat_map(char::to_uppercase)
            .collect()
    }

    fn render_view(&self) -> Node {
        let info = AccessibilityInfo::new(AccessibilityRole::Image).name(self.props.name.clone());
        match &self.props.image {
            Some(image) => Node::image("avatar", image.clone()).with_accessibility(info),
            None => Node::label("avatar", Self::initials(&self.props.name))
                .with_class(classes!("bg-accent text-on-accent rounded-full px-2 font-semibold"))
                .with_accessibility(info),
        }
    }
}
stateless!(Avatar, AvatarProps);

/// An empty state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EmptyStateProps {
    /// What is missing.
    pub title: String,
    /// What to do about it.
    pub message: String,
    /// The action that fills it, as a command and its text.
    pub action: Option<(CommandId, String)>,
}

/// What a screen shows when it has nothing to show.
#[derive(Debug)]
pub struct EmptyState {
    props: EmptyStateProps,
}

impl EmptyState {
    fn render_view(&self) -> Node {
        let mut children = vec![
            Node::label("title", self.props.title.clone())
                .with_class(classes!("font-semibold"))
                .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Heading {
                    level: 2,
                })),
            Node::label("message", self.props.message.clone()).with_class(classes!("text-muted")),
        ];
        if let Some((command, text)) = &self.props.action {
            children.push(
                Node::button("action", text.clone())
                    .with_command(*command)
                    .with_class(classes!("bg-accent text-on-accent")),
            );
        }
        Node::column("empty", children)
    }
}
stateless!(EmptyState, EmptyStateProps);

/// A progress indicator.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProgressIndicatorProps {
    /// What is in progress.
    pub label: String,
    /// How far, or `None` while unknown.
    pub percent: Option<u8>,
}

/// A labelled progress bar — how a long-running operation
/// (`rustnative_data::Operation`) is shown.
#[derive(Debug)]
pub struct ProgressIndicator {
    props: ProgressIndicatorProps,
}

impl ProgressIndicator {
    fn render_view(&self) -> Node {
        let status = self
            .props
            .percent
            .map_or_else(|| "Working…".to_owned(), |percent| format!("{percent}%"));
        Node::column(
            "progress",
            [
                Node::row(
                    "header",
                    [Node::label("label", self.props.label.clone()), Node::label("status", status)],
                ),
                Node::progress("bar", self.props.percent),
            ],
        )
    }
}
stateless!(ProgressIndicator, ProgressIndicatorProps);

/// A toolbar.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolbarProps {
    /// Its buttons: a command and its text each.
    pub items: Vec<(CommandId, String)>,
}

/// A row of command buttons, one keyboard stop, arrows moving along it.
#[derive(Debug)]
pub struct Toolbar {
    props: ToolbarProps,
}

impl Toolbar {
    fn render_view(&self) -> Node {
        Node::row(
            "toolbar",
            self.props.items.iter().enumerate().map(|(index, (command, text))| {
                Node::button(format!("tool-{index}"), text.clone()).with_command(*command)
            }),
        )
        .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Toolbar))
    }
}
stateless!(Toolbar, ToolbarProps);

// ---------------------------------------------------------------------
// Dialog and toast
// ---------------------------------------------------------------------

/// A dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct DialogProps {
    /// Its title.
    pub title: String,
    /// Its message.
    pub message: String,
    /// The confirming action.
    pub confirm: (CommandId, String),
    /// The cancelling action.
    pub cancel: (CommandId, String),
    /// Whether confirming destroys something.
    pub destructive: bool,
}

impl Default for DialogProps {
    fn default() -> Self {
        Self {
            title: String::new(),
            message: String::new(),
            confirm: (CommandId::new("rustnative.dialog.confirm"), "OK".into()),
            cancel: (CommandId::new("rustnative.dialog.cancel"), "Cancel".into()),
            destructive: false,
        }
    }
}

/// A dialog laid out by the host's idioms (`docs/idioms/windows.md`): its
/// buttons in the host's order, its destructive action marked, Escape
/// cancelling.
#[derive(Debug)]
pub struct Dialog {
    props: DialogProps,
    idioms: Idioms,
}

impl Component for Dialog {
    type Props = DialogProps;
    type Message = ();
    fn new(props: DialogProps) -> Self {
        Self { props, idioms: Idioms::default() }
    }
    fn props(&self) -> &DialogProps {
        &self.props
    }
    fn set_props(&mut self, props: DialogProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        let (confirm_command, confirm_text) = &self.props.confirm;
        let (cancel_command, cancel_text) = &self.props.cancel;
        let confirm = Node::button("confirm", confirm_text.clone()).with_command(*confirm_command);
        let confirm = if self.props.destructive {
            confirm.with_class(classes!("bg-danger text-on-danger"))
        } else {
            confirm.with_class(classes!("bg-accent text-on-accent"))
        };
        let cancel = Node::button("cancel", cancel_text.clone()).with_command(*cancel_command);
        let buttons = if self.idioms.confirm_first { [confirm, cancel] } else { [cancel, confirm] };
        Node::column(
            "dialog",
            [
                Node::label("title", self.props.title.clone())
                    .with_class(classes!("font-semibold"))
                    .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Heading {
                        level: 2,
                    })),
                Node::label("message", self.props.message.clone()),
                Node::row("buttons", buttons),
            ],
        )
        .with_class(classes!("bg-surface text-on-surface border-border p-4"))
        .with_accessibility(
            AccessibilityInfo::new(AccessibilityRole::Dialog)
                .name(self.props.title.clone())
                .described_by("message"),
        )
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        self.idioms = context.env(&IDIOMS);
        if self.idioms.escape_dismisses {
            // Escape invokes the cancel command wherever focus is inside.
            let (cancel, _) = self.props.cancel;
            context.command(rustnative_core::Command::new(cancel, "Cancel").shortcut(
                rustnative_core::Shortcut::new(
                    KeyCode::Escape,
                    rustnative_core::KeyModifiers::default(),
                ),
            ));
        }
        self.view()
    }
}

/// A toast.
#[derive(Debug, Clone, PartialEq)]
pub struct ToastProps {
    /// The bound message; `None` hides the toast.
    pub message: Store<Option<String>>,
    /// How long it shows before dismissing itself.
    pub duration: Duration,
}

impl Default for ToastProps {
    fn default() -> Self {
        Self { message: store("toast", None), duration: Duration::from_secs(4) }
    }
}

/// A brief message that dismisses itself, announced politely.
#[derive(Debug)]
pub struct Toast {
    props: ToastProps,
    shown: Option<String>,
}

impl Component for Toast {
    type Props = ToastProps;
    type Message = String;
    fn new(props: ToastProps) -> Self {
        Self { props, shown: None }
    }
    fn props(&self) -> &ToastProps {
        &self.props
    }
    fn set_props(&mut self, props: ToastProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("toast", [])
    }
    fn update(&mut self, event: Event) {
        if clicked(&event, "dismiss") {
            self.props.message.set(None);
        }
    }
    fn message(&mut self, expired: String) {
        // Only the message this timer was started for.
        if self.props.message.get().as_deref() == Some(expired.as_str()) {
            self.props.message.set(None);
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, String>) -> Node {
        let message = context.select(&self.props.message, Clone::clone);
        if message != self.shown {
            self.shown.clone_from(&message);
            if let Some(text) = message.clone() {
                let scheduler = context.task_scope().scheduler().clone();
                let duration = self.props.duration;
                context.task_scope().spawn_with(SuspendRule::Defer, async move {
                    scheduler.sleep(duration).await;
                    text
                });
            }
        }
        match message {
            Some(text) => Node::row(
                "toast",
                [Node::label("message", text), Node::button("dismiss", "Dismiss")],
            )
            .with_class(classes!("bg-on-surface text-surface rounded p-2"))
            .with_accessibility(
                AccessibilityInfo::new(AccessibilityRole::Status).live(LiveRegion::Polite),
            ),
            None => Node::column("toast", []).hidden(true),
        }
    }
}

// ---------------------------------------------------------------------
// Tabs, lists, tables, trees
// ---------------------------------------------------------------------

/// A tab view.
#[derive(Debug, Clone, PartialEq)]
pub struct TabViewProps {
    /// The tab labels.
    pub labels: Vec<String>,
    /// The page of each tab.
    pub pages: Vec<Node>,
    /// The bound selected tab.
    pub selected: Store<usize>,
}

impl Default for TabViewProps {
    fn default() -> Self {
        Self { labels: Vec::new(), pages: Vec::new(), selected: store("tab", 0) }
    }
}

/// A tab bar over pages, every page kept alive, only the selected one
/// shown (so a hidden page's tasks are suspended).
#[derive(Debug)]
pub struct TabView {
    props: TabViewProps,
}

impl Component for TabView {
    type Props = TabViewProps;
    type Message = ();
    fn new(props: TabViewProps) -> Self {
        Self { props }
    }
    fn props(&self) -> &TabViewProps {
        &self.props
    }
    fn set_props(&mut self, props: TabViewProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("tabs", [])
    }
    fn update(&mut self, event: Event) {
        if let Event::TabSelected { index, .. } = event {
            self.props.selected.set(index);
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let selected = context.select(&self.props.selected, |selected| *selected);
        let pages = self.props.pages.iter().enumerate().map(|(index, page)| {
            Node::column(format!("page-{index}"), [page.clone()])
                .hidden(index != selected)
                .with_accessibility(AccessibilityInfo::new(AccessibilityRole::TabPanel))
        });
        Node::column(
            "tabs",
            std::iter::once(Node::tab_bar(
                "bar",
                self.props.labels.clone(),
                selected,
                LayoutStyle::new(),
            ))
            .chain(pages),
        )
    }
}

/// A list.
#[derive(Debug, Clone, PartialEq)]
pub struct ListViewProps {
    /// The items, in order.
    pub items: Vec<String>,
    /// Section headers, each before the item index it names.
    pub sections: Vec<(usize, String)>,
    /// The bound selection.
    pub selected: Store<Option<usize>>,
}

impl Default for ListViewProps {
    fn default() -> Self {
        Self { items: Vec::new(), sections: Vec::new(), selected: store("selection", None) }
    }
}

/// A virtualized, sectioned list with keyboard selection
/// ([`ListSelection`]): only the visible rows exist.
#[derive(Debug)]
pub struct ListView {
    props: ListViewProps,
    behaviour: ListSelection,
    range: VirtualRange,
}

/// One row of a sectioned list: a header or an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListRow {
    Header(usize),
    Item(usize),
}

impl ListView {
    fn rows(&self) -> Vec<ListRow> {
        let mut rows = Vec::with_capacity(self.props.items.len() + self.props.sections.len());
        let mut sections = self.props.sections.iter().enumerate().peekable();
        for item in 0..self.props.items.len() {
            while let Some((section, _)) = sections.next_if(|(_, (at, _))| *at <= item) {
                rows.push(ListRow::Header(section));
            }
            rows.push(ListRow::Item(item));
        }
        rows
    }
}

impl Component for ListView {
    type Props = ListViewProps;
    type Message = ();
    fn new(props: ListViewProps) -> Self {
        let behaviour = ListSelection::single(props.items.len());
        Self { props, behaviour, range: VirtualRange::EMPTY }
    }
    fn props(&self) -> &ListViewProps {
        &self.props
    }
    fn set_props(&mut self, props: ListViewProps) {
        self.behaviour.set_count(props.items.len());
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("list", [])
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::VisibleRangeChanged { range, .. } => self.range = range,
            Event::KeyDown { key, modifiers, .. } => {
                if self.behaviour.key(key, modifiers) == Outcome::Selected {
                    self.props.selected.set(self.behaviour.selected().first().copied());
                }
            }
            Event::Click { target } => {
                if let Some(index) = (0..self.props.items.len())
                    .find(|index| target == NodeId::from_key(&format!("item-{index}")))
                {
                    self.behaviour.click(index, rustnative_core::KeyModifiers::default());
                    self.props.selected.set(Some(index));
                }
            }
            _ => {}
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let selected = context.select(&self.props.selected, |selected| *selected);
        let rows = self.rows();
        let count = u32::try_from(self.props.items.len()).unwrap_or(u32::MAX);
        let visible = self.range.indices().filter_map(|row| {
            let node = match rows.get(row)? {
                ListRow::Header(section) => Node::label(
                    format!("section-{section}"),
                    self.props.sections[*section].1.clone(),
                )
                .with_class(classes!("font-semibold text-muted"))
                .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Heading {
                    level: 3,
                })),
                ListRow::Item(item) => {
                    Node::button(format!("item-{item}"), self.props.items[*item].clone())
                        .with_accessibility(
                            AccessibilityInfo::new(AccessibilityRole::ListItem)
                                .name(self.props.items[*item].clone())
                                .selected(selected == Some(*item))
                                .position_in_set(u32::try_from(item + 1).unwrap_or(0), count)
                                .focusable(true),
                        )
                }
            };
            Some(node.with_item_index(row))
        });
        Node::virtual_list(
            "list",
            VirtualListStyle::new(rows.len(), ItemExtent::Fixed(28)),
            visible,
        )
        .with_accessibility(AccessibilityInfo::new(AccessibilityRole::List).focusable(true))
    }
}

/// How one section of a [`SectionedView`] lays its items out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionLayout {
    /// One item per row.
    List,
    /// Items in this many equal columns.
    Grid(u16),
    /// Items in one row that scrolls sideways.
    Carousel,
}

/// One section: a header and its items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The header.
    pub title: String,
    /// How its items are laid out.
    pub layout: SectionLayout,
    /// The items.
    pub items: Vec<String>,
}

/// A [`SectionedView`]'s sections.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SectionedViewProps {
    /// The sections, in order.
    pub sections: Vec<Section>,
}

/// A compositional list (`C26`): each section has a header and its own
/// layout — a list, a grid, or a carousel — and the sections are the items
/// of a virtual list, so only the sections on screen exist.
#[derive(Debug)]
pub struct SectionedView {
    props: SectionedViewProps,
    range: VirtualRange,
}

impl SectionedView {
    fn section(index: usize, section: &Section) -> Node {
        let item = |position: usize, text: &String| {
            Node::button(format!("s{index}-{position}"), text.clone()).with_accessibility(
                AccessibilityInfo::new(AccessibilityRole::ListItem)
                    .name(text.clone())
                    .focusable(true),
            )
        };
        let items = section.items.iter().enumerate().map(|(position, text)| item(position, text));
        let body = match section.layout {
            SectionLayout::List => Node::column(format!("s{index}-body"), items),
            SectionLayout::Grid(columns) => Node::grid(
                format!("s{index}-body"),
                rustnative_core::GridStyle::new(
                    (0..columns.max(1)).map(|_| rustnative_core::Track::Fraction(1)),
                )
                .gap(8),
                LayoutStyle::new().width(SizeMode::Fill),
                items,
            ),
            SectionLayout::Carousel => Node::row_with_layout(
                format!("s{index}-body"),
                items,
                LayoutStyle::new().width(SizeMode::Fill),
                rustnative_core::RowStyle::new().gap(8).overflow(rustnative_core::Overflow::Scroll),
            ),
        };
        Node::column(
            format!("section-{index}"),
            [
                Node::label(format!("s{index}-title"), section.title.clone())
                    .with_class(classes!("font-semibold text-muted"))
                    .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Heading {
                        level: 3,
                    })),
                body.with_accessibility(AccessibilityInfo::new(AccessibilityRole::List)),
            ],
        )
        .with_item_index(index)
    }
}

impl Component for SectionedView {
    type Props = SectionedViewProps;
    type Message = ();
    fn new(props: SectionedViewProps) -> Self {
        Self { props, range: VirtualRange::EMPTY }
    }
    fn props(&self) -> &SectionedViewProps {
        &self.props
    }
    fn set_props(&mut self, props: SectionedViewProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        let visible = self
            .range
            .indices()
            .filter_map(|index| Some(Self::section(index, self.props.sections.get(index)?)));
        Node::virtual_list(
            "sections",
            VirtualListStyle::new(self.props.sections.len(), ItemExtent::Estimated(160)),
            visible,
        )
    }
    fn update(&mut self, event: Event) {
        if let Event::VisibleRangeChanged { range, .. } = event {
            self.range = range;
        }
    }
}

/// How a table is sorted: a column and whether ascending.
pub type Sort = Option<(usize, bool)>;

/// A data table.
#[derive(Debug, Clone, PartialEq)]
pub struct DataTableProps {
    /// The column headers.
    pub columns: Vec<String>,
    /// The rows, one cell per column.
    pub rows: Vec<Vec<String>>,
    /// The bound sort order.
    pub sort: Store<Sort>,
}

impl Default for DataTableProps {
    fn default() -> Self {
        Self { columns: Vec::new(), rows: Vec::new(), sort: store("sort", None) }
    }
}

/// A virtualized table whose header buttons sort it. Sorting is a view —
/// an order of row indices — and never copies the rows.
#[derive(Debug)]
pub struct DataTable {
    props: DataTableProps,
    range: VirtualRange,
}

/// The order `rows` are shown in under `sort`: numbers compare as numbers.
#[must_use]
pub fn sorted_order(rows: &[Vec<String>], sort: Sort) -> Vec<usize> {
    let mut order = (0..rows.len()).collect::<Vec<_>>();
    if let Some((column, ascending)) = sort {
        let key = |row: usize| rows[row].get(column).map_or("", String::as_str);
        order.sort_by(|a, b| {
            let (a, b) = (key(*a), key(*b));
            let ordering = match (a.parse::<f64>(), b.parse::<f64>()) {
                (Ok(a), Ok(b)) => a.total_cmp(&b),
                _ => a.to_lowercase().cmp(&b.to_lowercase()),
            };
            if ascending { ordering } else { ordering.reverse() }
        });
    }
    order
}

impl Component for DataTable {
    type Props = DataTableProps;
    type Message = ();
    fn new(props: DataTableProps) -> Self {
        Self { props, range: VirtualRange::EMPTY }
    }
    fn props(&self) -> &DataTableProps {
        &self.props
    }
    fn set_props(&mut self, props: DataTableProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("table", [])
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::VisibleRangeChanged { range, .. } => self.range = range,
            Event::Click { target } => {
                if let Some(column) = (0..self.props.columns.len())
                    .find(|column| target == NodeId::from_key(&format!("header-{column}")))
                {
                    self.props.sort.update(move |sort| {
                        *sort = match *sort {
                            Some((current, true)) if current == column => Some((column, false)),
                            _ => Some((column, true)),
                        };
                    });
                }
            }
            _ => {}
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let sort = context.select(&self.props.sort, |sort| *sort);
        let order = sorted_order(&self.props.rows, sort);
        let headers = self.props.columns.iter().enumerate().map(|(column, name)| {
            let marker = match sort {
                Some((sorted, true)) if sorted == column => " ▲",
                Some((sorted, false)) if sorted == column => " ▼",
                _ => "",
            };
            Node::button_with_layout(
                format!("header-{column}"),
                format!("{name}{marker}"),
                LayoutStyle::new().width(SizeMode::Fixed(120)),
            )
        });
        let rows = self.range.indices().filter_map(|shown| {
            let row = *order.get(shown)?;
            let cells = self.props.rows[row].iter().enumerate().map(|(column, cell)| {
                Node::label_with_layout(
                    format!("cell-{row}-{column}"),
                    cell.clone(),
                    LayoutStyle::new().width(SizeMode::Fixed(120)),
                )
                .with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Cell).name(cell.clone()),
                )
            });
            Some(Node::row(format!("row-{row}"), cells).with_item_index(shown))
        });
        Node::column(
            "table",
            [
                Node::row("headers", headers),
                Node::virtual_list(
                    "rows",
                    VirtualListStyle::new(order.len(), ItemExtent::Fixed(26)),
                    rows,
                ),
            ],
        )
        .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Table))
    }
}

/// A tree view.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TreeViewProps {
    /// The items, parents before children.
    pub items: Vec<TreeItem>,
}

/// A tree with keyboard navigation ([`TreeNav`]), each visible item a row.
#[derive(Debug)]
pub struct TreeView {
    props: TreeViewProps,
    nav: TreeNav,
}

impl Component for TreeView {
    type Props = TreeViewProps;
    type Message = ();
    fn new(props: TreeViewProps) -> Self {
        let nav = TreeNav::new(props.items.clone());
        Self { props, nav }
    }
    fn props(&self) -> &TreeViewProps {
        &self.props
    }
    fn set_props(&mut self, props: TreeViewProps) {
        self.nav = TreeNav::new(props.items.clone());
        self.props = props;
    }
    fn view(&self) -> Node {
        let focused = self.nav.focused().map(str::to_owned);
        let expanded: &BTreeSet<String> = self.nav.expanded();
        let rows = self.nav.visible().into_iter().map(|(item, depth)| {
            let has_children = self
                .props
                .items
                .iter()
                .any(|child| child.parent.as_deref() == Some(item.id.as_str()));
            let marker = if !has_children {
                "  "
            } else if expanded.contains(&item.id) {
                "▾ "
            } else {
                "▸ "
            };
            let indent = "    ".repeat(depth);
            let mut info = AccessibilityInfo::new(AccessibilityRole::TreeItem)
                .name(item.label.clone())
                .selected(focused.as_deref() == Some(item.id.as_str()))
                .focusable(true);
            if has_children {
                info = info.expanded(expanded.contains(&item.id));
            }
            Node::button(format!("node-{}", item.id), format!("{indent}{marker}{}", item.label))
                .with_accessibility(info)
        });
        Node::column("tree", rows.collect::<Vec<_>>())
            .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Tree).focusable(true))
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::KeyDown { key, .. } => {
                self.nav.key(key);
            }
            Event::Click { target } => {
                if let Some(item) = self
                    .props
                    .items
                    .iter()
                    .find(|item| target == NodeId::from_key(&format!("node-{}", item.id)))
                {
                    let id = item.id.clone();
                    self.nav.toggle(&id);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------
// Command palette and adaptive navigation
// ---------------------------------------------------------------------

/// A command palette.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandPaletteProps {
    /// The commands it offers, with their names.
    pub commands: Vec<(CommandId, String)>,
    /// The bound query.
    pub query: Store<String>,
}

impl Default for CommandPaletteProps {
    fn default() -> Self {
        Self { commands: Vec::new(), query: store("palette", String::new()) }
    }
}

/// A searchable list of commands (`C20-3`): typing filters by name, and
/// choosing one invokes it.
#[derive(Debug)]
pub struct CommandPalette {
    props: CommandPaletteProps,
}

/// The commands whose names contain every word of `query`, ignoring case.
#[must_use]
pub fn palette_matches(commands: &[(CommandId, String)], query: &str) -> Vec<usize> {
    let words = query.to_lowercase().split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    (0..commands.len())
        .filter(|index| {
            let name = commands[*index].1.to_lowercase();
            words.iter().all(|word| name.contains(word.as_str()))
        })
        .collect()
}

impl Component for CommandPalette {
    type Props = CommandPaletteProps;
    type Message = ();
    fn new(props: CommandPaletteProps) -> Self {
        Self { props }
    }
    fn props(&self) -> &CommandPaletteProps {
        &self.props
    }
    fn set_props(&mut self, props: CommandPaletteProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("palette", [])
    }
    fn update(&mut self, event: Event) {
        if let Event::TextChanged { value, .. } = event {
            self.props.query.set(value);
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let query = context.select(&self.props.query, String::clone);
        let matches = palette_matches(&self.props.commands, &query);
        let results = matches.iter().map(|index| {
            let (command, name) = &self.props.commands[*index];
            Node::button(format!("command-{index}"), name.clone()).with_command(*command)
        });
        Node::column(
            "palette",
            [
                Node::text_input("query", query).with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::TextInput)
                        .name("Command")
                        .focusable(true),
                ),
                Node::column("results", results.collect::<Vec<_>>())
                    .with_accessibility(AccessibilityInfo::new(AccessibilityRole::List)),
            ],
        )
        .with_accessibility(
            AccessibilityInfo::new(AccessibilityRole::Dialog).name("Command palette"),
        )
    }
}

/// Which navigation arrangement an [`AdaptiveNavigation`] uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationStyle {
    /// A bar of icons along the bottom: compact widths.
    BottomBar,
    /// A narrow rail at the side: regular widths.
    Rail,
    /// A labelled sidebar: expanded widths.
    Sidebar,
}

/// Adaptive navigation.
#[derive(Debug, Clone, PartialEq)]
pub struct AdaptiveNavigationProps {
    /// The destinations.
    pub destinations: Vec<String>,
    /// The bound current destination.
    pub selected: Store<usize>,
    /// The content shown for the current destination.
    pub content: Vec<Node>,
}

impl Default for AdaptiveNavigationProps {
    fn default() -> Self {
        Self { destinations: Vec::new(), selected: store("destination", 0), content: Vec::new() }
    }
}

/// Navigation that is a bottom bar, a rail, or a sidebar by the size class
/// of the space it was given (`C22-3`), not the window's.
#[derive(Debug)]
pub struct AdaptiveNavigation {
    props: AdaptiveNavigationProps,
}

impl AdaptiveNavigation {
    /// The arrangement for a container `width` pixels wide.
    #[must_use]
    pub fn style_for(width: u32) -> NavigationStyle {
        match width {
            0..600 => NavigationStyle::BottomBar,
            600..840 => NavigationStyle::Rail,
            _ => NavigationStyle::Sidebar,
        }
    }
}

impl Component for AdaptiveNavigation {
    type Props = AdaptiveNavigationProps;
    type Message = ();
    fn new(props: AdaptiveNavigationProps) -> Self {
        Self { props }
    }
    fn props(&self) -> &AdaptiveNavigationProps {
        &self.props
    }
    fn set_props(&mut self, props: AdaptiveNavigationProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("navigation", [])
    }
    fn update(&mut self, event: Event) {
        if let Event::Click { target } = event {
            if let Some(index) = (0..self.props.destinations.len())
                .find(|index| target == NodeId::from_key(&format!("destination-{index}")))
            {
                self.props.selected.set(index);
            }
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let selected = context.select(&self.props.selected, |selected| *selected);
        let size = context.container_classes("navigation");
        let style = match size.map(|size| size.width) {
            Some(rustnative_core::SizeClass::Compact) => NavigationStyle::BottomBar,
            Some(rustnative_core::SizeClass::Medium) | None => NavigationStyle::Rail,
            Some(rustnative_core::SizeClass::Expanded) => NavigationStyle::Sidebar,
        };
        let destinations = self.props.destinations.iter().enumerate().map(|(index, name)| {
            let text = match style {
                NavigationStyle::Sidebar => name.clone(),
                _ => name.chars().take(3).collect(),
            };
            Node::button(format!("destination-{index}"), text).with_accessibility(
                AccessibilityInfo::new(AccessibilityRole::Tab)
                    .name(name.clone())
                    .selected(index == selected)
                    .focusable(true),
            )
        });
        let bar = match style {
            NavigationStyle::BottomBar => {
                Node::row("destinations", destinations.collect::<Vec<_>>())
            }
            _ => Node::column("destinations", destinations.collect::<Vec<_>>()),
        }
        .with_accessibility(AccessibilityInfo::new(AccessibilityRole::TabList));
        let layout = LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fill);
        let page = Node::column_with_layout(
            "content",
            self.props.content.clone(),
            layout,
            rustnative_core::ColumnStyle::new(),
        );
        match style {
            NavigationStyle::BottomBar => Node::column_with_layout(
                "navigation",
                [page, bar],
                layout,
                rustnative_core::ColumnStyle::new(),
            ),
            _ => Node::row_with_layout(
                "navigation",
                [bar, page],
                layout,
                rustnative_core::RowStyle::new(),
            ),
        }
    }
}
