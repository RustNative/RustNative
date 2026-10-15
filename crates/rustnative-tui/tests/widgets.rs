//! Every node kind as a drawn widget, through the synthetic terminal: what
//! it shows (goldens of the emulated screen) and how it behaves (keys and
//! the mouse, as bytes, to the portable events a native control reports).
//!
//! `RUSTNATIVE_BLESS=1 cargo test -p rustnative-tui --test widgets` rewrites
//! the goldens in `tests/goldens/`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

mod common;

use std::path::Path;

use rustnative_core::{CalendarDate, Event, KeyCode, KeyModifiers, NodeId};
use rustnative_headless::check_golden;
use rustnative_tui::TuiHarness;

fn golden(name: &str, actual: &str) {
    check_golden(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens").join(name), actual);
}

fn launch() -> (common::Log, TuiHarness) {
    let (log, root) = common::form();
    let terminal = TuiHarness::launch(common::window("Form"), 50, 36, root);
    (log, terminal)
}

/// Tabs until `key` has focus.
fn focus(terminal: &mut TuiHarness, key: &str) {
    for _ in 0..40 {
        if terminal.focused_key().as_deref() == Some(key) {
            return;
        }
        terminal.press_tab();
    }
    panic!("`{key}` never took focus; focused: {:?}", terminal.focused_key());
}

fn press(terminal: &mut TuiHarness, key: KeyCode) {
    terminal.press(key, KeyModifiers::default());
}

fn id(key: &str) -> NodeId {
    NodeId::from_key(key)
}

#[test]
fn every_node_kind_draws_as_its_golden_says() {
    let (_, terminal) = launch();
    golden("form.txt", &terminal.golden());
}

#[test]
fn focus_shows_on_the_focused_widget_and_the_cursor_follows_it() {
    let (_, mut terminal) = launch();
    focus(&mut terminal, "button");
    golden("form-button-focused.txt", &terminal.golden());
    let button = terminal.node("button").unwrap();
    let ((x, y), shown) = terminal.cursor();
    assert_eq!(y, button.cells.y, "the cursor is on the focused control's row");
    assert!(x >= button.cells.x && x < button.cells.right());
    assert!(!shown, "a control's cursor is placed, not shown");
}

#[test]
fn traversal_visits_focusable_enabled_nodes_in_order_and_wraps() {
    let (_, mut terminal) = launch();
    let mut visited = Vec::new();
    for _ in 0..17 {
        terminal.press_tab();
        visited.push(terminal.focused_key().unwrap_or_default());
    }
    assert_eq!(
        visited,
        [
            "button", "text", "check", "radio-a", "radio-b", "toggle", "slider", "select", "list",
            "date", "spin", "link", "tabs", "notes", "button", "text", "check",
        ],
        "progress, the rule, the labels, and the disabled button are skipped"
    );
    terminal.press(KeyCode::Tab, KeyModifiers { shift: true, ..KeyModifiers::default() });
    assert_eq!(terminal.focused_key().as_deref(), Some("text"), "Shift+Tab goes back");
}

#[test]
fn a_button_activates_by_enter_space_and_a_click_but_not_while_disabled() {
    let (log, mut terminal) = launch();
    focus(&mut terminal, "button");
    press(&mut terminal, KeyCode::Enter);
    press(&mut terminal, KeyCode::Space);
    terminal.click_key("button");
    assert!(terminal.text().contains("clicks 3"), "{}", terminal.text());
    terminal.click_key("off");
    assert!(
        !log.borrow().iter().any(|e| matches!(e, Event::Click { target } if *target == id("off"))),
        "a disabled button is never activated"
    );
}

#[test]
fn a_text_field_edits_like_a_native_one() {
    let (log, mut terminal) = launch();
    terminal.click_key("text");
    assert_eq!(terminal.focused_key().as_deref(), Some("text"));
    terminal.type_text("hello wörld");
    assert!(terminal.text().contains("hello wörld"));
    let ((x, y), shown) = terminal.cursor();
    let field = terminal.node("text").unwrap();
    assert!(shown, "a field's caret is the terminal's cursor");
    assert_eq!((x, y), (field.cells.x + 11, field.cells.y));
    // Ctrl+Backspace deletes the word; Home, Shift+End, typing replaces all.
    terminal.press(KeyCode::Backspace, KeyModifiers { ctrl: true, ..KeyModifiers::default() });
    assert_eq!(terminal.model().value(id("text")), "hello ", "the word went, its space stayed");
    press(&mut terminal, KeyCode::Home);
    terminal.press(KeyCode::End, KeyModifiers { shift: true, ..KeyModifiers::default() });
    terminal.type_text("日本");
    let last = log
        .borrow()
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::TextChanged { target, value } if *target == id("text") => Some(value.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(last, "日本");
    // A paste arrives whole.
    terminal.paste(" pasted\nline");
    assert!(
        terminal.text().contains("日本 pasted line"),
        "a one-line field takes breaks as spaces"
    );
    golden("form-text-editing.txt", &terminal.golden());
}

#[test]
fn a_long_value_scrolls_within_its_field() {
    let (_, mut terminal) = launch();
    terminal.click_key("text");
    let long = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    terminal.paste(long);
    let field = terminal.node("text").unwrap();
    let row = terminal.screen().row_text(u16::try_from(field.cells.y).unwrap());
    assert!(row.contains("XYZ") && !row.contains("abc"), "scrolled to the caret: {row:?}");
    press(&mut terminal, KeyCode::Home);
    let row = terminal.screen().row_text(u16::try_from(field.cells.y).unwrap());
    assert!(row.contains("abc"), "back to the start: {row:?}");
}

#[test]
fn check_box_radio_and_switch_toggle_by_key_and_click() {
    let (_, mut terminal) = launch();
    focus(&mut terminal, "check");
    press(&mut terminal, KeyCode::Space);
    assert!(terminal.text().contains("[x] Check"));
    terminal.click_key("check");
    assert!(terminal.text().contains("[ ] Check"));
    terminal.click_key("radio-b");
    assert!(terminal.text().contains("( ) A") && terminal.text().contains("(*) B"));
    focus(&mut terminal, "radio-a");
    press(&mut terminal, KeyCode::Space);
    assert!(terminal.text().contains("(*) A"));
    focus(&mut terminal, "toggle");
    press(&mut terminal, KeyCode::Enter);
    assert!(terminal.text().contains("[on]  Switch"), "{}", terminal.text());
}

#[test]
fn a_slider_moves_by_key_click_and_drag() {
    let (_, mut terminal) = launch();
    focus(&mut terminal, "slider");
    press(&mut terminal, KeyCode::ArrowRight);
    assert!(terminal.text().contains(" 51"));
    press(&mut terminal, KeyCode::PageDown);
    assert!(terminal.text().contains(" 41"));
    press(&mut terminal, KeyCode::End);
    assert!(terminal.text().contains(" 100"));
    press(&mut terminal, KeyCode::Home);
    let slider = terminal.node("slider").unwrap();
    let (x, y) = (u16::try_from(slider.cells.x).unwrap(), u16::try_from(slider.cells.y).unwrap());
    terminal.click(x, y);
    assert!(terminal.text().contains("● ") || terminal.text().contains(" 0"));
    terminal.drag((x, y), (x + 23, y));
    assert!(
        !terminal.text().contains("─ 0\n") && !terminal.screen().row_text(y).ends_with(" 0"),
        "dragging moved it: {}",
        terminal.screen().row_text(y)
    );
}

#[test]
fn a_select_opens_as_a_list_and_chooses() {
    let (log, mut terminal) = launch();
    focus(&mut terminal, "select");
    press(&mut terminal, KeyCode::ArrowDown);
    assert!(terminal.text().contains("Green"), "Down changes the choice while closed");
    press(&mut terminal, KeyCode::Enter);
    golden("form-select-open.txt", &terminal.golden());
    press(&mut terminal, KeyCode::ArrowDown);
    press(&mut terminal, KeyCode::Enter);
    assert!(
        terminal
            .screen()
            .row_text(u16::try_from(terminal.node("select").unwrap().cells.y).unwrap())
            .contains("Blue")
    );
    // Open by click, close with Escape: nothing chosen.
    terminal.click_key("select");
    let before = common::changes(&log).len();
    press(&mut terminal, KeyCode::Escape);
    assert_eq!(common::changes(&log).len(), before, "Escape chooses nothing");
    assert!(!terminal.text().contains("┌"), "the list closed");
    // Open by click, choose by click.
    terminal.click_key("select");
    let select = terminal.node("select").unwrap();
    let row = u16::try_from(select.cells.bottom() + 1).unwrap();
    terminal.click(u16::try_from(select.cells.x + 2).unwrap(), row);
    let chosen = log.borrow().iter().rev().find_map(|event| match event {
        Event::SelectionChanged { target, index } if *target == id("select") => *index,
        _ => None,
    });
    assert_eq!(chosen, Some(0), "the list's first row is Red");
}

#[test]
fn a_list_box_selects_by_arrow_and_click_and_scrolls() {
    let (_, mut terminal) = launch();
    focus(&mut terminal, "list");
    press(&mut terminal, KeyCode::ArrowDown);
    assert!(terminal.text().contains(">Item 1"));
    press(&mut terminal, KeyCode::End);
    assert!(terminal.text().contains(">Item 6"), "End reaches the last item, scrolled into view");
    let list = terminal.node("list").unwrap();
    terminal.wheel(
        u16::try_from(list.cells.x + 1).unwrap(),
        u16::try_from(list.cells.y).unwrap(),
        false,
    );
    terminal.click(u16::try_from(list.cells.x + 1).unwrap(), u16::try_from(list.cells.y).unwrap());
    assert!(terminal.text().contains(">Item"), "{}", terminal.text());
}

#[test]
fn a_date_picker_steps_each_part_and_stays_a_date() {
    let (_, mut terminal) = launch();
    focus(&mut terminal, "date");
    press(&mut terminal, KeyCode::ArrowRight);
    press(&mut terminal, KeyCode::ArrowUp);
    assert!(
        terminal.text().contains("2026-02-28"),
        "31 January + a month clamps to 28 February: {}",
        terminal.text()
    );
    press(&mut terminal, KeyCode::ArrowLeft);
    press(&mut terminal, KeyCode::ArrowDown);
    assert!(terminal.text().contains("2025-02-28"));
    press(&mut terminal, KeyCode::ArrowRight);
    press(&mut terminal, KeyCode::ArrowRight);
    press(&mut terminal, KeyCode::ArrowUp);
    assert!(
        terminal.text().contains("2025-02-01") || terminal.text().contains("2025-03-01"),
        "{}",
        terminal.text()
    );
    let _ = CalendarDate::new(2025, 3, 1);
}

#[test]
fn a_spinner_steps_within_its_range() {
    let (_, mut terminal) = launch();
    focus(&mut terminal, "spin");
    press(&mut terminal, KeyCode::ArrowUp);
    assert!(terminal.text().contains("< 6 >"), "{}", terminal.text());
    press(&mut terminal, KeyCode::End);
    press(&mut terminal, KeyCode::ArrowUp);
    assert!(terminal.text().contains("< 9 >"), "clamped at the maximum");
    let spin = terminal.node("spin").unwrap();
    terminal.click(u16::try_from(spin.cells.x).unwrap(), u16::try_from(spin.cells.y).unwrap());
    assert!(terminal.text().contains("< 8 >"), "the left half steps down: {}", terminal.text());
}

#[test]
fn tabs_choose_by_arrow_and_click() {
    let (_, mut terminal) = launch();
    focus(&mut terminal, "tabs");
    press(&mut terminal, KeyCode::ArrowRight);
    let log_row = |terminal: &TuiHarness| {
        let tabs = terminal.node("tabs").unwrap();
        terminal
            .screen()
            .row(u16::try_from(tabs.cells.y).unwrap())
            .iter()
            .map(|cell| cell.style.attrs.bits())
            .collect::<Vec<_>>()
    };
    let reversed_second = log_row(&terminal);
    assert!(reversed_second.iter().any(|bits| *bits != 0));
    let tabs = terminal.node("tabs").unwrap();
    terminal.click(u16::try_from(tabs.cells.x + 1).unwrap(), u16::try_from(tabs.cells.y).unwrap());
    assert_ne!(log_row(&terminal), reversed_second, "clicking the first tab chose it");
}

#[test]
fn a_link_activates_by_enter_and_click() {
    let (_, mut terminal) = launch();
    focus(&mut terminal, "link");
    press(&mut terminal, KeyCode::Enter);
    terminal.click_key("link");
    assert!(terminal.text().contains("links 2"), "{}", terminal.text());
}

#[test]
fn multi_line_text_wraps_breaks_lines_and_moves_by_line() {
    let (_, mut terminal) = launch();
    terminal.click_key("notes");
    terminal.type_text("first line");
    press(&mut terminal, KeyCode::Enter);
    terminal.type_text("second");
    press(&mut terminal, KeyCode::ArrowUp);
    terminal.type_text("!");
    let notes = terminal.node("notes").unwrap();
    let first = terminal.screen().row_text(u16::try_from(notes.cells.y).unwrap());
    let second = terminal.screen().row_text(u16::try_from(notes.cells.y + 1).unwrap());
    assert!(first.contains("first ") && first.contains('!'), "{first:?}");
    assert!(second.contains("second"), "{second:?}");
}

#[test]
fn ctrl_c_closes_the_window_unless_a_field_copies() {
    let (_, mut terminal) = launch();
    terminal.click_key("text");
    terminal.type_text("copy me");
    terminal.press(KeyCode::Character('a'), KeyModifiers { ctrl: true, ..KeyModifiers::default() });
    terminal.press(KeyCode::Character('c'), KeyModifiers { ctrl: true, ..KeyModifiers::default() });
    assert!(!terminal.session().is_finished(), "Ctrl+C with a selection copies");
    assert_eq!(terminal.session().channel().local_clipboard().as_deref(), Some("copy me"));
    let copied =
        terminal.terminal().with_emulator(|emulator| emulator.clipboard().map(str::to_owned));
    assert_eq!(
        copied.as_deref(),
        Some("copy me"),
        "and reached the terminal's clipboard through OSC 52"
    );
    focus(&mut terminal, "button");
    terminal.press(KeyCode::Character('c'), KeyModifiers { ctrl: true, ..KeyModifiers::default() });
    assert!(terminal.session().is_finished(), "elsewhere it closes the window");
}
