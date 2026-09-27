# The client subset (Web milestone A)

Client logic is ordinary Rust, in a module marked `#[rustnative_web::client]`.
When the crate is built, the macro translates the module to a JavaScript
module and leaves the Rust in place, so the same component runs natively and
in a browser. The translation happens once per build, never per request: the
output is a static, cacheable asset, and a strict content security policy
needs no nonces for it.

```rust
#[rustnative_web::client]
pub mod counter {
    use rustnative_core::{Event, Node, NodeId, rsx};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct State { pub count: i32 }

    impl State {
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            if let Event::Click { target } = event {
                if target == NodeId::from_key("add") { self.count += 1; }
            }
        }

        pub fn view(&self) -> Node {
            rsx! { <Button key="add" text={format!("{} clicks", self.count)} /> }
        }
    }
}
```

The module expands to:

- its own Rust, with integer arithmetic replaced by
  `rustnative_web::client::rt`'s checked arithmetic and `rsx!` by its builder
  expansion, so an overflow panics at the same step in both languages;
- `impl ClientLogic for State`, carrying the JavaScript, the CSS its class
  strings need, the server functions it calls, and a line table from each
  JavaScript line back to the Rust (the source map);
- `pub type Island = rustnative_web::Client<State>`, a component whose props
  are the initial state.

Natively, `Island` is a component like any other: effects are carried out
through `Services`. Under a server render it registers an island, and the
browser runtime (`rn.js`) runs the generated module against the same state,
serialized with serde's defaults.

## The module

| Item | Required | Signature |
|---|---|---|
| the state type | yes | a struct or enum deriving `Clone`, `PartialEq`, `Serialize`, `Deserialize` |
| `update` | yes | `fn update(&mut self, event: Event, fx: &mut Effects<M>)` |
| `view` | yes | `fn view(&self) -> Node` |
| `message` | no | `fn message(&mut self, message: M, fx: &mut Effects<M>)`, for effect replies |
| `init` | no | `fn init(&mut self, fx: &mut Effects<M>)`, once when the component mounts |

Other structs, enums, free functions and inherent methods in the module are
translated too, so helpers can be factored out as usual. `#[serde(rename)]`
and the other renaming attributes are refused: the generated code reads Rust
field names.

## What is in the subset

**Values.** Every integer type, `f32`, `f64`, `bool`, `char`, `String`,
`&str`, `Vec<T>`, `Option<T>`, `Result<T, E>`, tuples, the module's own
structs and enums, and the framework types a view uses (`Node`, `NodeId`,
`Event` and its payloads, `LayoutStyle`, `SizeMode`, `EdgeInsets`,
`Typography`, `AccessibilityInfo` and its roles, `KeyCode`, `CalendarDate`,
and the other types in `rustnative_core`'s view vocabulary).

**Statements and expressions.** `let` (with patterns and `else`), `if`,
`if let` and let chains, `match` with guards and every pattern shape the
types above allow, `while`, `while let`, `loop` with `break` values,
`for` over ranges and iterators, labelled `break` and `continue`, `return`,
closures, method calls, struct literals and update syntax, indexing,
compound assignment, and `as` casts between numbers.

**Arithmetic.** Integer arithmetic is checked in both languages. Values of
the 64-bit types (`i64`, `u64`, `isize`, `usize`) must stay within
±(2⁵³ − 1), the range a JavaScript number holds exactly; beyond it is an
overflow, in Rust as well, so the two never disagree. Division truncates
toward zero; `%` keeps the dividend's sign; `as` wraps and saturates as Rust
does.

**Formatting.** `format!`, `println!` and the other print macros, with
width, fill, alignment, sign, `#`, zero padding, precision (rounded half to
even, exactly as Rust does), `{:x}`, `{:X}`, `{:b}`, `{:o}`, and `{:?}` of
strings, chars, integers, `bool`, `Option`, `Vec` and derived-`Debug` types.
Floats display as Rust displays them (the shortest representation that
round-trips).

**Strings.** Lengths are UTF-8 byte counts; comparison is by code point;
`trim` uses Rust's `White_Space`. The methods: `len`, `is_empty`, `trim`,
`trim_start`, `trim_end`, `to_uppercase`, `to_lowercase`, their ASCII forms,
`contains`, `starts_with`, `ends_with`, `eq_ignore_ascii_case`, `replace`,
`repeat`, `split`, `split_whitespace`, `lines`, `chars`, `parse`,
`push_str`, `push`, `pop`, `clear`, `to_string`, and the `char`
classification methods (`is_alphabetic`, `is_ascii_digit`, `to_digit`, …).

**Numbers.** `abs`, `pow`, `min`, `max`, `clamp`, `signum`, the `checked_*`
and `saturating_*` families, `rem_euclid`, `abs_diff`, `is_power_of_two`;
for floats, the rounding functions, `sqrt`, the trigonometric and
exponential functions, `powi`, `powf`, `is_nan`, `is_finite`, and the usual
constants (`MAX`, `MIN`, `PI`, `E`, `EPSILON`, …).

**Collections.** `Vec`'s `push`, `pop`, `insert`, `remove`, `swap_remove`,
`truncate`, `clear`, `get`, `get_mut`, `first`, `last`, `sort` and its keyed
and comparator forms (stable, as Rust's), `reverse`, `retain`, `dedup`,
`extend`, `swap`; and iterators with `map`, `filter`, `filter_map`,
`flat_map`, `flatten`, `enumerate`, `rev`, `skip`, `take`, `step_by`,
`chain`, `zip`, `count`, `sum`, `product`, `min`, `max`, `min_by_key`,
`max_by_key`, `any`, `all`, `find`, `position`, `nth`, `fold`, `for_each`,
`cloned`, `copied`, `peekable`, `collect` and `join`. An out-of-bounds index
panics with Rust's message.

**`Option` and `Result`.** `is_some`, `is_none`, `unwrap`, `expect`, the
`unwrap_or` family, `map`, `and_then`, `filter`, `map_or`, `ok_or`, `or`,
`xor`, `zip`, `as_ref`, `ok`, `err`, `map_err`, `is_ok_and`, and the rest of
the combinators a client needs.

**Views.** `rsx!` markup, the `Node` builders, the node modifiers
(`with_class`, `with_style`, `with_state_style`, `with_accessibility`,
`hidden`, `disabled`, …), and `classes!` and `styles!`, whose rules are
compiled at build time into the page's style sheet.

**Panics.** `panic!`, `unreachable!`, `todo!`, `assert!` and its forms. A
panic in the browser stops the island with the same message the native
component would have given.

## Effects

`fx` is how client logic reaches outside itself. Each effect is a value in a
queue, so the native and generated code request the same effects in the same
order and a test can compare them.

| Effect | What it does |
|---|---|
| `fx.call::<F>(input, reply)` | calls server function `F`; `reply` turns its answer into a message |
| `fx.after(duration, message)` | sends `message` after a delay |
| `fx.navigate(path)`, `fx.back()` | client-side navigation |
| `fx.focus(key)` | moves focus to a node |
| `fx.copy(text)` | writes the clipboard |
| `fx.store(key, value)`, `fx.load(key, reply)` | local storage |
| `fx.notify(text)` | an announcement to assistive technology |
| `fx.publish(topic, value)`, `fx.subscribe(topic, reply)` | values shared between the islands of a page |
| `fx.download(name, bytes)` | offers a file to save |
| `fx.js(module, function, args, reply)` | calls hand-written JavaScript |
| `fx.http_get`, `fx.share`, `fx.locate`, `fx.open_file`, `fx.db_put`, `fx.socket_open`, `fx.worker`, `fx.capture_pointer`, … | the platform capabilities (`docs/web/capabilities.md`) |

Natively, a native host performs each through `Services`; `fx.js` answers
with an error, since there is no JavaScript.

## What is outside it, and why

Code outside the subset is a compile error at the construct, not a runtime
surprise. Every refusal names what was refused, why, and the three ways
forward.

| Refused | Why |
|---|---|
| the file system, threads, processes, the network outside `fx` | a browser has none of them |
| `unsafe`, raw pointers | nothing in JavaScript to translate them to |
| `?` | match on the `Option` or `Result`, so the early return reads the same in both languages |
| shifts and bitwise operators | JavaScript's are 32-bit, and would disagree with Rust's on every wider type |
| slicing, and slice patterns | byte offsets into a UTF-16 string would disagree; index one element at a time |
| literals and constants beyond ±(2⁵³ − 1) | a JavaScript number cannot hold them exactly |
| `{:?}` of floats | Rust's `Debug` float form differs from `Display` in ways not worth mirroring |
| items inside functions | move them to the module |
| generic types and functions | client data has one JSON shape, and each function one JavaScript body |
| `async fn`, `async` blocks | asynchronous work is an effect; its answer is a message |
| methods the tables above do not list | each translated method is written and tested against Rust; there is no general fallback |

The ways forward:

1. **`#[server]`.** Mark the function `#[server]` to run it on the server, and
   call it from the client with `fx.call::<F>(..)`. Full Rust, any crate.
2. **A WebAssembly subtree** (`rustnative_web::wasm_subtree!`). The logic runs
   as full Rust compiled to WebAssembly, in the browser, for work that must be
   local and fast.
3. **`fx.js(..)`.** Hand-written JavaScript for a browser API the framework
   does not cover.

## How it is checked

`crates/rustnative-web/tests/client_subset.rs` runs each client module in
both languages, the Rust natively and the JavaScript in Node with the
browser runtime, through the same events. After every step the two must
hold the same state, realize the same elements and rules, request the same
effects, and panic at the same step with the same message.
`crates/rustnative-webgen/tests/refusals.rs` holds each refusal to its
construct and its message.
