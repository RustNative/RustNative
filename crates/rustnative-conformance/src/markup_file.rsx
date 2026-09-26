// The `.rsx` spelling of every syntax-equivalence case: markup as a plain
// expression, with no wrapper, lowered by `rustnative_build::compile_rsx`.

use rustnative_core::{ComponentContext, Cursor, Node};

use crate::syntax::{self, Card, values};

/// Every case, by name, in the same order as the builder cases.
#[must_use]
pub fn cases() -> Vec<(&'static str, Node)> {
    vec![
        ("label", <Label key="greeting" text="Hello" />),
        (
            "label_layout",
            <Label
                key="sized"
                text="Sized"
                width={values::WIDTH}
                height={values::HEIGHT}
                margin={values::MARGIN}
                align_self={values::CENTER}
                constraints={syntax::constraints()}
                direction={values::RTL}
            />,
        ),
        ("button", <Button key="save" text="Save" />),
        ("text_input", <TextInput key="name" value="Ada" />),
        ("canvas", <Canvas key="swatch" draw_list={syntax::drawing()} />),
        ("surface", <Surface key="scene" />),
        ("foreign", <Foreign key="date" kind="month-calendar" />),
        ("tab_bar", <TabBar key="tabs" labels={["One", "Two"]} selected=1 />),
        ("checkbox", <Checkbox key="remember" label="Remember me" checked=true />),
        ("radio", <Radio key="small" label="Small" selected=false />),
        ("toggle", <Toggle key="wifi" label="Wi-Fi" on=true />),
        ("slider", <Slider key="volume" value=40 min=0 max=100 />),
        ("progress", <Progress key="upload" percent={Some(40)} />),
        ("select", <Select key="size" options={["S", "M"]} selected={Some(1)} />),
        ("list_box", <ListBox key="fruit" items={["Apple", "Pear"]} selected={None} />),
        ("date_picker", <DatePicker key="due" date={rustnative_core::CalendarDate { year: 2026, month: 9, day: 25 }} />),
        ("spinner", <Spinner key="copies" value=2 min=1 max=9 />),
        ("separator", <Separator key="rule" />),
        ("link", <Link key="help" text="Help" />),
        ("multiline_text", <MultilineText key="notes" value="Hello" />),
        ("image", <Image key="logo" image={syntax::picture()} />),
        (
            "column",
            <Column
                key="stack"
                padding={values::PADDING}
                gap=4
                align_items={values::CENTER}
                overflow={values::SCROLL}
            >
                <Label key="a" text="A" />
                <Label key="b" text="B" />
            </Column>,
        ),
        (
            "row",
            <Row key="line" gap=2>
                <Button key="ok" text="OK" />
                <Button key="cancel" text="Cancel" />
            </Row>,
        ),
        (
            "virtual_list",
            <VirtualList key="rows" list={syntax::list()}>
                for index in 0..3 {
                    <Label key={format!("row-{index}")} text={format!("Row {index}")} item_index={index} />
                }
            </VirtualList>,
        ),
        ("accessibility", <Button key="icon" text="\u{1F4BE}" accessibility={syntax::accessibility()} />),
        ("style", <Label key="styled" text="Styled" style={syntax::visual()} />),
        ("class", <Label key="classy" text="Classy" class="p-2 bg-blue-500 hover:bg-blue-600" />),
        ("style_declarations", <Label key="declared" text="Declared" style="padding: 4px; color: #123456" />),
        ("input", <Column key="pad" input={syntax::interest()}></Column>),
        ("opacity", <Label key="faint" text="Faint" opacity=0.5 />),
        ("transition", <Column key="panel" transition={syntax::slide()}></Column>),
        ("item_index", <Label key="item" text="Item" item_index=7 />),
        ("command", <Button key="save" text="Save" command={syntax::SAVE} />),
        ("cursor", <Label key="link" text="Link" cursor={Cursor::Pointer} />),
        ("shared_id", <Label key="thumb" text="Photo" shared_id="photo" />),
        ("disabled", <Button key="off" text="Off" disabled />),
        ("hidden", <Label key="gone" text="Gone" hidden={true} />),
        (
            "grid",
            <Grid key="form" tracks={values::grid_tracks()}>
                <Label key="name-label" text="Name" />
                <TextInput key="name" value="" />
                <Label key="note" text="Required" grid={rustnative_core::GridPlacement::at(1, 0).span(1, 2)} />
            </Grid>
        ),
        ("spread", <Label key="loud" text="Loud" ..{syntax::emphasized} />),
        ("control_flow", control_flow(true, 2, &["x", "y"], Some("extra"))),
        ("control_flow_else", control_flow(false, 0, &[], None)),
    ]
}

/// The same children as the other two spellings.
#[must_use]
pub fn control_flow(show: bool, count: u8, items: &[&str], extra: Option<&str>) -> Node {
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
        {extra.map(|text| <Label key="extra" text={text} />)}
    </Column>
}

/// A component element: the context is the enclosing function's
/// `ComponentContext` parameter, found rather than written.
pub fn components(context: &mut ComponentContext<'_, ()>) -> Node {
    <Column key="cards">
        <Card key="card" title="Welcome" highlighted />
    </Column>
}
