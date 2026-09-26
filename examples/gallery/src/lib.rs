//! The component gallery (`PLAN.md` Milestone 48's "done when"): an
//! application built entirely from `rustnative-components` and one token
//! set. Nothing here draws or styles a primitive itself; every color comes
//! from `tokens.json` through `gallery.css`, and the roles the token set
//! marks follow the host's accent and surface.

use std::time::Duration;

use rustnative_components::behaviour::TreeItem;
use rustnative_components::{
    ActionButton, ActionButtonProps, AdaptiveNavigation, AdaptiveNavigationProps, Badge,
    BadgeProps, ButtonVariant, Card, CardProps, Chart, ChartKind, ChartProps, DataTable,
    DataTableProps, RadioGroup, RadioGroupProps, Section, SectionLayout, SectionedView,
    SectionedViewProps, Series, TextField, Toast, ToastProps, Tone, TreeView, TreeViewProps,
    with_roles,
};
use rustnative_core::{Command, CommandId, Component, ComponentContext, Event, Node, Store, rsx};

/// The gallery's save command.
pub const SAVE: CommandId = CommandId::new("gallery.save");

/// The gallery's theme: its token set, with the library's defaults for any
/// role the set leaves out.
#[must_use]
pub fn theme() -> rustnative_core::Theme {
    with_roles(include!(concat!(env!("OUT_DIR"), "/app_theme.rs")))
}

/// The gallery.
#[derive(Debug)]
pub struct Gallery {
    page: Store<usize>,
    name: Store<String>,
    size: Store<usize>,
    sort: Store<Option<(usize, bool)>>,
    toast: Store<Option<String>>,
}

impl Gallery {
    fn overview(context: &mut ComponentContext<'_, ()>) -> Node {
        let badges = [
            ("new", "New", Tone::Accent),
            ("beta", "Beta", Tone::Neutral),
            ("late", "Overdue", Tone::Danger),
        ]
        .map(|(key, text, tone)| {
            context.child_with_props::<Badge, _>(
                key,
                BadgeProps { text: text.into(), tone },
                Badge::new,
            )
        });
        let card = context.child_with_props::<Card, _>(
            "status",
            CardProps { title: "Status".into(), body: badges.to_vec() },
            Card::new,
        );
        let chart = context.child_with_props::<Chart, _>(
            "revenue",
            ChartProps {
                kind: ChartKind::Bar,
                title: "Revenue".into(),
                categories: vec!["Jan".into(), "Feb".into(), "Mar".into()],
                series: vec![
                    Series::new("2025", [10.0, 20.0, 42.0]),
                    Series::new("2026", [12.0, 25.0, 40.0]),
                ],
                width: 320,
                height: 180,
            },
            Chart::new,
        );
        Node::column("overview", [card, chart])
    }

    fn data(&self, context: &mut ComponentContext<'_, ()>) -> Node {
        let table = context.child_with_props::<DataTable, _>(
            "people",
            DataTableProps {
                columns: vec!["Name".into(), "Born".into()],
                rows: vec![
                    vec!["Ada Lovelace".into(), "1815".into()],
                    vec!["Grace Hopper".into(), "1906".into()],
                    vec!["Alan Turing".into(), "1912".into()],
                ],
                sort: self.sort.clone(),
            },
            DataTable::new,
        );
        let tree = context.child_with_props::<TreeView, _>(
            "files",
            TreeViewProps {
                items: vec![
                    TreeItem { id: "src".into(), parent: None, label: "src".into() },
                    TreeItem {
                        id: "lib".into(),
                        parent: Some("src".into()),
                        label: "lib.rs".into(),
                    },
                ],
            },
            TreeView::new,
        );
        let sections = context.child_with_props::<SectionedView, _>(
            "library",
            SectionedViewProps {
                sections: vec![
                    Section {
                        title: "Recent".into(),
                        layout: SectionLayout::List,
                        items: vec!["Notes".into(), "Plan".into()],
                    },
                    Section {
                        title: "Photos".into(),
                        layout: SectionLayout::Grid(3),
                        items: (1..=6).map(|index| format!("Photo {index}")).collect(),
                    },
                ],
            },
            SectionedView::new,
        );
        Node::column("data", [table, tree, sections])
    }

    fn settings(&self, context: &mut ComponentContext<'_, ()>) -> Node {
        let name = self.name.clone();
        // A component element in markup, beside builder calls: the library
        // is at home in both syntaxes.
        let field = rsx! { in context, <TextField key="name" label="Display name" value={name} /> };
        let size = context.child_with_props::<RadioGroup, _>(
            "size",
            RadioGroupProps {
                label: "Text size".into(),
                options: vec!["Small".into(), "Large".into()],
                selected: self.size.clone(),
            },
            RadioGroup::new,
        );
        let save = context.child_with_props::<ActionButton, _>(
            "save",
            ActionButtonProps {
                text: "Save".into(),
                variant: ButtonVariant::Primary,
                command: Some(SAVE),
            },
            ActionButton::new,
        );
        Node::column("settings", [field, size, save])
    }
}

impl Component for Gallery {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self {
            page: Store::new("gallery.page", 0),
            name: Store::new("gallery.name", "Ada".to_owned()),
            size: Store::new("gallery.size", 0),
            sort: Store::new("gallery.sort", None),
            toast: Store::new("gallery.toast", None),
        }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("gallery", [])
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Command { id, .. } if id == SAVE) {
            self.toast.set(Some(format!("Saved {}", self.name.get())));
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        context.command(Command::new(SAVE, "Save"));
        let page = context.select(&self.page, |page| *page);
        let page_node = match page {
            0 => Self::overview(context),
            1 => self.data(context),
            _ => self.settings(context),
        };
        let navigation = context.child_with_props::<AdaptiveNavigation, _>(
            "navigation",
            AdaptiveNavigationProps {
                destinations: vec!["Overview".into(), "Data".into(), "Settings".into()],
                selected: self.page.clone(),
                content: vec![page_node],
            },
            AdaptiveNavigation::new,
        );
        let toast = context.child_with_props::<Toast, _>(
            "toast",
            ToastProps { message: self.toast.clone(), duration: Duration::from_secs(3) },
            Toast::new,
        );
        Node::column("gallery", [navigation, toast])
    }
}
