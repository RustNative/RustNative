//! The invalidation contract (`docs/invalidation.md`), asserted: for each
//! kind of change, exactly the components the contract names render, and
//! every other component's output is reused.

use std::collections::BTreeSet;

use rustnative_core::{
    ColorScheme, Component, ComponentContext, ComponentTree, Event, Locale, Node, NodeId,
    Preference, PreferenceKey, RenderCause, keys,
};

/// A leaf that shows its props and, optionally, the locale.
struct Leaf {
    props: LeafProps,
    clicks: u32,
}

#[derive(Clone, PartialEq, Default)]
struct LeafProps {
    label: String,
    reads_locale: bool,
    title: Option<String>,
}

impl Component for Leaf {
    type Props = LeafProps;
    type Message = ();
    fn new(props: LeafProps) -> Self {
        Self { props, clicks: 0 }
    }
    fn props(&self) -> &LeafProps {
        &self.props
    }
    fn set_props(&mut self, props: LeafProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::button("leaf", format!("{} {}", self.props.label, self.clicks))
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { .. }) {
            self.clicks += 1;
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        if let Some(title) = &self.props.title {
            context.prefer(&TITLE, Title(title.clone()));
        }
        let suffix = if self.props.reads_locale {
            format!(" [{}]", context.env(&keys::LOCALE).tag())
        } else {
            String::new()
        };
        Node::button("leaf", format!("{} {}{suffix}", self.props.label, self.clicks))
    }
}

/// The window title a screen wants: the last publisher in tree order wins.
#[derive(Debug, Clone, PartialEq)]
struct Title(String);

impl Preference for Title {
    fn reduce(self, next: Self) -> Self {
        next
    }
}

const TITLE: PreferenceKey<Title> = PreferenceKey::new("test.title");

/// Root → two leaves, one of which reads the locale; a third reads the
/// locale inside a subtree that overrides it.
struct Root {
    left_label: String,
    dark_subtree: bool,
    shown_title: Option<String>,
}

impl Component for Root {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { left_label: "left".into(), dark_subtree: false, shown_title: None }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("root", [])
    }
    fn update(&mut self, event: Event) {
        if let Event::Click { target } = event {
            if target == NodeId::from_key("rename") {
                self.left_label = "renamed".into();
            } else if target == NodeId::from_key("override") {
                self.dark_subtree = true;
            }
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        self.shown_title = context.preference(&TITLE).map(|title| title.0);
        let left = context.child_with_props::<Leaf, _>(
            "left",
            LeafProps { label: self.left_label.clone(), ..LeafProps::default() },
            Leaf::new,
        );
        let right = context.child_with_props::<Leaf, _>(
            "right",
            LeafProps { label: "right".into(), reads_locale: true, title: Some("Right".into()) },
            Leaf::new,
        );
        let nested = context.child_with_props::<Scope, _>("scope", self.dark_subtree, Scope::new);
        Node::column(
            "root",
            [
                Node::button("rename", "Rename"),
                Node::button("override", "Override"),
                Node::label("title", self.shown_title.clone().unwrap_or_default()),
                left,
                right,
                nested,
            ],
        )
    }
}

/// Provides a locale override to its child when its props say so.
struct Scope {
    override_locale: bool,
}

impl Component for Scope {
    type Props = bool;
    type Message = ();
    fn new(override_locale: bool) -> Self {
        Self { override_locale }
    }
    fn props(&self) -> &bool {
        &self.override_locale
    }
    fn set_props(&mut self, value: bool) {
        self.override_locale = value;
    }
    fn view(&self) -> Node {
        Node::column("scope", [])
    }
    fn update(&mut self, _event: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        if self.override_locale {
            context.provide_env(&keys::LOCALE, Locale::new("ar-EG"));
        }
        let inner = context.child_with_props::<Leaf, _>(
            "inner",
            LeafProps { label: "inner".into(), reads_locale: true, title: None },
            Leaf::new,
        );
        Node::column("scope", [inner])
    }
}

fn rendered(tree: &ComponentTree) -> BTreeSet<String> {
    tree.last_render_log()
        .iter()
        .map(|record| record.path.rsplit('/').next().unwrap_or("").to_owned())
        .collect()
}

fn texts(tree: &ComponentTree) -> Vec<String> {
    let mut out = Vec::new();
    tree.view().visit(&mut |node, _, _| match node {
        Node::Button(button) => out.push(button.text().to_owned()),
        Node::Label(label) => out.push(label.text().to_owned()),
        _ => {}
    });
    out
}

fn leaf_id(tree: &ComponentTree, label_prefix: &str) -> NodeId {
    let mut found = None;
    tree.view().visit(&mut |node, _, _| {
        if let Node::Button(button) = node {
            if button.text().starts_with(label_prefix) {
                found = Some(node.id());
            }
        }
    });
    assert!(found.is_some(), "the leaf is rendered");
    found.unwrap_or_else(|| NodeId::from_key("missing"))
}

#[test]
fn an_event_renders_only_the_component_that_handled_it() {
    let mut tree = ComponentTree::new(Root::new(()));
    let right = leaf_id(&tree, "right");
    tree.dispatch(Event::Click { target: right });
    assert_eq!(rendered(&tree), BTreeSet::from(["right".to_owned()]));
    assert!(texts(&tree).contains(&"right 1 [en-US]".to_owned()), "{:?}", texts(&tree));
    assert!(
        tree.last_render_log().iter().all(|record| record.cause == RenderCause::Event),
        "{:?}",
        tree.last_render_log()
    );
}

#[test]
fn new_props_render_the_parent_and_the_child_whose_props_changed() {
    let mut tree = ComponentTree::new(Root::new(()));
    tree.dispatch(Event::Click { target: NodeId::from_key("rename") });
    let log = tree.last_render_log();
    let paths = rendered(&tree);
    assert!(paths.contains("left"), "{log:?}");
    assert!(!paths.contains("right"), "unchanged props are skipped: {log:?}");
    assert!(!paths.contains("inner"), "{log:?}");
    assert!(texts(&tree).contains(&"renamed 0".to_owned()));
}

#[test]
fn an_environment_change_renders_only_its_readers() {
    let mut tree = ComponentTree::new(Root::new(()));
    tree.set_environment(&keys::LOCALE, Locale::new("pl-PL"));
    assert_eq!(rendered(&tree), BTreeSet::from(["inner".to_owned(), "right".to_owned()]));
    assert!(texts(&tree).contains(&"right 0 [pl-PL]".to_owned()));
    assert!(texts(&tree).contains(&"inner 0 [pl-PL]".to_owned()));

    // A value nobody reads invalidates nothing.
    tree.set_environment(&keys::COLOR_SCHEME, ColorScheme::Dark);
    assert!(rendered(&tree).is_empty(), "{:?}", tree.last_render_log());
}

#[test]
fn a_provided_value_overrides_the_window_for_one_subtree() {
    let mut tree = ComponentTree::new(Root::new(()));
    tree.dispatch(Event::Click { target: NodeId::from_key("override") });
    let texts = texts(&tree);
    assert!(texts.contains(&"inner 0 [ar-EG]".to_owned()), "{texts:?}");
    assert!(texts.contains(&"right 0 [en-US]".to_owned()), "outside the subtree: {texts:?}");
}

#[test]
fn a_descendants_preference_reaches_its_ancestor() {
    let tree = ComponentTree::new(Root::new(()));
    // Published during the first render; read by the root on the pass the
    // change triggers.
    assert!(texts(&tree).contains(&"Right".to_owned()), "{:?}", texts(&tree));
}

#[test]
fn a_forced_render_renders_everything() {
    let mut tree = ComponentTree::new(Root::new(()));
    let _ = tree.render();
    assert_eq!(rendered(&tree).len(), 5, "{:?}", tree.last_render_log());
}

#[test]
fn the_batching_guarantee_holds_across_a_message_cascade() {
    // One event whose handling changes state in the handler: one pass, one
    // render per affected component, never a partial set (C09).
    let mut tree = ComponentTree::new(Root::new(()));
    tree.dispatch(Event::Click { target: NodeId::from_key("rename") });
    let log = tree.last_render_log();
    let mut seen = BTreeSet::new();
    for record in log {
        assert!(seen.insert(record.component), "rendered twice in one pass: {log:?}");
    }
}
