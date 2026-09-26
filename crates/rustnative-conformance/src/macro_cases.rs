//! The `rsx!` spelling of every syntax-equivalence case — markup inside a
//! `.rs` file.

use rustnative_core::{ComponentContext, Cursor, Node, rsx};

use crate::syntax::{self, Card, values};

/// Every case, by name, in the same order as the builder cases.
#[must_use]
pub fn cases() -> Vec<(&'static str, Node)> {
    vec![
        ("label", rsx! { <Label key="greeting" text="Hello" /> }),
        (
            "label_layout",
            rsx! {
                <Label
                    key="sized"
                    text="Sized"
                    width={values::WIDTH}
                    height={values::HEIGHT}
                    margin={values::MARGIN}
                    align_self={values::CENTER}
                    constraints={syntax::constraints()}
                    direction={values::RTL}
                />
            },
        ),
        ("button", rsx! { <Button key="save" text="Save" /> }),
        ("text_input", rsx! { <TextInput key="name" value="Ada" /> }),
        ("canvas", rsx! { <Canvas key="swatch" draw_list={syntax::drawing()} /> }),
        ("surface", rsx! { <Surface key="scene" /> }),
        ("foreign", rsx! { <Foreign key="date" kind="month-calendar" /> }),
        ("tab_bar", rsx! { <TabBar key="tabs" labels={["One", "Two"]} selected=1 /> }),
        ("checkbox", rsx! { <Checkbox key="remember" label="Remember me" checked=true /> }),
        ("radio", rsx! { <Radio key="small" label="Small" selected=false /> }),
        ("toggle", rsx! { <Toggle key="wifi" label="Wi-Fi" on=true /> }),
        ("slider", rsx! { <Slider key="volume" value=40 min=0 max=100 /> }),
        ("progress", rsx! { <Progress key="upload" percent={Some(40)} /> }),
        ("select", rsx! { <Select key="size" options={["S", "M"]} selected={Some(1)} /> }),
        ("list_box", rsx! { <ListBox key="fruit" items={["Apple", "Pear"]} selected={None} /> }),
        (
            "date_picker",
            rsx! { <DatePicker key="due" date={rustnative_core::CalendarDate { year: 2026, month: 9, day: 25 }} /> },
        ),
        ("spinner", rsx! { <Spinner key="copies" value=2 min=1 max=9 /> }),
        ("separator", rsx! { <Separator key="rule" /> }),
        ("link", rsx! { <Link key="help" text="Help" /> }),
        ("multiline_text", rsx! { <MultilineText key="notes" value="Hello" /> }),
        ("image", rsx! { <Image key="logo" image={syntax::picture()} /> }),
        (
            "column",
            rsx! {
                <Column
                    key="stack"
                    padding={values::PADDING}
                    gap=4
                    align_items={values::CENTER}
                    overflow={values::SCROLL}
                >
                    <Label key="a" text="A" />
                    <Label key="b" text="B" />
                </Column>
            },
        ),
        (
            "row",
            rsx! {
                <Row key="line" gap=2>
                    <Button key="ok" text="OK" />
                    <Button key="cancel" text="Cancel" />
                </Row>
            },
        ),
        (
            "virtual_list",
            rsx! {
                <VirtualList key="rows" list={syntax::list()}>
                    for index in 0..3 {
                        <Label key={format!("row-{index}")} text={format!("Row {index}")} item_index={index} />
                    }
                </VirtualList>
            },
        ),
        (
            "accessibility",
            rsx! { <Button key="icon" text="\u{1F4BE}" accessibility={syntax::accessibility()} /> },
        ),
        ("style", rsx! { <Label key="styled" text="Styled" style={syntax::visual()} /> }),
        (
            "class",
            rsx! { <Label key="classy" text="Classy" class="p-2 bg-blue-500 hover:bg-blue-600" /> },
        ),
        (
            "style_declarations",
            rsx! { <Label key="declared" text="Declared" style="padding: 4px; color: #123456" /> },
        ),
        ("input", rsx! { <Column key="pad" input={syntax::interest()}></Column> }),
        ("opacity", rsx! { <Label key="faint" text="Faint" opacity=0.5 /> }),
        ("transition", rsx! { <Column key="panel" transition={syntax::slide()}></Column> }),
        ("item_index", rsx! { <Label key="item" text="Item" item_index=7 /> }),
        ("command", rsx! { <Button key="save" text="Save" command={syntax::SAVE} /> }),
        ("cursor", rsx! { <Label key="link" text="Link" cursor={Cursor::Pointer} /> }),
        ("shared_id", rsx! { <Label key="thumb" text="Photo" shared_id="photo" /> }),
        ("disabled", rsx! { <Button key="off" text="Off" disabled /> }),
        ("hidden", rsx! { <Label key="gone" text="Gone" hidden={true} /> }),
        (
            "grid",
            rsx! {
                <Grid key="form" tracks={values::grid_tracks()}>
                    <Label key="name-label" text="Name" />
                    <TextInput key="name" value="" />
                    <Label key="note" text="Required" grid={rustnative_core::GridPlacement::at(1, 0).span(1, 2)} />
                </Grid>
            },
        ),
        ("spread", rsx! { <Label key="loud" text="Loud" ..{syntax::emphasized} /> }),
        ("control_flow", control_flow(true, 2, &["x", "y"], Some("extra"))),
        ("control_flow_else", control_flow(false, 0, &[], None)),
    ]
}

/// The same children as `builder_cases::control_flow`, as markup writes
/// them: the constructs markup is good at.
#[must_use]
pub fn control_flow(show: bool, count: u8, items: &[&str], extra: Option<&str>) -> Node {
    rsx! {
        <Column key="flow">
            if show {
                <Label key="shown" text="Shown" />
            } else {
                <Label key="hidden-note" text="Nothing to show" />
            }
            match count {
                0 => <Label key="count" text="none" />,
                1 => <Label key="count" text="one" />,
                _ => { <Label key="count" text="many" /> }
            }
            for item in items {
                <Label key={*item} text={*item} />
            }
            <>
                <Label key="first" text="1" />
                <Label key="second" text="2" />
            </>
            {extra.map(|text| rsx! { <Label key="extra" text={text} /> })}
        </Column>
    }
}

/// A component element: `in context,` names the context it composes
/// through.
pub fn components(context: &mut ComponentContext<'_, ()>) -> Node {
    rsx! {
        in context,
        <Column key="cards">
            <Card key="card" title="Welcome" highlighted />
        </Column>
    }
}
