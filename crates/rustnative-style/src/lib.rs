//! The style spellings of Rust Native (`PLAN.md` 2.14 and Milestone 58).
//!
//! The resolved style is the model, and it has two spellings: the typed
//! properties a builder chain sets (`VisualStyle`, `LayoutStyle`, …), and
//! the declaration vocabulary with the utility classes above it. This crate
//! owns the second: its vocabulary, its parse, its lowering, and its
//! diagnostics. The `classes!`/`styles!` macros, `rustnative_build`'s
//! `compile_styles()`, and the CLI link it, so all three accept exactly the
//! same language; `rustnative-core` re-exports the model and resolves it.
//!
//! ```text
//! utility classes      p-4  bg-blue-500  hover:bg-blue-500/90  md:w-64  dark:…
//!         │            compiled at build time; unknown class = error
//!         ▼
//! declarations         padding: 1rem;  background-color: var(--color-blue-500);
//!         │            values, units, calc(), colour functions, token references
//!         ▼
//! typed properties     the same values a builder chain sets directly
//! ```
//!
//! The utility vocabulary is **Tailwind CSS v4.1.13**'s ([`TAILWIND_VERSION`]),
//! over its vendored default theme (`VENDORED.md`). What it covers, and the
//! typed property each utility sets:
//!
//! | Utilities | Property |
//! |---|---|
//! | `p-*` `px-*` `py-*` `pt-*` `pb-*` `ps-*` `pe-*` (`pl-*`/`pr-*` as start/end) | padding |
//! | `m-*` `mx-*` `my-*` `mt-*` `mb-*` `ms-*` `me-*`, negatives | `LayoutStyle::margin` |
//! | `w-*` `h-*` `size-*` (spacing scale, `auto`, `full`, `[…]`) | `LayoutStyle::width`/`height` |
//! | `min-w-*` `min-h-*` `max-w-*` `max-h-*` | `Constraints` |
//! | `gap-*` | a container's gap |
//! | `items-*` / `self-*` | alignment |
//! | `overflow-*` | a container's overflow |
//! | `bg-*` `text-*` `border-*` (colour, `/NN` opacity, `[…]`) | background, foreground, border colour |
//! | `text-xs` … `text-9xl` | font size |
//! | `font-thin` … `font-black`, `font-sans`/`serif`/`mono` | font weight, family |
//! | `rounded`, `rounded-*` | corner radius |
//! | `shadow`, `shadow-*` | shadow |
//! | `opacity-*` | opacity |
//! | `hidden`, `block`, `flex` | visibility |
//!
//! Variants: `hover:` `focus:` `focus-visible:` `active:` `disabled:` (state
//! styles, visual properties only), `dark:`, `sm:` `md:` `lg:` `xl:` `2xl:`
//! and `min-[…]:`, `rtl:` `ltr:`, `motion-reduce:` `motion-safe:`,
//! `pointer-coarse:` `pointer-fine:`, and a project's `@custom-variant`s.
//! Refused, each with a diagnostic naming the reason: relational variants
//! (`group-*`, `peer-*` — selector matching), container queries (deferred),
//! a container's axis (`flex-row` — it is the element), border widths and
//! per-side or per-corner values (no typed property), and the cascade's
//! keywords.
#![deny(missing_docs)]

pub mod capability;
pub mod color;
pub mod design_tokens;
pub mod model;
pub mod sheet;
mod token_table;
#[cfg(feature = "tokens")]
pub mod tokens;
pub mod value;
pub mod vocabulary;
pub mod web;

pub use capability::{
    ANDROID, ANDROID_UNITS, HEADLESS, HEADLESS_UNITS, LINUX, LINUX_UNITS, StyleCapabilities,
    StyleSupport, UnitMapping, WEB, WEB_UNITS, WINDOWS, WINDOWS_UNITS,
};
pub use model::{
    Color, Condition, ConditionEnv, ConditionalDeclaration, Declaration, DeclarationSet, Direction,
    Fixed, Keyword, Length, Pointer, Scheme, SetKind, ShadowLayer, State, StyleProperty,
    StyleValue, ValueKind,
};
pub use sheet::StyleError;
pub use token_table::TokenTable;
pub use vocabulary::{DEFAULT_THEME, TAILWIND_VERSION, Vocabulary};
