//! Syntax equivalence (`PLAN.md` 2.9): the builder spelling, the `rsx!`
//! spelling, and the `.rsx` spelling of every case produce equal `Node`
//! values — and every node kind and modifier the grammar knows has a case.

use std::collections::BTreeSet;

use rustnative_conformance::{builder_cases, macro_cases, markup_file};
use rustnative_core::{Component, ComponentContext, ComponentTree, Event, Node};

#[test]
fn every_case_is_equal_in_all_three_spellings() {
    let builder = builder_cases::cases();
    let macro_ = macro_cases::cases();
    let file = markup_file::cases();
    let names =
        |cases: &[(&'static str, Node)]| cases.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    assert_eq!(names(&builder), names(&macro_), "the rsx! cases are the builder cases");
    assert_eq!(names(&builder), names(&file), "the .rsx cases are the builder cases");
    for (((name, built), (_, from_macro)), (_, from_file)) in builder.iter().zip(&macro_).zip(&file)
    {
        assert_eq!(built, from_macro, "`{name}`: builder and rsx! differ");
        assert_eq!(built, from_file, "`{name}`: builder and .rsx differ");
    }
}

#[test]
fn every_node_kind_and_modifier_has_a_case() {
    let covered: BTreeSet<&str> =
        builder_cases::cases().into_iter().map(|(name, _)| name).collect();
    for element in rustnative_markup_names() {
        assert!(
            covered.contains(element.as_str()),
            "no equivalence case for element or attribute `{element}`"
        );
    }
}

/// The element and modifier names the suite must cover, from the grammar's
/// own table — so a new element or modifier without a case fails here.
fn rustnative_markup_names() -> Vec<String> {
    let mut names = BTreeSet::new();
    let kinds = [
        ("Label", "label"),
        ("Button", "button"),
        ("TextInput", "text_input"),
        ("Canvas", "canvas"),
        ("Surface", "surface"),
        ("Foreign", "foreign"),
        ("TabBar", "tab_bar"),
        ("Checkbox", "checkbox"),
        ("Radio", "radio"),
        ("Toggle", "toggle"),
        ("Slider", "slider"),
        ("Progress", "progress"),
        ("Select", "select"),
        ("ListBox", "list_box"),
        ("DatePicker", "date_picker"),
        ("Spinner", "spinner"),
        ("Separator", "separator"),
        ("Link", "link"),
        ("MultilineText", "multiline_text"),
        ("Image", "image"),
        ("Column", "column"),
        ("Row", "row"),
        ("VirtualList", "virtual_list"),
        ("Grid", "grid"),
    ];
    let table = rustnative_markup::element_table();
    for spec in &table {
        let case = kinds.iter().find(|(element, _)| *element == spec.name).map(|(_, case)| *case);
        names.insert(case.unwrap_or(spec.name).to_owned());
        for attr in &spec.attrs {
            if attr.kind == rustnative_markup::AttrKind::Modifier {
                names.insert(attr.name.to_owned());
            }
        }
    }
    names.into_iter().collect()
}

/// Renders the component-element case through a real component tree, so
/// composition is compared, not just construction.
struct Host {
    spelling: u8,
}

impl Component for Host {
    type Props = u8;
    type Message = ();
    fn new(spelling: u8) -> Self {
        Self { spelling }
    }
    fn props(&self) -> &u8 {
        &self.spelling
    }
    fn set_props(&mut self, spelling: u8) {
        self.spelling = spelling;
    }
    fn view(&self) -> Node {
        Node::column("empty", [])
    }
    fn update(&mut self, _event: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        match self.spelling {
            0 => builder_cases::components(context),
            1 => macro_cases::components(context),
            _ => markup_file::components(context),
        }
    }
}

#[test]
fn a_component_element_composes_like_the_builder() {
    let views: Vec<Node> =
        (0..3).map(|spelling| ComponentTree::new(Host::new(spelling)).view()).collect();
    assert_eq!(views[0], views[1], "builder and rsx! compose the same tree");
    assert_eq!(views[0], views[2], "builder and .rsx compose the same tree");
}
