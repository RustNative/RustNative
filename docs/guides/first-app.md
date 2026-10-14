# Your first application

A task-oriented walk from an empty folder to a packaged application on
Windows, Linux, or Android (`PLAN.md` Milestone 52). Every tree on this page
is shown in both syntaxes, side by side, and every style in both spellings.
Neither is the "real" one: they produce the same tree.

## Create it

```sh
rustnative new notes --syntax builder
rustnative new notes --syntax markup    # src/app.rsx; there is no default
cd notes
rustnative dev windows          # run it; saving a file restarts it with its state kept
                                # (`linux`, or `android` with a phone attached)
```

The project has:

- the application in `src/lib.rs` (or `src/app.rsx`) with its `run`
  function, and a thin `src/main.rs` that calls it — Android calls the same
  `run` from its launcher activity;
- no platform in its code: `rustnative-host` picks the backend for the
  target being built and supplies its services, so the same source builds
  for every host;
- its style file, `app.css`;
- `rustnative.toml`, which sets its identity;
- a golden test for each preview.

## Say what the screen shows

A component holds state and turns it into a tree of nodes. Here is the
same tree in each syntax:

```rust
fn view(&self) -> Node {
    Node::column(
        "root",
        [
            Node::label("count", format!("Clicked {} times", self.clicks)),
            Node::button("click", "Click me"),
        ],
    )
}
```

```rust
fn view(&self) -> Node {
    <Column key="root">
        <Label key="count" text={format!("Clicked {} times", self.clicks)} />
        <Button key="click" text="Click me" />
    </Column>
}
```

Keys (`"root"`, `"count"`, `"click"`) name nodes. Events and tests find a
node by its key.

## Make it do something

Events arrive in `update`. Change the state there. The framework renders
the component again, and only that component (`docs/policy/change-detection.md`).

```rust
fn update(&mut self, event: Event) {
    if matches!(event, Event::Click { target } if target == NodeId::from_key("click")) {
        self.clicks += 1;
    }
}
```

## Style it

The typed spelling and the utility classes produce the same style:

```rust
Node::label("count", "Clicked 0 times")
    .with_style(VisualStyle::new().background(Color::rgb(0x2b, 0x7f, 0xff)).border_radius(8))
```

```rust
Node::label("count", "Clicked 0 times").with_class(classes!("bg-blue-500 rounded-lg"))
```

In markup, the same two spellings:

```rust
<Label key="count" text="Clicked 0 times"
    style={VisualStyle::new().background(Color::rgb(0x2b, 0x7f, 0xff)).border_radius(8)} />
<Label key="count" text="Clicked 0 times" class="bg-blue-500 rounded-lg" />
```

Theme tokens live in `app.css`. `rustnative dev` applies a change to a
token without restarting.

## Test it

The headless backend runs the application with no window, so a test can
click and read back what is shown:

```rust
let mut app = HeadlessApp::launch(Window::new("notes", Size::new(480, 320)), || App::new(()));
app.click(&Query::key("click")).unwrap();
assert_eq!(app.find(&Query::key("count")).unwrap().text.as_deref(), Some("Clicked 1 times"));
```

`rustnative test` runs these tests and the preview goldens.

## Ship it

```sh
rustnative package windows              # a ZIP and an MSIX in target/package/
rustnative package linux                # .deb, .rpm, pacman, tarball, AppImage
rustnative package android              # an APK and an App Bundle, signed
rustnative compliance                   # SBOM, licenses, and privacy manifest, from the build
```

## Ask for capabilities, not platforms

When a feature depends on the host — a share sheet, a tray icon, a camera —
ask the context, and give the host without it something that works:

```rust
if context.capabilities().supports(Capability::SystemShare) {
    Node::button("share", "Share")
} else {
    Node::button("share", "Copy link")
}
```

```rust
if context.capabilities().supports(Capability::SystemShare) {
    <Button key="share" text="Share" />
} else {
    <Button key="share" text="Copy link" />
}
```

The service behind it is on `context.services()` (`share()`, here) where the
host has one, and absent where it does not.

- **Updates:** `docs/deploy.md`.
- **Adding a capability** someone else wrote:

  ```sh
  rustnative search battery
  rustnative add ../package-battery     # a path, or a name from the index
  ```

## Where next

Each topic has a guide and an example that runs; see
`docs/guides/README.md`.
