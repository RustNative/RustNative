//! One definition, two realizations, one test: the runtime's JavaScript
//! realizer (`rn.realize`) and the Rust one (`dom::Realizer`) must produce
//! the same elements and the same rules for the same nodes; the runtime's
//! hash, number formatting, and string functions must agree with Rust's.
//! Runs the runtime in Node.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::approx_constant,
    clippy::cast_possible_truncation,
    reason = "formatting cases chosen for their digits; a failed expectation in an integration test is the test failing"
)]

use std::path::PathBuf;
use std::process::Command;

use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, CalendarDate, Color, ColumnStyle, Constraints, Cursor,
    DrawList, EdgeInsets, GridPlacement, GridStyle, ItemExtent, LayoutDirection, LayoutStyle, Node,
    Paint, RectF, RowStyle, SizeMode, Track, Typography, VirtualListStyle, VisualStyle, classes,
};
use rustnative_web::css::StyleSheet;
use rustnative_web::dom::Realizer;
use serde_json::{Value, json};

fn corpus() -> Vec<Node> {
    let item = |text: &str| {
        Node::label(text.to_lowercase(), text)
            .with_accessibility(AccessibilityInfo::new(AccessibilityRole::ListItem))
    };
    vec![
        Node::label("title", "Notes"),
        Node::label("h", "Heading")
            .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Heading { level: 3 })),
        Node::label("deep", "Deep")
            .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Heading { level: 8 })),
        Node::button("go", "Go").disabled(true),
        Node::text_input("name", "Ada")
            .with_accessibility(AccessibilityInfo::new(AccessibilityRole::TextInput).name("Name").focusable(true)),
        Node::tab_bar("tabs", ["One", "Two", "Three"], 1, LayoutStyle::new()),
        Node::checkbox("agree", "I agree", true),
        Node::toggle("wifi", "Wi-Fi", false),
        Node::radio("small", "Small", true),
        Node::slider("volume", 3, 0, 10),
        Node::spinner("count", 2, 0, 9),
        Node::progress("load", Some(40)),
        Node::progress("busy", None),
        Node::select("size", ["S", "M", "L"], Some(1)),
        Node::select("none", ["A", "B"], None),
        Node::list_box("pick", ["x", "y", "z"], Some(0)),
        Node::date_picker("when", CalendarDate::new(2026, 9, 27).unwrap()),
        Node::separator("rule"),
        Node::link("more", "Read more"),
        Node::multiline_text("notes", "one\ntwo"),
        Node::canvas(
            "chart",
            DrawList::new().fill_rect(RectF::new(0.0, 0.0, 10.0, 10.0), Paint::color(Color::rgb(1, 2, 3))),
            LayoutStyle::new().height(SizeMode::Fixed(40)),
        ),
        Node::foreign("map", "map-view", LayoutStyle::new()),
        Node::column("list", ["Milk", "Bread"].map(item))
            .with_accessibility(AccessibilityInfo::new(AccessibilityRole::List)),
        Node::column("mixed", [item("One"), Node::button("b", "B")])
            .with_accessibility(AccessibilityInfo::new(AccessibilityRole::List)),
        Node::column("dialog", [Node::label("m", "Sure?")])
            .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Dialog).name("Confirm")),
        Node::column_with_layout(
            "sized",
            [
                Node::label_with_layout("fill", "Fill", LayoutStyle::new().height(SizeMode::Fill)),
                Node::label_with_layout(
                    "fixed",
                    "Fixed",
                    LayoutStyle::new()
                        .width(SizeMode::Fixed(120))
                        .margin(EdgeInsets::logical(1, 2, 3, 4))
                        .constraints(Constraints::new().with_min_width(10).with_max_height(90)),
                ),
                Node::label_with_layout(
                    "centered",
                    "Centered",
                    LayoutStyle::new().width(SizeMode::Auto).align_self(rustnative_core::Alignment::Center),
                ),
            ],
            LayoutStyle::new().width(SizeMode::Fixed(300)).direction(LayoutDirection::Rtl),
            ColumnStyle::new().gap(4).padding(EdgeInsets::all(8)).overflow(rustnative_core::Overflow::Scroll),
        ),
        Node::row_with_layout(
            "row",
            [Node::button("a", "A"), Node::button("c", "C")],
            LayoutStyle::new(),
            RowStyle::new().gap(0).align_items(rustnative_core::Alignment::End),
        ),
        Node::grid(
            "grid",
            GridStyle::new([Track::Fixed(80), Track::Fraction(2), Track::Auto]).gap(6).rows([Track::Auto]),
            LayoutStyle::new(),
            [
                Node::label_with_layout("g1", "1", LayoutStyle::new().grid(GridPlacement::at(1, 2))),
                Node::label_with_layout("g2", "2", LayoutStyle::new().width(SizeMode::Fixed(20))),
            ],
        ),
        Node::virtual_list_with_layout(
            "vlist",
            VirtualListStyle::new(100, ItemExtent::Fixed(20)),
            LayoutStyle::new().height(SizeMode::Fixed(200)),
            (10..14).map(|index| Node::label(format!("item-{index}"), format!("Item {index}")).with_item_index(index)),
        ),
        Node::label("styled", "Styled")
            .with_style(
                VisualStyle::new()
                    .foreground(Color::rgb(255, 0, 0))
                    .background(Color::rgba(0, 0, 0, 128))
                    .border(Color::rgb(0, 0, 255))
                    .border_radius(6)
                    .padding(EdgeInsets::symmetric(2, 4))
                    .typography(Typography { family: "Inter".into(), size: 18, weight: 600 }),
            )
            .with_state_style(rustnative_core::ControlState::Hovered, VisualStyle::new().background(Color::rgb(9, 9, 9)))
            .with_opacity(0.5)
            .with_cursor(Cursor::Pointer),
        Node::label("classes", "Classes").with_class(classes!(
            "p-4 md:p-8 hover:bg-[#ff0000] dark:text-white w-32 md:w-full hidden lg:flex self-center"
        )),
        Node::row(
            "responsive",
            [Node::label("r1", "r").with_class(classes!("w-16 md:w-auto lg:h-10 xl:self-end"))],
        ),
        Node::column(
            "related",
            [
                Node::label("caption", "Volume"),
                Node::slider("v", 3, 0, 10).with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Slider)
                        .labelled_by("caption")
                        .described_by("help")
                        .focusable(true),
                ),
                Node::label("help", "Louder or quieter"),
                Node::column("group", []).with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Group)
                        .name("Settings")
                        .description("All of them")
                        .expanded(true)
                        .required(true)
                        .busy(true)
                        .live(rustnative_core::accessibility::LiveRegion::Polite)
                        .position_in_set(2, 5)
                        .range(0.0, 1.0, 0.25, 0.1)
                        .focusable(true),
                ),
            ],
        ),
        Node::label("hidden", "gone").hidden(true),
    ]
}

fn node_program() -> Option<PathBuf> {
    rustnative_web_testing::find_node()
}

fn run_node(script: &str, input: &Value) -> Value {
    let directory = std::env::temp_dir().join(format!("rn-runtime-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("rn.mjs"), rustnative_web::runtime::RUNTIME_JS).unwrap();
    let script_path =
        directory.join(format!("script-{}.mjs", rustnative_web::hash::cyrb53(script)));
    std::fs::write(&script_path, script).unwrap();
    let input_path =
        directory.join(format!("input-{}.json", rustnative_web::hash::cyrb53(&input.to_string())));
    std::fs::write(&input_path, input.to_string()).unwrap();
    let output =
        Command::new(node_program().unwrap()).arg(&script_path).arg(&input_path).output().unwrap();
    assert!(output.status.success(), "node failed:\n{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!("node's output was not JSON ({error}):\n{}", String::from_utf8_lossy(&output.stdout))
    })
}

#[test]
fn the_javascript_realizer_writes_what_the_rust_one_writes() {
    if node_program().is_none() {
        eprintln!("skipped: no node (set RUSTNATIVE_NODE)");
        return;
    }
    let cases: Vec<Value> = corpus()
        .iter()
        .map(|node| {
            let mut sheet = StyleSheet::new();
            let element = Realizer::new(&mut sheet, "i0-", node).root(node);
            json!({
                "node": rustnative_web::jsnode::from_node(node),
                "element": serde_json::to_value(&element).unwrap(),
                "rules": sheet.entries(),
            })
        })
        .collect();
    let script = r#"
import * as rn from "./rn.mjs";
import { readFileSync } from "node:fs";
const cases = JSON.parse(readFileSync(process.argv[2], "utf8"));
const out = cases.map((c) => {
  const { element, sheet } = rn.realize(c.node, "i0-");
  return { element, rules: sheet.entries() };
});
process.stdout.write(JSON.stringify(out));
"#;
    let results = run_node(script, &Value::Array(cases.clone()));
    let mut failures = Vec::new();
    for (index, (case, result)) in cases.iter().zip(results.as_array().unwrap()).enumerate() {
        if case["element"] != result["element"] {
            failures.push(format!(
                "case {index} element:\n  rust: {}\n  js:   {}",
                case["element"], result["element"]
            ));
        }
        let mut rust: Vec<Value> = case["rules"].as_array().unwrap().clone();
        let mut js: Vec<Value> = result["rules"].as_array().unwrap().clone();
        rust.sort_by_key(ToString::to_string);
        js.sort_by_key(ToString::to_string);
        if rust != js {
            failures.push(format!("case {index} rules:\n  rust: {rust:?}\n  js:   {js:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

#[test]
fn hashing_and_number_formatting_agree_with_rust() {
    if node_program().is_none() {
        eprintln!("skipped: no node (set RUSTNATIVE_NODE)");
        return;
    }
    // A deterministic spread of doubles: halves and ties, tiny and huge
    // magnitudes, negatives, and the classic 0.1 + 0.2.
    let mut values: Vec<f64> = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        1.5,
        2.5,
        -2.5,
        0.125,
        0.375,
        1e21,
        1e22,
        1.5e300,
        1e-7,
        5e-324,
        0.1 + 0.2,
        123_456.789,
        -0.001,
        9_007_199_254_740_993.0,
        3.141_592_653_589_793,
        2.675,
        1.005,
    ];
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    for _ in 0..400 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let exponent = i32::try_from(seed % 60).unwrap() - 30;
        let mantissa = f64::from(u32::try_from(seed >> 40).unwrap()) / 16_777_216.0;
        let sign = if seed & 1 == 0 { 1.0 } else { -1.0 };
        values.push(sign * mantissa * 10_f64.powi(exponent));
    }
    let expected: Vec<Value> = values
        .iter()
        .map(|value| {
            json!({
                "value": value,
                "negative_zero": value.to_bits() == (-0.0_f64).to_bits(),
                "display": format!("{value}"),
                "p0": format!("{value:.0}"),
                "p2": format!("{value:.2}"),
                "p5": format!("{value:.5}"),
                "f32": format!("{}", *value as f32),
            })
        })
        .collect();
    let strings = ["", "a", "héllo 🚀", "\u{feff} x \u{3000}", "Σίσυφος", "ﬁ"];
    let text: Vec<Value> = strings
        .iter()
        .map(|text| {
            json!({
                "text": text,
                "hash": rustnative_web::hash::cyrb53(text),
                "len": text.len(),
                "chars": text.chars().count(),
                "trim": text.trim(),
                "upper": text.to_uppercase(),
                "lower": text.to_lowercase(),
                "debug": format!("{text:?}"),
            })
        })
        .collect();
    let script = r#"
import * as rn from "./rn.mjs";
import { readFileSync } from "node:fs";
const input = JSON.parse(readFileSync(process.argv[2], "utf8"));
const floats = input.floats.map((c) => {
  const v = c.negative_zero ? -0 : c.value;
  return { display: rn.float(v, "f64"), p0: rn.fixed(v, 0), p2: rn.fixed(v, 2), p5: rn.fixed(v, 5), f32: rn.float(Math.fround(v), "f32") };
});
const text = input.text.map((c) => ({ hash: rn.cyrb53(c.text), len: rn.utf8len(c.text), chars: rn.charCount(c.text),
  trim: rn.trim(c.text), upper: c.text.toUpperCase(), lower: c.text.toLowerCase(), debug: rn.debugString(c.text, '"') }));
process.stdout.write(JSON.stringify({ floats, text }));
"#;
    let results = run_node(script, &json!({ "floats": expected, "text": text }));
    let mut failures = Vec::new();
    for (case, result) in expected.iter().zip(results["floats"].as_array().unwrap()) {
        for field in ["display", "p0", "p2", "p5", "f32"] {
            if case[field] != result[field] {
                failures.push(format!(
                    "{} {field}: rust {} js {}",
                    case["value"], case[field], result[field]
                ));
            }
        }
    }
    for (case, result) in text.iter().zip(results["text"].as_array().unwrap()) {
        for field in ["hash", "len", "chars", "trim", "upper", "lower", "debug"] {
            if case[field] != result[field] {
                failures.push(format!(
                    "{:?} {field}: rust {} js {}",
                    case["text"], case[field], result[field]
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{} differences:\n{}", failures.len(), failures.join("\n"));
}
