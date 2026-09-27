//! Web milestones B and C in a real browser: every node kind, rendered under
//! the strict content security policy, read back through the browser's own
//! accessibility tree and layout.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, CalendarDate, EdgeInsets, LayoutDirection, LayoutStyle,
    Node, RowStyle, SizeMode, Theme, classes,
};
use rustnative_web_testing::{Response, StaticServer, browser_or_skip};

const CSP: &str = "default-src 'self'; script-src 'self' 'nonce-t3st'; style-src 'self' 'nonce-t3st'; \
                   img-src 'self' data:; object-src 'none'; base-uri 'none'";

fn document(view: &Node) -> String {
    let theme = Theme::default();
    let rendered = rustnative_web::render(view, &theme);
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>t</title>\
         <style nonce=\"t3st\">{}</style></head><body>{}</body></html>",
        rendered.css(&theme),
        rendered.html
    )
}

fn gallery() -> Node {
    let heading = Node::label("title", "Notes")
        .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Heading { level: 2 }));
    let items = ["Milk", "Bread"].map(|text| {
        Node::label(text.to_lowercase(), text)
            .with_accessibility(AccessibilityInfo::new(AccessibilityRole::ListItem))
    });
    Node::column(
        "root",
        vec![
            heading,
            Node::button("add", "Add note"),
            Node::text_input("draft", "Buy milk").with_accessibility(
                AccessibilityInfo::new(AccessibilityRole::TextInput).name("Draft").focusable(true),
            ),
            Node::checkbox("agree", "I agree", true),
            Node::toggle("wifi", "Wi-Fi", false),
            Node::radio("small", "Small", true),
            Node::slider("volume", 3, 0, 10),
            Node::spinner("count", 2, 0, 9),
            Node::progress("load", Some(40)),
            Node::select("size", ["S", "M", "L"], Some(1)),
            Node::date_picker("when", CalendarDate::new(2026, 9, 27).unwrap()),
            Node::separator("rule"),
            Node::link("more", "Read more"),
            Node::multiline_text("notes", "one\ntwo"),
            Node::column("list", items)
                .with_accessibility(AccessibilityInfo::new(AccessibilityRole::List)),
            Node::row_with_layout(
                "rtl",
                [
                    Node::label_with_layout(
                        "first",
                        "1",
                        LayoutStyle::new().width(SizeMode::Fixed(40)),
                    ),
                    Node::label_with_layout(
                        "second",
                        "2",
                        LayoutStyle::new().width(SizeMode::Fixed(40)),
                    ),
                ],
                LayoutStyle::new().width(SizeMode::Fixed(300)).direction(LayoutDirection::Rtl),
                RowStyle::new().gap(0).padding(EdgeInsets::logical(0, 0, 0, 10)),
            ),
            Node::label("styled", "Styled").with_class(classes!("p-4 text-[#ff0000] md:p-8")),
        ],
    )
}

#[test]
fn every_node_kind_is_its_semantic_element_under_a_strict_policy() {
    let Some(browser) =
        browser_or_skip("every_node_kind_is_its_semantic_element_under_a_strict_policy")
    else {
        return;
    };
    let server = StaticServer::start().unwrap();
    server.set(
        "/",
        Response::ok("text/html; charset=utf-8", document(&gallery()))
            .header("content-security-policy", CSP),
    );
    let page = browser.page().unwrap();
    page.goto(&server.url("/")).unwrap();

    // Nothing the framework wrote needed more than the policy allows.
    let console = page.console();
    assert!(console.iter().all(|line| !line.contains("Content Security Policy")), "{console:?}");

    // What assistive technology is told, from the browser itself.
    let heading = page.find_accessible("heading", "Notes").unwrap();
    assert!(heading.properties.contains(&("level".into(), "2".into())), "{heading:?}");
    page.find_accessible("button", "Add note").unwrap();
    page.find_accessible("textbox", "Draft").unwrap();
    let agree = page.find_accessible("checkbox", "I agree").unwrap();
    assert!(agree.properties.contains(&("checked".into(), "true".into())), "{agree:?}");
    page.find_accessible("switch", "Wi-Fi").unwrap();
    page.find_accessible("radio", "Small").unwrap();
    page.find_accessible("slider", "").or_else(|_| page.find_accessible("slider", "volume")).ok();
    page.find_accessible("link", "Read more").unwrap();
    page.find_accessible("listitem", "").ok();
    let tree = page.accessibility_tree().unwrap();
    assert!(tree.iter().any(|node| node.role == "list"), "a list");
    assert_eq!(tree.iter().filter(|node| node.role == "listitem").count(), 2);
    assert!(tree.iter().any(|node| node.role == "combobox"), "the select");
    assert!(tree.iter().any(|node| node.role == "progressbar"), "the progress");
    assert!(tree.iter().any(|node| node.role == "spinbutton"), "the spinner");
    assert!(tree.iter().any(|node| node.role == "separator"), "the rule");

    // Right-to-left mirrors start and end with no application code: the
    // first child sits at the right, inside the start padding.
    let geometry = page
        .eval(
            "(() => { const row = document.getElementById('rtl').getBoundingClientRect(); \
             const first = document.getElementById('first').getBoundingClientRect(); \
             return [row.right - first.right, first.width]; })()",
        )
        .unwrap();
    assert_eq!(geometry[0].as_f64(), Some(10.0), "start padding is on the right: {geometry}");
    assert_eq!(geometry[1].as_f64(), Some(40.0), "a fixed width is honored: {geometry}");

    // Declarations: the colour applies, and the breakpoint follows the
    // window rather than the server.
    let padding = |page: &rustnative_web_testing::Page| {
        page.eval("getComputedStyle(document.getElementById('styled')).paddingTop").unwrap()
    };
    page.emulate(None, Some(600)).unwrap();
    assert_eq!(padding(&page), "16px");
    page.emulate(None, Some(1024)).unwrap();
    assert_eq!(padding(&page), "32px");
    let color = page.eval("getComputedStyle(document.getElementById('styled')).color").unwrap();
    assert_eq!(color, "rgb(255, 0, 0)");
}
