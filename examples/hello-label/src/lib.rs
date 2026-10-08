//! The Rust Native framework's example application: the desktop binary
//! (`main.rs`) and the Android library both run [`run`].

use std::sync::Arc;
use std::time::Duration;

#[cfg(target_os = "android")]
use rustnative_android::{AndroidPlatform as HostPlatform, FileStateStore};
use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, AccessibleAction, AccessibleActionKind, Alignment,
    AnimatedProperty, AnimatedValue, Animation, AnimationRequests, Application, Callback,
    CheckedState, Color, ColumnStyle, Component, ComponentContext, DrawList, DropEffect,
    EdgeInsets, Event, InputInterest, InputRequests, ItemExtent, LayoutStyle, LiveRegion, MenuBar,
    MenuItem, NavigationCommand, NavigationStack, Navigator, Node, NodeId, Overflow, Paint,
    PanicPolicy, Persisted, Platform, Point, RectF, RowStyle, Services, Size, SizeMode, TaskHandle,
    Transition, Vec2, VirtualListStyle, VirtualRange, Window, classes,
};
use rustnative_core::{ColorScheme, environment::keys};
#[cfg(target_os = "linux")]
use rustnative_linux::{FileStateStore, LinuxPlatform as HostPlatform};
#[cfg(not(any(target_os = "linux", target_os = "android")))]
use rustnative_windows::{FileStateStore, WindowsPlatform as HostPlatform};

#[derive(Debug, Clone, PartialEq, Eq)]
enum AppMessage {
    ChildCountChanged(u32),
    ChildNameChanged(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CounterPanelProps {
    title: String,
    on_message: Callback<AppMessage>,
}

enum CounterMessage {
    AsyncCompleted,
    /// Set by the inspector, or by `rustnative dev` restoring state after a
    /// restart (`Component::edit`).
    Restore {
        count: u32,
        name: String,
    },
}

struct CounterPanel {
    props: CounterPanelProps,
    count: u32,
    name: String,
    mounted_count: u32,
    unmounted_count: u32,
    props_update_count: u32,
    async_task: Option<TaskHandle>,
    async_requested: bool,
    async_status: String,
}

impl Component for CounterPanel {
    type Props = CounterPanelProps;
    type Message = CounterMessage;

    fn new(props: Self::Props) -> Self {
        Self {
            props,
            count: 0,
            name: String::new(),
            mounted_count: 0,
            unmounted_count: 0,
            props_update_count: 0,
            async_task: None,
            async_requested: false,
            async_status: "Idle".to_owned(),
        }
    }

    fn props(&self) -> &Self::Props {
        &self.props
    }

    fn set_props(&mut self, props: Self::Props) {
        self.props = props;
    }

    fn view(&self) -> Node {
        Node::column_with_layout(
            "counter-panel",
            [
                // Styled with utility classes: `dark:` follows the host's
                // colour scheme (a live `WM_SETTINGCHANGE`), or the shell's
                // override below — re-resolved on these same controls.
                Node::label_with_layout(
                    "title",
                    self.props.title.clone(),
                    LayoutStyle::new().height(SizeMode::Fixed(32)),
                )
                .with_class(classes!("panel-text font-semibold dark:text-sky-300")),
                Node::label_with_layout(
                    "counter",
                    format!("Count: {}", self.count),
                    LayoutStyle::new().height(SizeMode::Fixed(32)),
                )
                .with_class(classes!("panel-text")),
                Node::text_input_with_layout(
                    "name",
                    self.name.clone(),
                    LayoutStyle::new()
                        .width(SizeMode::Fixed(480))
                        .height(SizeMode::Fixed(32))
                        .align_self(Alignment::Center),
                ),
                Node::row_with_layout(
                    "controls",
                    [
                        Node::button_with_layout(
                            "increment",
                            "Increment",
                            LayoutStyle::new().width(SizeMode::Auto).height(SizeMode::Fixed(36)),
                        )
                        // Demonstrates `disabled`: past 5, the button is
                        // realized with `EnableWindow(hwnd, 0)` and the
                        // theme's `ControlState::Disabled` style.
                        .disabled(self.count >= 5),
                        Node::button_with_layout(
                            "async-task",
                            if self.async_task.is_some() { "Cancel Async" } else { "Run Async" },
                            LayoutStyle::new().width(SizeMode::Auto).height(SizeMode::Fixed(36)),
                        )
                        // Demonstrates the accessibility bridge. This
                        // button's visible label changes as the task runs,
                        // so its *announced* name is pinned to something
                        // stable and unambiguous instead — realized on
                        // Windows through `IAccPropServices`, which
                        // overrides the accessible name of the standard
                        // control without replacing the control.
                        //
                        // Every node already carries a sensible default
                        // (a button is `Role::Button` and focusable); this
                        // is the case where the default is not enough.
                        .with_accessibility(
                            AccessibilityInfo::new(AccessibilityRole::Button)
                                .name("Run or cancel the background task")
                                .description(
                                    "Starts a two-second task, or cancels it if one is running",
                                )
                                .focusable(true),
                        ),
                        Node::label_with_layout(
                            "async-status",
                            format!("Async: {}", self.async_status),
                            LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Auto),
                        ),
                        Node::label_with_layout(
                            "lifecycle",
                            format!(
                                "Mounted: {} | Unmounted: {} | Props updated: {}",
                                self.mounted_count, self.unmounted_count, self.props_update_count
                            ),
                            LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Auto),
                        ),
                    ],
                    LayoutStyle::new().width(SizeMode::Fixed(520)).align_self(Alignment::Center),
                    RowStyle::new()
                        .padding(EdgeInsets::symmetric(12, 20))
                        .gap(12)
                        .align_items(Alignment::Center),
                ),
                Node::column_with_layout(
                    "items",
                    (0..24).map(|index| {
                        Node::label_with_layout(
                            format!("item-{index}"),
                            format!("Scrollable item {index}"),
                            LayoutStyle::new().height(SizeMode::Fixed(28)),
                        )
                    }),
                    LayoutStyle::new()
                        .width(SizeMode::Fixed(480))
                        .height(SizeMode::Fixed(180))
                        .align_self(Alignment::Center),
                    ColumnStyle::new()
                        .padding(EdgeInsets::all(12))
                        .gap(6)
                        .overflow(Overflow::Scroll),
                ),
            ],
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::all(24)).gap(16).align_items(Alignment::Center),
        )
        .with_class(classes!("bg-panel dark:bg-neutral-900 border-neutral-300 rounded-lg"))
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("increment") => {
                self.count += 1;
                self.props.on_message.send(AppMessage::ChildCountChanged(self.count));
            }
            Event::Click { target } if target == NodeId::from_key("async-task") => {
                if let Some(task) = &self.async_task {
                    task.cancel();
                    self.async_task = None;
                    "Cancelled".clone_into(&mut self.async_status);
                } else {
                    self.async_requested = true;
                    "Running".clone_into(&mut self.async_status);
                }
            }
            Event::TextChanged { target, value } if target == NodeId::from_key("name") => {
                self.name.clone_from(&value);
                self.props.on_message.send(AppMessage::ChildNameChanged(value));
            }
            // The explicit list below is deliberate documentation, not dead
            // code: it names every event kind this component chooses not to
            // react to, so a reader can see at a glance what was considered
            // rather than just assuming an oversight. `#[allow]`d rather
            // than collapsed into the wildcard alone, since collapsing it
            // would lose exactly that value for a lint whose concern
            // (identical arm bodies) doesn't apply to documentation intent.
            #[allow(clippy::match_same_arms)]
            Event::KeyDown { .. }
            | Event::TextInput { .. }
            | Event::TextChanged { .. }
            | Event::Click { .. }
            | Event::FocusGained { .. }
            | Event::FocusLost { .. }
            | Event::WindowResized { .. }
            | Event::WindowMoved { .. }
            | Event::WindowCloseRequested { .. }
            | Event::WindowStateChanged { .. }
            | Event::MenuAction { .. } => {}
            // `Event` is `#[non_exhaustive]` (rustnative-core standards audit
            // P2.4): a future new variant lands here by default rather than
            // failing to build every downstream crate.
            _ => {}
        }
    }

    fn message(&mut self, message: Self::Message) {
        match message {
            CounterMessage::AsyncCompleted => {
                self.async_task = None;
                self.async_requested = false;
                "Completed".clone_into(&mut self.async_status);
            }
            CounterMessage::Restore { count, name } => {
                self.count = count;
                self.name = name;
            }
        }
    }

    fn inspect(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({ "count": self.count, "name": self.name }))
    }

    fn edit(&self, field: &str, value: &serde_json::Value) -> Option<Self::Message> {
        let (mut count, mut name) = (self.count, self.name.clone());
        match field {
            "count" => count = u32::try_from(value.as_u64()?).ok()?,
            "name" => value.as_str()?.clone_into(&mut name),
            _ => return None,
        }
        Some(CounterMessage::Restore { count, name })
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        if self.async_requested && self.async_task.is_none() {
            self.async_requested = false;
            let delay = context.sleep(std::time::Duration::from_secs(2));
            let task = context.spawn(async move {
                delay.await;
                CounterMessage::AsyncCompleted
            });
            self.async_task = Some(task);
        }
        self.view()
    }

    fn props_changed(&mut self) {
        self.props_update_count += 1;
    }

    fn mounted(&mut self) {
        self.mounted_count += 1;
    }

    fn unmounted(&mut self) {
        // Async tasks are owned by the framework-managed component scope.
        // The scope is cancelled automatically when this component unmounts.
        self.async_task = None;
        self.unmounted_count += 1;
    }
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "independent one-shot menu requests, each consumed by the next render"
)]
struct AppShell {
    panel_visible: bool,
    title_index: usize,
    child_count: u32,
    child_name: String,
    settings_requested: bool,
    settings_opened_count: u32,
    input_lab_requested: bool,
    long_list_requested: bool,
    preferences_requested: bool,
    /// The colour scheme forced on the counter panel, or `None` to follow
    /// the host's.
    scheme: Option<ColorScheme>,
}

/// A separate root used to exercise the native multi-window host. It has no
/// shared component state with `AppShell`; the framework owns its lifetime and
/// scheduler independently.
struct AuxiliaryWindow;

impl Component for AuxiliaryWindow {
    type Props = ();
    type Message = ();

    fn new((): Self::Props) -> Self {
        Self
    }

    fn props(&self) -> &Self::Props {
        static PROPS: () = ();
        &PROPS
    }

    fn set_props(&mut self, (): Self::Props) {}

    fn view(&self) -> Node {
        Node::column_with_layout(
            "auxiliary-root",
            [Node::label_with_layout(
                "auxiliary-label",
                "Independent native window",
                LayoutStyle::new().height(SizeMode::Fixed(36)),
            )],
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::all(24)),
        )
    }

    fn update(&mut self, _: Event) {}
}

/// Opened dynamically at runtime from the "File \u{2192} New Settings Window"
/// menu action (see `AppShell::render`) rather than up front in `main`, to
/// exercise `ComponentContext::windows()`.
struct SettingsWindow {
    opened_at_click: u32,
}

impl Component for SettingsWindow {
    type Props = u32;
    type Message = ();

    fn new(props: Self::Props) -> Self {
        Self { opened_at_click: props }
    }

    fn props(&self) -> &Self::Props {
        &self.opened_at_click
    }

    fn set_props(&mut self, props: Self::Props) {
        self.opened_at_click = props;
    }

    fn view(&self) -> Node {
        Node::column_with_layout(
            "settings-root",
            [Node::label_with_layout(
                "settings-label",
                format!("Opened from the menu (request #{})", self.opened_at_click),
                LayoutStyle::new().height(SizeMode::Fixed(36)),
            )],
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::all(24)),
        )
    }

    fn update(&mut self, _: Event) {}
}

/// Milestone 25's advanced input, live: opened from "File \u{2192} Input
/// Lab". Every stream it shows is opt-in on a node (`with_input`), so the
/// rest of the example never receives pointer or controller traffic.
struct InputLab {
    input: Option<InputRequests>,
    animations: Option<AnimationRequests>,
    // Milestone 27: holding the pad down dims it, and letting go brings it
    // back. Both are animated by the backend, not by rerendering.
    pressed: bool,
    // Milestone 29: which bar of the canvas chart was clicked.
    selected_bar: Option<u32>,
    pointer: String,
    gesture: String,
    wheel: String,
    dropped: String,
    keyboard: String,
    gamepad: String,
    // Milestone 26: custom controls with no native equivalent, fully
    // described to assistive technology (see `view`).
    muted: bool,
    volume: f32,
}

/// Milestone 28: a hundred thousand rows, of which only the visible dozen
/// or so exist as native windows at any moment.
///
/// The component holds the *data* (here, computed on demand) and the range
/// the backend last asked for; it renders exactly that range. Scrolling
/// inside the range costs nothing; crossing out of it arrives as one
/// `Event::VisibleRangeChanged`, and the rows that scroll in are realized
/// on the windows of the rows that scrolled out. "Add 10 above" inserts
/// data at the top without moving what is on screen.
struct LongList {
    range: VirtualRange,
    /// Rows inserted at the top since the window opened; row keys name the
    /// datum, so a row keeps its identity (and its place on screen) when
    /// data arrives above it.
    prepended: usize,
}

impl LongList {
    const ROWS: usize = 100_000;

    /// The datum at item `index`: rows added above the original first row
    /// count down from -1.
    fn datum(&self, index: usize) -> String {
        match index.checked_sub(self.prepended) {
            Some(datum) => datum.to_string(),
            None => format!("-{}", self.prepended - index),
        }
    }
}

impl Component for LongList {
    type Props = ();
    type Message = ();

    fn new((): Self::Props) -> Self {
        Self { range: VirtualRange::EMPTY, prepended: 0 }
    }

    fn props(&self) -> &Self::Props {
        static PROPS: () = ();
        &PROPS
    }

    fn set_props(&mut self, (): Self::Props) {}

    fn view(&self) -> Node {
        let rows = self.range.indices().map(|index| {
            let datum = self.datum(index);
            Node::label_with_layout(
                format!("datum-{datum}"),
                format!("Row {datum}"),
                LayoutStyle::new().height(SizeMode::Fixed(24)),
            )
            .with_item_index(index)
        });
        Node::column(
            "long-root",
            [
                Node::button_with_layout(
                    "long-prepend",
                    "Add 10 above",
                    LayoutStyle::new().width(SizeMode::Fixed(140)).height(SizeMode::Fixed(30)),
                ),
                Node::label_with_layout(
                    "long-range",
                    format!(
                        "Realized rows {}..{} of {}",
                        self.range.first,
                        self.range.last_exclusive,
                        Self::ROWS + self.prepended
                    ),
                    LayoutStyle::new().height(SizeMode::Fixed(24)),
                ),
                Node::virtual_list(
                    "long-list",
                    VirtualListStyle::new(Self::ROWS + self.prepended, ItemExtent::Fixed(24)),
                    rows,
                ),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::VisibleRangeChanged { range, .. } => self.range = range,
            Event::Click { target } if target == NodeId::from_key("long-prepend") => {
                self.prepended += 10;
            }
            _ => {}
        }
    }
}

/// Milestone 30: tabs, a navigation stack, and state that outlives the
/// process.
///
/// Both tabs' pages stay mounted — the hidden one keeps its state — and
/// the "Guide" tab is a navigation stack whose screens are child
/// components, so going back finds the page as it was left. The press
/// count is persisted: it is still there the next time the example runs.
struct Preferences {
    tab: usize,
    guide: NavigationStack<String>,
    presses: Option<Persisted<u32>>,
}

impl Component for Preferences {
    type Props = ();
    type Message = NavigationCommand<String>;

    fn new((): Self::Props) -> Self {
        Self { tab: 0, guide: NavigationStack::new("Welcome".to_owned()), presses: None }
    }

    fn props(&self) -> &Self::Props {
        static PROPS: () = ();
        &PROPS
    }

    fn set_props(&mut self, (): Self::Props) {}

    fn view(&self) -> Node {
        Node::label("prefs-unused", "")
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        let presses = context.persisted("presses", 0u32);
        let count = presses.get();
        self.presses = Some(presses);
        let navigator = Navigator::new(context.callback());
        let guide = self.guide.view("prefs-guide", |entry| {
            context.child_with_props(
                entry.id().key(),
                GuidePageProps { title: entry.route().clone(), navigator: navigator.clone() },
                GuidePage::new,
            )
        });
        Node::column(
            "prefs-root",
            [
                Node::tab_bar(
                    "prefs-tabs",
                    ["General", "Guide"],
                    self.tab,
                    LayoutStyle::new().height(SizeMode::Fixed(28)),
                ),
                Node::column(
                    "prefs-general",
                    [
                        Node::label(
                            "prefs-count",
                            format!("Pressed {count} times (kept across runs)"),
                        ),
                        Node::button("prefs-press", "Press"),
                    ],
                )
                .hidden(self.tab != 0),
                guide.hidden(self.tab != 1),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::TabSelected { index, .. } => self.tab = index,
            Event::Click { target } if target == NodeId::from_key("prefs-press") => {
                if let Some(presses) = &self.presses {
                    presses.update(|count| *count += 1);
                }
            }
            _ => {}
        }
    }

    fn message(&mut self, command: NavigationCommand<String>) {
        self.guide.apply(command);
    }
}

#[derive(Debug, Clone, PartialEq)]
struct GuidePageProps {
    title: String,
    navigator: Navigator<String>,
}

/// One screen of the guide, with its own counter — which survives pushing
/// another screen on top and popping back to it.
struct GuidePage {
    props: GuidePageProps,
    clicks: u32,
}

impl Component for GuidePage {
    type Props = GuidePageProps;
    type Message = ();

    fn new(props: Self::Props) -> Self {
        Self { props, clicks: 0 }
    }

    fn props(&self) -> &Self::Props {
        &self.props
    }

    fn set_props(&mut self, props: Self::Props) {
        self.props = props;
    }

    fn view(&self) -> Node {
        Node::column(
            "guide-page",
            [
                Node::label(
                    "guide-title",
                    format!("{} (clicked {})", self.props.title, self.clicks),
                ),
                Node::button("guide-click", "Click"),
                Node::button("guide-next", "Next page"),
                Node::button("guide-back", "Back"),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        let Event::Click { target } = event else { return };
        if target == NodeId::from_key("guide-click") {
            self.clicks += 1;
        } else if target == NodeId::from_key("guide-next") {
            self.props.navigator.push(format!("{} \u{2192} next", self.props.title));
        } else if target == NodeId::from_key("guide-back") {
            self.props.navigator.pop();
        }
    }
}

/// How long the pad takes to dim and to come back.
const FADE: Duration = Duration::from_millis(120);

impl InputLab {
    /// A four-bar chart; the selected bar is drawn darker.
    fn chart(&self) -> DrawList {
        const VALUES: [f32; 4] = [0.4, 0.9, 0.6, 0.75];
        let mut chart = DrawList::new()
            .fill_rounded_rect(
                RectF::new(0.0, 0.0, 240.0, 90.0),
                6.0,
                Paint::color(Color::rgb(245, 246, 250)),
            )
            .text(Vec2::new(8.0, 4.0), "Click a bar", 12.0, Color::rgb(90, 90, 110));
        for ((index, value), x) in (0u32..).zip(VALUES).zip([20.0, 75.0, 130.0, 185.0]) {
            let height = 60.0 * value;
            let bar = RectF::new(x, 84.0 - height, 36.0, height);
            let color = if self.selected_bar == Some(index) {
                Color::rgb(30, 70, 160)
            } else {
                Color::rgb(90, 140, 230)
            };
            chart = chart.fill_rect(bar, Paint::color(color)).hit_region(index, bar);
        }
        chart
    }

    /// Springs the pad back from an offset to where it was laid out.
    ///
    /// A spring has no duration: it runs until it stops moving, and
    /// interrupting it mid-flight keeps the motion continuous instead of
    /// restarting. Nothing in the tree changes while it runs.
    fn bounce_pad(&self) {
        let Some(animations) = &self.animations else {
            return;
        };
        animations.animate(
            "lab-pad",
            Animation::new(
                AnimatedProperty::Translation,
                AnimatedValue::Offset(Point::new(0, 0)),
                Transition::spring(220.0, 14.0, 1.0),
            )
            .from(AnimatedValue::Offset(Point::new(24, 0))),
        );
    }
}

impl Component for InputLab {
    type Props = ();
    type Message = ();

    fn new((): Self::Props) -> Self {
        let idle = || "\u{2014}".to_owned();
        Self {
            input: None,
            animations: None,
            pressed: false,
            selected_bar: None,
            pointer: idle(),
            gesture: idle(),
            wheel: idle(),
            dropped: "Drop files here".to_owned(),
            keyboard: idle(),
            gamepad: "No controller input yet".to_owned(),
            muted: false,
            volume: 40.0,
        }
    }

    fn props(&self) -> &Self::Props {
        static PROPS: () = ();
        &PROPS
    }

    fn set_props(&mut self, (): Self::Props) {}

    fn view(&self) -> Node {
        let line = |key: &str, text: String| {
            Node::label_with_layout(key, text, LayoutStyle::new().height(SizeMode::Fixed(24)))
        };
        Node::column_with_layout(
            "lab-root",
            [
                Node::column_with_layout(
                    "lab-pad",
                    [line(
                        "lab-pad-hint",
                        "Press, drag, pinch, or scroll here; muting springs it".to_owned(),
                    )],
                    LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fixed(120)),
                    ColumnStyle::new().padding(EdgeInsets::all(12)),
                )
                // Pointer capture is requested on press (see `update`), so a
                // drag keeps reporting after it leaves the pad.
                .with_input(InputInterest::new().pointer().gestures().wheel().gamepad())
                // Milestone 27: the declared opacity changes with `pressed`,
                // and the transition says to reach the new value over 120 ms
                // instead of jumping to it. The fade costs no renders.
                .with_opacity(if self.pressed { 0.6 } else { 1.0 })
                .with_transition(AnimatedProperty::Opacity, Transition::new(FADE))
                // Focusable, so it can receive keys and IME composition.
                .with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Group)
                        .name("Input test pad")
                        .focusable(true),
                ),
                // Milestone 29: a chart drawn with Direct2D. Each bar is a
                // hit region, so a click says which bar it landed on.
                Node::canvas(
                    "lab-chart",
                    self.chart(),
                    LayoutStyle::new().width(SizeMode::Fixed(240)).height(SizeMode::Fixed(90)),
                )
                .with_input(InputInterest::new().pointer()),
                line("lab-pointer", format!("Pointer: {}", self.pointer)),
                line("lab-gesture", format!("Gesture: {}", self.gesture)),
                line("lab-wheel", format!("Wheel: {}", self.wheel)),
                line("lab-keyboard", format!("Keyboard/IME/clipboard: {}", self.keyboard)),
                line("lab-gamepad", format!("Gamepad: {}", self.gamepad)),
                // Two custom-drawn controls, one native window each. On
                // Windows these are real UI Automation elements — a check
                // box with a Toggle pattern, a slider with a RangeValue
                // pattern — that Narrator reads and operates; what it asks
                // for arrives in `update` as `AccessibilityAction`.
                Node::column_with_layout(
                    "lab-mute",
                    [line(
                        "lab-mute-text",
                        format!("[{}] Mute", if self.muted { "x" } else { " " }),
                    )],
                    LayoutStyle::new().width(SizeMode::Fixed(160)).height(SizeMode::Fixed(28)),
                    ColumnStyle::new(),
                )
                .with_input(InputInterest::new().pointer())
                .with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::CheckBox)
                        .name("Mute")
                        .checked(if self.muted {
                            CheckedState::Checked
                        } else {
                            CheckedState::Unchecked
                        })
                        .action(AccessibleActionKind::Toggle)
                        .focusable(true),
                ),
                Node::column_with_layout(
                    "lab-volume",
                    [line("lab-volume-text", format!("Volume: {:.0}", self.volume))],
                    LayoutStyle::new().width(SizeMode::Fixed(160)).height(SizeMode::Fixed(28)),
                    ColumnStyle::new(),
                )
                // Announced when it changes, wherever the person is.
                .with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Slider)
                        .name("Volume")
                        .range(0.0, 100.0, self.volume, 5.0)
                        .action(AccessibleActionKind::SetValue)
                        .live(LiveRegion::Polite)
                        .focusable(true),
                ),
                Node::column_with_layout(
                    "lab-drop",
                    [line("lab-drop-text", self.dropped.clone())],
                    LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fixed(60)),
                    ColumnStyle::new().padding(EdgeInsets::all(12)),
                )
                .with_input(InputInterest::new().drop_target()),
            ],
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::all(16)).gap(6),
        )
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        self.input = Some(context.input());
        self.animations = Some(context.animations());
        self.view()
    }

    fn update(&mut self, event: Event) {
        let Some(input) = self.input.clone() else {
            return;
        };
        match event {
            Event::PointerDown { target, pointer } if target == NodeId::from_key("lab-chart") => {
                self.selected_bar = pointer.region();
            }
            Event::PointerDown { target, .. } if target == NodeId::from_key("lab-mute") => {
                self.muted = !self.muted;
                self.bounce_pad();
            }
            Event::AccessibilityAction { action: AccessibleAction::Toggle, .. } => {
                self.muted = !self.muted;
            }
            Event::AccessibilityAction {
                action: AccessibleAction::SetRangeValue(value), ..
            } => {
                self.volume = value.get();
            }
            Event::PointerDown { pointer, .. } => {
                input.capture_pointer("lab-pad", pointer.pointer_id());
                self.pressed = true;
                self.pointer = format!("{:?} down at {:?}", pointer.kind(), pointer.position());
            }
            Event::PointerMove { pointer, .. } if !pointer.buttons().is_empty() => {
                self.pointer = format!("dragging at {:?}", pointer.position());
            }
            Event::PointerUp { pointer, .. } => {
                input.release_pointer("lab-pad", pointer.pointer_id());
                self.pressed = false;
                self.pointer = format!("up at {:?}", pointer.position());
            }
            Event::PointerCancel { .. } => {
                self.pressed = false;
                "cancelled (capture lost)".clone_into(&mut self.pointer);
            }
            Event::PointerEnter { .. } => "hovering".clone_into(&mut self.pointer),
            Event::PointerLeave { .. } => "left the pad".clone_into(&mut self.pointer),
            Event::Gesture { gesture, .. } => self.gesture = format!("{gesture:?}"),
            Event::Wheel { delta, .. } => self.wheel = format!("{delta:?}"),
            Event::KeyUp { key, .. } => self.keyboard = format!("released {key:?}"),
            Event::Composition { composition, .. } => {
                self.keyboard = format!("IME {composition:?}");
            }
            Event::Clipboard { action, .. } => self.keyboard = format!("{action:?}"),
            Event::Gamepad { gamepad, input: change, .. } => {
                self.gamepad = format!("#{gamepad}: {change:?}");
            }
            Event::DragEnter { data, .. } | Event::DragOver { data, .. } => {
                let accept = !data.files().is_empty();
                input.set_drop_effect(if accept { DropEffect::Copy } else { DropEffect::None });
            }
            Event::Drop { data, .. } => {
                let names: Vec<_> = data
                    .files()
                    .iter()
                    .filter_map(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .collect();
                self.dropped = format!("Dropped: {}", names.join(", "));
            }
            _ => {}
        }
    }
}

impl AppShell {
    fn new() -> Self {
        Self {
            panel_visible: true,
            title_index: 0,
            child_count: 0,
            child_name: String::new(),
            settings_requested: false,
            settings_opened_count: 0,
            input_lab_requested: false,
            long_list_requested: false,
            preferences_requested: false,
            scheme: None,
        }
    }

    fn panel_title(&self) -> &'static str {
        ["Counter Panel — Parent-owned title", "Counter Panel — Props changed without remounting"]
            [self.title_index]
    }
}

impl Component for AppShell {
    type Props = ();
    type Message = AppMessage;

    fn new((): Self::Props) -> Self {
        Self::new()
    }

    fn props(&self) -> &Self::Props {
        static PROPS: () = ();
        &PROPS
    }

    fn set_props(&mut self, (): Self::Props) {}

    fn view(&self) -> Node {
        Node::label("app-shell-placeholder", "Managed component tree")
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        if self.settings_requested {
            self.settings_requested = false;
            self.settings_opened_count += 1;
            // Deferred: applied by `Application` once this render finishes
            // (see `ComponentContext::windows`), then realized as a new
            // native top-level window by the platform backend.
            context.windows().open(
                SettingsWindow::new(self.settings_opened_count),
                Window::new("Rust Native UI — Settings", Size::new(360, 160)),
                None,
            );
        }

        if self.preferences_requested {
            self.preferences_requested = false;
            context.windows().open(
                Preferences::new(()),
                Window::new("Rust Native UI — Preferences", Size::new(420, 320)),
                None,
            );
        }

        if self.long_list_requested {
            self.long_list_requested = false;
            context.windows().open(
                LongList::new(()),
                Window::new("Rust Native UI — 100,000 rows", Size::new(420, 560)),
                None,
            );
        }

        if self.input_lab_requested {
            self.input_lab_requested = false;
            context.windows().open(
                InputLab::new(()),
                Window::new("Rust Native UI — Input Lab", Size::new(520, 520)),
                None,
            );
        }

        // The scheme override, when there is one, is provided to the
        // subtree; `None` follows the host.
        if let Some(scheme) = self.scheme {
            context.provide_env(&keys::COLOR_SCHEME, scheme);
        }

        let panel = if self.panel_visible {
            Some(context.child_with_props(
                "counter-panel",
                CounterPanelProps {
                    title: self.panel_title().to_owned(),
                    on_message: context.callback(),
                },
                CounterPanel::new,
            ))
        } else {
            None
        };

        Node::column_with_layout(
            "app-shell",
            [
                Node::row_with_layout(
                    "lifecycle-controls",
                    [
                        Node::button_with_layout(
                            "toggle-panel",
                            if self.panel_visible { "Unmount Panel" } else { "Mount Panel" },
                            LayoutStyle::new().height(SizeMode::Fixed(36)),
                        ),
                        Node::button_with_layout(
                            "change-title",
                            "Change Child Props",
                            LayoutStyle::new().height(SizeMode::Fixed(36)),
                        ),
                        Node::button_with_layout(
                            "toggle-scheme",
                            match self.scheme {
                                None => "Scheme: System",
                                Some(ColorScheme::Light) => "Scheme: Light",
                                Some(ColorScheme::Dark) => "Scheme: Dark",
                            },
                            LayoutStyle::new().height(SizeMode::Fixed(36)),
                        ),
                    ],
                    LayoutStyle::new().width(SizeMode::Fixed(520)).align_self(Alignment::Center),
                    RowStyle::new().gap(12).align_items(Alignment::Center),
                ),
                Node::label_with_layout(
                    "child-status",
                    format!("Child count: {} | Name: {}", self.child_count, self.child_name),
                    LayoutStyle::new().height(SizeMode::Fixed(32)),
                ),
                panel.unwrap_or_else(|| {
                    Node::label_with_layout(
                        "empty-panel",
                        "The panel is currently unmounted.",
                        LayoutStyle::new()
                            .height(SizeMode::Fixed(40))
                            .align_self(Alignment::Center),
                    )
                }),
            ],
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::all(24)).gap(16).align_items(Alignment::Center),
        )
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } => {
                if target == NodeId::from_key("toggle-panel") {
                    self.panel_visible = !self.panel_visible;
                } else if target == NodeId::from_key("change-title") {
                    self.title_index = (self.title_index + 1) % 2;
                } else if target == NodeId::from_key("toggle-scheme") {
                    self.scheme = match self.scheme {
                        None => Some(ColorScheme::Dark),
                        Some(ColorScheme::Dark) => Some(ColorScheme::Light),
                        Some(ColorScheme::Light) => None,
                    };
                }
            }
            Event::MenuAction { item, .. } => {
                if item == NodeId::from_key("view.toggle-panel") {
                    self.panel_visible = !self.panel_visible;
                } else if item == NodeId::from_key("file.new-settings-window") {
                    self.settings_requested = true;
                } else if item == NodeId::from_key("file.input-lab") {
                    self.input_lab_requested = true;
                } else if item == NodeId::from_key("file.long-list") {
                    self.long_list_requested = true;
                } else if item == NodeId::from_key("file.preferences") {
                    self.preferences_requested = true;
                }
            }
            _ => {}
        }
    }

    fn message(&mut self, message: Self::Message) {
        match message {
            AppMessage::ChildCountChanged(count) => self.child_count = count,
            AppMessage::ChildNameChanged(name) => self.child_name = name,
        }
    }
}

/// Milestone 30: state that outlives the process lives under
/// `%LOCALAPPDATA%\dev.rustnative.hello-label\state`, and the same id makes
/// the example single-instance (a second launch hands over its link).
const APP_ID: &str = "dev.rustnative.hello-label";

/// Builds the example application and runs it on this host's backend.
///
/// # Errors
///
/// The state store could not be opened, or the backend failed.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let menu = MenuBar::new([
        MenuItem::submenu(
            "file",
            "File",
            [
                MenuItem::action("file.new-settings-window", "New Settings Window"),
                MenuItem::action("file.input-lab", "Input Lab"),
                MenuItem::action("file.long-list", "100,000 Rows"),
                MenuItem::action("file.preferences", "Preferences"),
            ],
        ),
        MenuItem::submenu("view", "View", [MenuItem::action("view.toggle-panel", "Toggle Panel")]),
    ]);

    let services = Services::default().with_state_store(Arc::new(FileStateStore::for_app(APP_ID)?));
    let mut application = Application::with_services(
        AppShell::new(),
        Window::new("Rust Native UI", Size::new(640, 360)).with_menu(menu),
        services,
    );
    // The theme `app.css` describes (`rustnative_build::compile_styles`).
    application.set_theme(rustnative_core::app_theme!());
    // A component panic is caught at the Win32 callback boundary regardless
    // (unwinding across `extern "system"` is undefined behavior, so that
    // part is not a policy). What *is* a policy is what happens next, and
    // the framework defaults to ending the application because that is the
    // only response that cannot keep running on state a panic already
    // disproved.
    //
    // This example opts into closing just the offending window instead,
    // which is the right trade for a multi-window app where the other
    // windows hold independent state — see `rustnative_core::panic` for the
    // full reasoning.
    application.set_panic_policy(PanicPolicy::CloseWindow);
    application.open_window(
        AuxiliaryWindow,
        Window::new("Rust Native UI — Auxiliary", Size::new(320, 160)),
        None,
    );

    HostPlatform::new().with_app_id(APP_ID).run(&mut application)?;

    Ok(())
}

#[cfg(target_os = "android")]
rustnative_android::export_main!(run);
