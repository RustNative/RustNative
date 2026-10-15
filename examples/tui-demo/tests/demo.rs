//! The demo on the synthetic terminal: the same steps the conhost test
//! takes in a real console window, everywhere.

use rustnative_core::{Component, KeyCode, KeyModifiers, Size, Window};
use rustnative_tui::TuiHarness;
use tui_demo::Todo;

fn launch() -> TuiHarness {
    TuiHarness::launch(Window::new("To do", Size::new(0, 0)), 100, 30, || Todo::new(()))
}

#[test]
fn adding_a_task_chooses_it_and_the_list_moves_from_there() {
    let mut terminal = launch();
    assert!(terminal.text().contains("1 of 3 done"));
    terminal.click_key("draft");
    terminal.type_text("Buy milk");
    terminal.press_enter();
    assert!(terminal.text().contains("1 of 4 done") && terminal.text().contains("Buy milk"));
    assert!(terminal.text().contains("Done: Buy milk"), "the new task is chosen");
    // The list's cursor followed the application's choice: Up goes to the
    // task above the new one, not above where the cursor last was.
    terminal.click_key("tasks");
    terminal.press(KeyCode::ArrowUp, KeyModifiers::default());
    assert!(terminal.text().contains("Done: Resize the terminal"), "{}", terminal.text());
    terminal.press_tab();
    terminal.press(KeyCode::Space, KeyModifiers::default());
    assert!(terminal.text().contains("2 of 4 done"), "{}", terminal.text());
}

#[test]
fn ctrl_s_adds_through_its_command_and_the_filter_tabs_filter() {
    let mut terminal = launch();
    terminal.click_key("draft");
    terminal.type_text("Water plants");
    terminal.press(KeyCode::Character('s'), KeyModifiers { ctrl: true, ..KeyModifiers::default() });
    assert!(terminal.text().contains("Water plants"));
    let filter = terminal.node("filter").expect("the filter");
    // "Done" is the third tab.
    let spans = rustnative_tui::paint::tab_spans(
        &["All".to_owned(), "Open".to_owned(), "Done".to_owned()],
        rustnative_tui::Width::default(),
    );
    let column = u16::try_from(filter.cells.x + spans[2].0 + 1).unwrap_or(0);
    terminal.click(column, u16::try_from(filter.cells.y).unwrap_or(0));
    assert!(!terminal.text().contains("· Water plants"), "only done tasks are listed");
    assert!(terminal.text().contains("Read the terminal backend's guide"));
}
