//! The component library on the headless backend (`PLAN.md` Milestone 48):
//! both syntaxes, bindings, commands, keyboard behaviour, accessibility, and
//! charts.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::cell::RefCell;
use std::time::Duration;

use rustnative_components::behaviour::TreeItem;
use rustnative_components::{
    ActionButton, ActionButtonProps, ButtonVariant, Chart, ChartKind, ChartProps, CommandPalette,
    CommandPaletteProps, DataTable, DataTableProps, Dialog, DialogProps, IDIOMS, Idioms, ListView,
    ListViewProps, RadioGroup, RadioGroupProps, Section, SectionLayout, SectionedView,
    SectionedViewProps, Series, TextField, Toast, ToastProps, TreeView, TreeViewProps,
    sorted_order, with_roles,
};
use rustnative_core::{
    AccessibilityRole, Command, CommandId, Component, ComponentContext, Event, KeyCode,
    KeyModifiers, Node, NodeId, Services, Size, Store, Window, rsx,
};
use rustnative_headless::{HeadlessApp, Query};

const SAVE: CommandId = CommandId::new("gallery.save");
const OK: CommandId = CommandId::new("gallery.ok");
const CANCEL: CommandId = CommandId::new("gallery.cancel");

thread_local! {
    static INVOKED: RefCell<Vec<CommandId>> = const { RefCell::new(Vec::new()) };
    static STORES: RefCell<Option<Stores>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct Stores {
    name: Store<String>,
    size: Store<usize>,
    fruit: Store<Option<usize>>,
    sort: Store<Option<(usize, bool)>>,
    toast: Store<Option<String>>,
    palette: Store<String>,
}

fn stores() -> Stores {
    STORES.with(|slot| slot.borrow().clone().unwrap())
}

struct Gallery {
    stores: Stores,
}

impl Component for Gallery {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { stores: stores() }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("gallery", [])
    }
    fn update(&mut self, event: Event) {
        if let Event::Command { id, .. } = event {
            INVOKED.with(|invoked| invoked.borrow_mut().push(id));
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        for (id, name) in [(SAVE, "Save"), (OK, "OK"), (CANCEL, "Cancel")] {
            context.command(Command::new(id, name));
        }
        let s = self.stores.clone();
        let name = s.name.clone();
        // The markup spelling, composing through the context.
        let field = rsx! { in context, <TextField key="name" label="Name" value={name} /> };
        let save = context.child_with_props::<ActionButton, _>(
            "save",
            ActionButtonProps {
                text: "Save".into(),
                variant: ButtonVariant::Primary,
                command: Some(SAVE),
            },
            ActionButton::new,
        );
        let sizes = context.child_with_props::<RadioGroup, _>(
            "sizes",
            RadioGroupProps {
                label: "Size".into(),
                options: vec!["Small".into(), "Large".into()],
                selected: s.size,
            },
            RadioGroup::new,
        );
        let list = context.child_with_props::<ListView, _>(
            "fruit",
            ListViewProps {
                items: vec!["Apple".into(), "Banana".into(), "Cherry".into()],
                sections: vec![(0, "A–B".into()), (2, "C".into())],
                selected: s.fruit,
            },
            ListView::new,
        );
        let table = context.child_with_props::<DataTable, _>(
            "table",
            DataTableProps {
                columns: vec!["Name".into(), "Age".into()],
                rows: vec![
                    vec!["Ada".into(), "36".into()],
                    vec!["Grace".into(), "85".into()],
                    vec!["Alan".into(), "41".into()],
                ],
                sort: s.sort,
            },
            DataTable::new,
        );
        let tree = context.child_with_props::<TreeView, _>(
            "tree",
            TreeViewProps {
                items: vec![
                    TreeItem { id: "src".into(), parent: None, label: "src".into() },
                    TreeItem {
                        id: "main".into(),
                        parent: Some("src".into()),
                        label: "main.rs".into(),
                    },
                ],
            },
            TreeView::new,
        );
        let dialog = context.child_with_props::<Dialog, _>(
            "dialog",
            DialogProps {
                title: "Delete file?".into(),
                message: "This cannot be undone.".into(),
                confirm: (OK, "Delete".into()),
                cancel: (CANCEL, "Cancel".into()),
                destructive: true,
            },
            Dialog::new,
        );
        let toast = context.child_with_props::<Toast, _>(
            "toast",
            ToastProps { message: s.toast, duration: Duration::from_secs(4) },
            Toast::new,
        );
        let palette = context.child_with_props::<CommandPalette, _>(
            "palette",
            CommandPaletteProps {
                commands: vec![(SAVE, "Save document".into()), (OK, "Open settings".into())],
                query: s.palette,
            },
            CommandPalette::new,
        );
        let chart = context.child_with_props::<Chart, _>(
            "chart",
            ChartProps {
                kind: ChartKind::Bar,
                title: "Revenue".into(),
                categories: vec!["Jan".into(), "Feb".into(), "Mar".into()],
                series: vec![
                    Series::new("2025", [10.0, 20.0, 42.0]),
                    Series::new("2026", [12.0, 25.0, 40.0]),
                ],
                width: 300,
                height: 200,
            },
            Chart::new,
        );
        Node::column(
            "gallery",
            [field, save, sizes, list, table, tree, dialog, toast, palette, chart],
        )
    }
}

fn launch() -> HeadlessApp {
    STORES.with(|slot| {
        *slot.borrow_mut() = Some(Stores {
            name: Store::new("name", "Ada".to_owned()),
            size: Store::new("size", 0),
            fruit: Store::new("fruit", None),
            sort: Store::new("sort", None),
            toast: Store::new("toast", None),
            palette: Store::new("palette", String::new()),
        });
    });
    INVOKED.with(|invoked| invoked.borrow_mut().clear());
    HeadlessApp::launch_with(
        Window::new("Gallery", Size::new(900, 2400)),
        Services::default(),
        with_roles(rustnative_core::Theme::default()),
        || Gallery::new(()),
    )
}

fn text(app: &HeadlessApp, key: &str) -> String {
    app.find(&Query::key(key)).map(|node| node.text.clone().unwrap_or_default()).unwrap_or_default()
}

#[test]
fn a_bound_field_writes_its_store_and_follows_it() {
    let mut app = launch();
    assert_eq!(text(&app, "input"), "Ada");
    app.set_text(&Query::key("input"), "Grace").unwrap();
    assert_eq!(stores().name.get(), "Grace");
    stores().name.set("Alan".into());
    app.settle();
    assert_eq!(text(&app, "input"), "Alan");
}

#[test]
fn buttons_invoke_commands_the_parent_declared() {
    let mut app = launch();
    app.click(&Query::key("button")).unwrap();
    assert_eq!(INVOKED.with(|invoked| invoked.borrow().clone()), vec![SAVE]);
}

#[test]
fn a_radio_group_list_table_and_tree_behave() {
    let mut app = launch();
    app.toggle(&Query::key("option-1")).unwrap();
    assert_eq!(stores().size.get(), 1);

    app.click(&Query::key("item-2")).unwrap();
    assert_eq!(stores().fruit.get(), Some(2));
    assert!(app.find(&Query::key("section-1")).is_ok(), "section headers are rows");

    app.click(&Query::key("header-1")).unwrap();
    assert_eq!(stores().sort.get(), Some((1, true)));
    app.click(&Query::key("header-1")).unwrap();
    assert_eq!(stores().sort.get(), Some((1, false)));
    assert_eq!(text(&app, "header-1"), "Age ▼");

    assert!(app.find(&Query::key("node-main")).is_err(), "collapsed");
    app.click(&Query::key("node-src")).unwrap();
    assert!(app.find(&Query::key("node-main")).is_ok(), "expanded");
}

#[test]
fn a_dialog_follows_the_hosts_button_order() {
    let app = launch();
    let confirm = app.find(&Query::key("confirm")).unwrap().window_rect.x;
    let cancel = app.find(&Query::key("cancel")).unwrap().window_rect.x;
    assert!(confirm < cancel, "Windows: the confirming button first");
    let dialog = app.find_all(&Query::role(AccessibilityRole::Dialog));
    assert!(dialog.iter().any(|node| node.name.as_deref() == Some("Delete file?")));
    // Another host's idioms, as a backend would provide them.
    assert!(!Idioms::macos().confirm_first);
    let _ = IDIOMS;
}

#[test]
fn a_toast_dismisses_itself() {
    let mut app = launch();
    stores().toast.set(Some("Saved".into()));
    app.settle();
    assert!(
        app.find(&Query::text("Saved")).is_ok(),
        "shown: {}",
        app.golden()
            .lines()
            .filter(|l| l.contains("toast") || l.contains("Saved"))
            .collect::<Vec<_>>()
            .join(
                "
"
            )
    );
    app.advance(Duration::from_secs(5));
    assert_eq!(stores().toast.get(), None);
    assert!(app.find(&Query::text("Saved")).is_err(), "gone");
}

#[test]
fn the_palette_filters_and_invokes() {
    let mut app = launch();
    app.set_text(&Query::key("query"), "open").unwrap();
    assert!(app.find(&Query::key("command-0")).is_err());
    app.click(&Query::key("command-1")).unwrap();
    assert_eq!(INVOKED.with(|invoked| invoked.borrow().clone()), vec![OK]);
}

#[test]
fn a_chart_is_a_table_of_its_points_and_reads_them_by_keyboard() {
    let mut app = launch();
    let plot = app.find(&Query::key("plot")).unwrap().clone();
    let info = &plot.accessibility;
    assert_eq!(info.role(), AccessibilityRole::Table);
    assert_eq!(info.elements().len(), 6, "every data point is a cell");
    assert!(
        info.description_hint().unwrap().starts_with("Bar chart of 2 series over 3 categories")
    );
    assert_eq!(info.elements()[2].info().name_hint(), Some("2025, Mar: 42"));

    let key =
        |key| Event::KeyDown { target: Some(plot.id), key, modifiers: KeyModifiers::default() };
    app.dispatch(key(KeyCode::ArrowRight));
    app.dispatch(key(KeyCode::ArrowDown));
    assert_eq!(text(&app, "point"), "2026, Feb: 25");
    let _ = NodeId::from_key("unused");
}

#[test]
fn table_sorting_is_a_view_over_the_rows() {
    let rows = vec![vec!["b".into(), "10".into()], vec!["a".into(), "9".into()]];
    assert_eq!(sorted_order(&rows, Some((1, true))), vec![1, 0], "numbers compare as numbers");
    assert_eq!(sorted_order(&rows, Some((0, true))), vec![1, 0]);
    assert_eq!(sorted_order(&rows, None), vec![0, 1]);
}

#[test]
fn sections_lay_out_as_list_grid_and_carousel() {
    let section = |title: &str, layout, count: usize| Section {
        title: title.into(),
        layout,
        items: (0..count).map(|index| format!("{title} {index}")).collect(),
    };
    let props = SectionedViewProps {
        sections: vec![
            section("Recent", SectionLayout::List, 2),
            section("Photos", SectionLayout::Grid(3), 5),
            section("Albums", SectionLayout::Carousel, 4),
        ],
    };
    let app = HeadlessApp::launch_with(
        Window::new("Sections", Size::new(600, 900)),
        Services::default(),
        with_roles(rustnative_core::Theme::default()),
        move || SectionedView::new(props.clone()),
    );
    let rect = |key: &str| app.find(&Query::key(key)).expect(key).window_rect;
    assert_eq!(text(&app, "s1-title"), "Photos");
    // A list stacks.
    assert!(rect("s0-1").y > rect("s0-0").y);
    // A grid of three fills a row, then wraps.
    assert_eq!(rect("s1-0").y, rect("s1-2").y);
    assert!(rect("s1-1").x > rect("s1-0").x);
    assert!(rect("s1-3").y > rect("s1-0").y);
    assert_eq!(rect("s1-3").x, rect("s1-0").x);
    // A carousel is one row.
    assert_eq!(rect("s2-0").y, rect("s2-3").y);
    assert!(rect("s2-3").x > rect("s2-0").x);
}
