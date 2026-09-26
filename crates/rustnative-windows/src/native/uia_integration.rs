//! Milestone 26 native integration tests: a real UI Automation client
//! reading and operating this backend's providers.
//!
//! Each test runs the client — `IUIAutomation`, exactly what Narrator,
//! NVDA, Inspect, and test-automation tools use — on a worker thread in a
//! multithreaded apartment, while the test thread keeps pumping the UI
//! thread's messages through the production loop. That is the same
//! arrangement a screen reader in another process produces: its calls are
//! marshaled to the UI thread's apartment and run from the message loop,
//! and `WM_GETOBJECT` arrives as a cross-thread sent message. So these
//! tests exercise the real round trip — provider lookup, COM marshaling,
//! pattern calls turning into component events, re-render, and the
//! provider reporting the component's new state — not a direct call into
//! the provider objects.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, AccessibleAction, AccessibleActionKind, Application,
    CheckedState, ColumnStyle, Component, Event, LayoutStyle, LiveRegion, Node, NodeId, Rect, Size,
    SizeMode, VirtualElement, Window, WindowId,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationEventHandler,
    IUIAutomationEventHandler_Impl, IUIAutomationInvokePattern, IUIAutomationRangeValuePattern,
    IUIAutomationTogglePattern, ToggleState_Off, ToggleState_On, TreeScope_Children,
    TreeScope_Subtree, UIA_ButtonControlTypeId, UIA_CheckBoxControlTypeId, UIA_CustomControlTypeId,
    UIA_EVENT_ID, UIA_InvokePatternId, UIA_LiveRegionChangedEventId, UIA_PositionInSetPropertyId,
    UIA_RangeValuePatternId, UIA_SizeOfSetPropertyId, UIA_SliderControlTypeId, UIA_TogglePatternId,
};
use windows::core::{Ref, implement};

use super::harness::NativeHarness;

type Log = Rc<RefCell<Vec<String>>>;

#[derive(Clone, PartialEq)]
struct Props {
    log: Log,
}

/// Every M26 surface in one component: overridden names, a labelled field,
/// a custom check box, a custom slider, a live status line, a positioned
/// list item, and a canvas with virtual elements.
struct Probe {
    props: Props,
    agreed: bool,
    volume: f32,
    status: String,
    bars: Vec<&'static str>,
}

fn fixed(width: i32, height: i32) -> LayoutStyle {
    LayoutStyle::new().width(SizeMode::Fixed(width)).height(SizeMode::Fixed(height))
}

impl Component for Probe {
    type Props = Props;
    type Message = ();

    fn new(props: Props) -> Self {
        Self {
            props,
            agreed: false,
            volume: 30.0,
            status: "Idle".into(),
            bars: vec!["bar-a", "bar-b"],
        }
    }
    fn props(&self) -> &Props {
        &self.props
    }
    fn set_props(&mut self, props: Props) {
        self.props = props;
    }

    fn view(&self) -> Node {
        let mut chart = AccessibilityInfo::new(AccessibilityRole::Canvas).name("Sales chart");
        for (index, bar) in (0..).zip(&self.bars) {
            chart = chart.element(VirtualElement::new(
                bar,
                AccessibilityInfo::new(AccessibilityRole::Button)
                    .name(format!("Bar {}", bar.trim_start_matches("bar-").to_uppercase()))
                    .action(AccessibleActionKind::Invoke),
                Rect::new(index * 30, 0, 20, 20),
            ));
        }
        Node::column(
            "root",
            [
                Node::label("caption", "Email address"),
                Node::text_input("email", "").with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::TextInput)
                        .labelled_by("caption")
                        .description("We never share it")
                        .automation_id("email-field")
                        .focusable(true),
                ),
                Node::button("submit", "OK").with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Button)
                        .name("Confirm the payment")
                        .focusable(true),
                ),
                Node::column_with_layout("agree", [], fixed(120, 24), ColumnStyle::new())
                    .with_accessibility(
                        AccessibilityInfo::new(AccessibilityRole::CheckBox)
                            .name("I agree")
                            .checked(if self.agreed {
                                CheckedState::Checked
                            } else {
                                CheckedState::Unchecked
                            })
                            .action(AccessibleActionKind::Toggle)
                            .focusable(true),
                    ),
                Node::column_with_layout("volume", [], fixed(120, 24), ColumnStyle::new())
                    .with_accessibility(
                        AccessibilityInfo::new(AccessibilityRole::Slider)
                            .name("Volume")
                            .range(0.0, 100.0, self.volume, 5.0)
                            .action(AccessibleActionKind::SetValue)
                            .focusable(true),
                    ),
                Node::label("status", self.status.clone()).with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Status).live(LiveRegion::Polite),
                ),
                Node::label("item", "Third").with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::ListItem).position_in_set(3, 250),
                ),
                Node::column_with_layout("chart", [], fixed(120, 40), ColumnStyle::new())
                    .with_accessibility(chart),
                Node::button("advance", "Advance"),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        match &event {
            Event::AccessibilityAction { action: AccessibleAction::Toggle, .. } => {
                self.agreed = !self.agreed;
            }
            Event::AccessibilityAction {
                action: AccessibleAction::SetRangeValue(value), ..
            } => {
                self.volume = value.get();
            }
            Event::Click { target } if *target == NodeId::from_key("advance") => {
                self.status = "Saved".into();
                self.bars.retain(|bar| *bar != "bar-b");
            }
            _ => {}
        }
        if let Event::AccessibilityAction { target, element, action } = event {
            let element =
                element.map(|id| if id == NodeId::from_key("bar-a") { "bar-a" } else { "?" });
            let target = if target == NodeId::from_key("chart") {
                "chart".to_owned()
            } else {
                format!("{target:?}")
            };
            self.props.log.borrow_mut().push(format!("{target}:{element:?}:{action:?}"));
        }
    }
}

fn application(log: &Log) -> Application {
    Application::new(
        Probe::new(Props { log: log.clone() }),
        Window::new("uia", Size::new(480, 640)),
    )
}

/// Runs `client` on a fresh multithreaded-apartment thread with a UI
/// Automation client, pumping the UI thread until it finishes.
fn with_client<R: Send + 'static>(
    harness: &mut NativeHarness,
    client: impl FnOnce(&IUIAutomation) -> R + Send + 'static,
) -> R {
    let (sender, receiver) = channel();
    let worker = std::thread::spawn(move || {
        // SAFETY: first COM call on this new thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().expect("MTA");
        // SAFETY: creates the in-process UI Automation client.
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
                .expect("the UI Automation client");
        let result = client(&automation);
        drop(automation);
        // SAFETY: pairs the initialization above, after every COM object
        // this thread created has been released.
        unsafe { CoUninitialize() };
        let _ = sender.send(result);
    });
    let result = pump_until(harness, &receiver, Duration::from_secs(30));
    worker.join().expect("the client thread must not panic");
    result
}

fn pump_until<T>(harness: &mut NativeHarness, receiver: &Receiver<T>, timeout: Duration) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        harness.pump();
        if let Ok(value) = receiver.try_recv() {
            return value;
        }
        assert!(Instant::now() < deadline, "the UI Automation client did not finish in time");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn element(automation: &IUIAutomation, hwnd: usize) -> IUIAutomationElement {
    // SAFETY: `hwnd` is a live window of the application under test.
    unsafe { automation.ElementFromHandle(HWND(hwnd as *mut core::ffi::c_void)) }
        .expect("an element for a realized node's window")
}

fn name(element: &IUIAutomationElement) -> String {
    // SAFETY: a plain property read.
    unsafe { element.CurrentName() }.expect("a name").to_string()
}

fn hwnd(harness: &NativeHarness, key: &str) -> usize {
    harness.expect_control(WindowId::PRIMARY, key) as usize
}

/// Names, descriptions, control types, automation ids, the labelled-by
/// relationship, and set position, all read back by a real client.
///
/// Catches: a provider UI Automation never asks for (no `WM_GETOBJECT`
/// hook), overrides that lose to the native proxy (or the reverse: a
/// native button's type replaced by nothing), relationships left pointing
/// at local keys, and set position lost for virtualized items.
#[test]
fn native_uia_reads_names_types_relationships_and_positions() {
    let log = Log::default();
    let mut application = application(&log);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let (submit, email, agree, item) = (
        hwnd(&harness, "submit"),
        hwnd(&harness, "email"),
        hwnd(&harness, "agree"),
        hwnd(&harness, "item"),
    );

    let read = with_client(&mut harness, move |automation| {
        let submit = element(automation, submit);
        let email = element(automation, email);
        let agree = element(automation, agree);
        let item = element(automation, item);
        // SAFETY: (whole block) plain property reads on live elements.
        unsafe {
            let label = email.CurrentLabeledBy().expect("a labelled-by element");
            let position = item.GetCurrentPropertyValue(UIA_PositionInSetPropertyId).unwrap();
            let size = item.GetCurrentPropertyValue(UIA_SizeOfSetPropertyId).unwrap();
            (
                name(&submit),
                submit.CurrentControlType().unwrap(),
                email.CurrentHelpText().unwrap().to_string(),
                email.CurrentAutomationId().unwrap().to_string(),
                name(&email),
                name(&label),
                name(&agree),
                agree.CurrentControlType().unwrap(),
                i32::try_from(&position).unwrap(),
                i32::try_from(&size).unwrap(),
            )
        }
    });
    assert_eq!(read.0, "Confirm the payment", "an overridden name beats the caption");
    assert_eq!(read.1, UIA_ButtonControlTypeId);
    assert_eq!(read.2, "We never share it");
    assert_eq!(read.3, "email-field");
    assert_eq!(read.4, "Email address", "a field labelled by a caption is named by it");
    assert_eq!(read.5, "Email address", "LabeledBy resolves to the caption's element");
    assert_eq!(read.6, "I agree");
    assert_eq!(read.7, UIA_CheckBoxControlTypeId, "a custom container reports its declared role");
    assert_eq!((read.8, read.9), (3, 250), "a virtualized item reports its true position");
}

/// A client toggles the custom check box and sets the custom slider; each
/// arrives as the component's `AccessibilityAction`, and the provider then
/// reports the state the component rendered.
///
/// Catches: patterns offered but never wired to the component, a pattern
/// that reports its own assumed state instead of the component's, and
/// out-of-range values passed through.
#[test]
fn native_uia_patterns_round_trip_through_the_component() {
    let log = Log::default();
    let mut application = application(&log);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let (agree, volume) = (hwnd(&harness, "agree"), hwnd(&harness, "volume"));

    let observed = with_client(&mut harness, move |automation| {
        let agree = element(automation, agree);
        let volume = element(automation, volume);
        // SAFETY: (whole block) pattern calls on live elements.
        unsafe {
            let toggle: IUIAutomationTogglePattern =
                agree.GetCurrentPatternAs(UIA_TogglePatternId).expect("a toggle pattern");
            let before = toggle.CurrentToggleState().unwrap();
            toggle.Toggle().expect("toggle");
            let after = toggle.CurrentToggleState().unwrap();

            let range: IUIAutomationRangeValuePattern =
                volume.GetCurrentPatternAs(UIA_RangeValuePatternId).expect("a range pattern");
            let initial = range.CurrentValue().unwrap();
            let maximum = range.CurrentMaximum().unwrap();
            range.SetValue(42.0).expect("set in range");
            let set = range.CurrentValue().unwrap();
            let rejected = range.SetValue(500.0).is_err();
            (before, after, initial, maximum, set, rejected, volume.CurrentControlType().unwrap())
        }
    });
    assert_eq!((observed.0, observed.1), (ToggleState_Off, ToggleState_On));
    assert!((observed.2 - 30.0).abs() < f64::EPSILON);
    assert!((observed.3 - 100.0).abs() < f64::EPSILON);
    assert!(
        (observed.4 - 42.0).abs() < f64::EPSILON,
        "the provider reports the component's new value"
    );
    assert!(observed.5, "an out-of-range value is refused, not clamped silently");
    assert_eq!(observed.6, UIA_SliderControlTypeId);
    let log = log.borrow();
    assert!(log.iter().any(|entry| entry.ends_with("None:Toggle")), "{log:?}");
    assert!(log.iter().any(|entry| entry.ends_with("None:SetRangeValue(Scalar(42.0))")), "{log:?}");
}

/// A canvas' virtual elements are navigable children with their own names
/// and patterns; invoking one reaches the component with its element id;
/// and an element the component stops declaring becomes unavailable to the
/// client that still holds it.
///
/// Catches: fragment navigation that never reaches the elements, invoke
/// actions that lose the element id, and a stale fragment that keeps
/// answering for an element that no longer exists.
#[test]
fn native_uia_virtual_elements_are_navigable_invokable_and_disconnected() {
    let log = Log::default();
    let mut application = application(&log);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let chart = hwnd(&harness, "chart");
    let advance = "advance";

    let (to_ui, from_client) = channel::<(String, Vec<String>)>();
    let (to_client, from_ui) = channel::<()>();
    let (done, finished) = channel::<(bool, bool)>();
    let worker = std::thread::spawn(move || {
        client_thread(chart, &to_ui, &from_ui, &done);
    });

    let (kind, names) = pump_until(&mut harness, &from_client, Duration::from_secs(30));
    assert_eq!(kind, format!("{UIA_CustomControlTypeId:?}"));
    assert_eq!(names, ["Bar A", "Bar B"]);
    assert!(
        log.borrow().iter().any(|entry| entry == "chart:Some(\"bar-a\"):Invoke"),
        "invoking a virtual element reaches the component with its id: {:?}",
        log.borrow()
    );

    // The component drops `bar-b` (and changes its live status line).
    harness.click(WindowId::PRIMARY, advance);
    to_client.send(()).unwrap();
    let (stale_is_gone, remaining_is_one) =
        pump_until(&mut harness, &finished, Duration::from_secs(30));
    worker.join().unwrap();
    assert!(stale_is_gone, "a removed element answers UIA_E_ELEMENTNOTAVAILABLE");
    assert!(remaining_is_one, "the canvas now has exactly one child");
}

fn client_thread(
    chart: usize,
    to_ui: &Sender<(String, Vec<String>)>,
    from_ui: &Receiver<()>,
    done: &Sender<(bool, bool)>,
) {
    // SAFETY: first COM call on this thread.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().expect("MTA");
    {
        // SAFETY: (whole block) UI Automation client calls on live objects.
        unsafe {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).unwrap();
            let chart = element(&automation, chart);
            // The control view — what assistive technology navigates.
            let all = automation.ControlViewCondition().unwrap();
            let children = chart.FindAll(TreeScope_Children, &all).unwrap();
            let count = children.Length().unwrap();
            let elements: Vec<_> = (0..count).map(|i| children.GetElement(i).unwrap()).collect();
            let names = elements.iter().map(name).collect();
            let invoke: IUIAutomationInvokePattern =
                elements[0].GetCurrentPatternAs(UIA_InvokePatternId).expect("an invoke pattern");
            invoke.Invoke().expect("invoke");
            to_ui.send((format!("{:?}", chart.CurrentControlType().unwrap()), names)).unwrap();

            from_ui.recv().unwrap();
            let stale = elements[1].CurrentName();
            let stale_is_gone = stale.is_err();
            let remaining = chart.FindAll(TreeScope_Children, &all).unwrap().Length().unwrap();
            drop(elements);
            done.send((stale_is_gone, remaining == 1)).unwrap();
        }
    }
    // SAFETY: pairs the initialization above.
    unsafe { CoUninitialize() };
}

/// A live-region change is announced to a client listening for it.
///
/// Catches: notifications raised inside the runtime borrow (a reentrancy
/// hazard this bridge is designed around) or never raised at all.
#[test]
#[allow(
    clippy::ref_as_ptr,
    clippy::inline_always,
    reason = "`windows::core::implement` generates the listener's COM plumbing"
)]
fn native_uia_live_region_change_is_announced() {
    #[implement(IUIAutomationEventHandler)]
    struct Listener(std::sync::Mutex<Sender<String>>);

    impl IUIAutomationEventHandler_Impl for Listener_Impl {
        fn HandleAutomationEvent(
            &self,
            sender: Ref<'_, IUIAutomationElement>,
            _id: UIA_EVENT_ID,
        ) -> windows::core::Result<()> {
            let text = sender.as_ref().map(name).unwrap_or_default();
            let _ = self.0.lock().unwrap().send(text);
            Ok(())
        }
    }

    let log = Log::default();
    let mut application = application(&log);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let top = harness.hwnd(WindowId::PRIMARY) as usize;

    let (events, heard) = channel::<String>();
    let (registered, is_registered) = channel::<()>();
    let (stop, should_stop) = channel::<()>();
    let worker = std::thread::spawn(move || {
        // SAFETY: (whole block) COM setup and client calls on this thread.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().expect("MTA");
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).unwrap();
            let root = element(&automation, top);
            let listener: IUIAutomationEventHandler =
                Listener(std::sync::Mutex::new(events)).into();
            automation
                .AddAutomationEventHandler(
                    UIA_LiveRegionChangedEventId,
                    &root,
                    TreeScope_Subtree,
                    None,
                    &listener,
                )
                .expect("subscribing to live-region changes");
            registered.send(()).unwrap();
            should_stop.recv().unwrap();
            automation.RemoveAllEventHandlers().unwrap();
            drop(listener);
            drop(root);
            drop(automation);
            CoUninitialize();
        }
    });
    pump_until(&mut harness, &is_registered, Duration::from_secs(30));

    harness.click(WindowId::PRIMARY, "advance");
    let announced = pump_until(&mut harness, &heard, Duration::from_secs(30));
    stop.send(()).unwrap();
    // Keep pumping while the client unsubscribes: removing handlers calls
    // back into this apartment.
    let deadline = Instant::now() + Duration::from_secs(30);
    while !worker.is_finished() {
        harness.pump();
        assert!(Instant::now() < deadline, "the client did not unsubscribe in time");
        std::thread::sleep(Duration::from_millis(2));
    }
    worker.join().unwrap();
    assert_eq!(announced, "Saved", "the live region is announced with its new text");
}
