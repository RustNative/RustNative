//! The typed environment: values that flow down the tree (`C15`).
//!
//! A component that needs the locale, the layout direction, the person's
//! text size, the colour scheme, or whether motion is reduced does not ask
//! the host — it reads the environment, which every backend feeds from the
//! host's own settings. A subtree can override a value for everything
//! beneath it ([`crate::ComponentContext::provide_env`]), which is how a
//! preview renders one screen in Arabic inside an English application, or
//! a dialog forces the dark scheme.
//!
//! Reads are tracked. When a value changes, exactly the components that
//! read it re-render (the invalidation contract, `docs/invalidation.md`):
//! a locale switch does not re-render a component that never looked at the
//! locale.
//!
//! Upward *preferences* go the other way: a descendant publishes a value
//! ([`crate::ComponentContext::prefer`]) that its ancestors read, reduced
//! with [`Preference::reduce`], on their next render — how a screen deep in
//! a navigation stack tells the window what title it wants.
//!
//! # Example
//!
//! ```
//! use rustnative_core::{ColorScheme, Component, ComponentContext, ComponentTree, Event, Node, keys};
//!
//! struct Badge;
//! impl Component for Badge {
//!     type Props = ();
//!     type Message = ();
//!     fn new((): ()) -> Self { Self }
//!     fn props(&self) -> &() { &() }
//!     fn set_props(&mut self, (): ()) {}
//!     fn view(&self) -> Node { Node::label("badge", "") }
//!     fn update(&mut self, _: Event) {}
//!     fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
//!         let scheme = context.env(&keys::COLOR_SCHEME);
//!         Node::label("badge", format!("{scheme:?}"))
//!     }
//! }
//!
//! let mut tree = ComponentTree::new(Badge);
//! tree.set_environment(&keys::COLOR_SCHEME, ColorScheme::Dark);
//! let Node::Label(label) = tree.view() else { unreachable!() };
//! assert_eq!(label.text(), "Dark");
//!
//! // The rendered label, in markup:
//! assert_eq!(rustnative_core::rsx! { <Label key="badge" text="Dark" /> }, tree.view());
//! ```

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::input::Scalar;
use crate::layout::{EdgeInsets, LayoutDirection, Rect};

/// A typed key into the environment.
///
/// `name` identifies the key and must be unique across a program — the
/// framework's own keys are namespaced `rustnative.*`; an application's
/// should be namespaced by its crate. `default` is the value when nothing
/// has set or provided one.
pub struct EnvKey<T: 'static> {
    name: &'static str,
    default: fn() -> T,
}

impl<T> EnvKey<T> {
    /// A key named `name` whose value is `default()` until set.
    #[must_use]
    pub const fn new(name: &'static str, default: fn() -> T) -> Self {
        Self { name, default }
    }

    /// The key's name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The key's default value.
    #[must_use]
    pub fn default_value(&self) -> T {
        (self.default)()
    }
}

impl<T> fmt::Debug for EnvKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("EnvKey").field(&self.name).finish()
    }
}

/// What an environment value must be: cheap to clone, comparable (so an
/// unchanged value invalidates nothing), and shareable with the host.
pub trait EnvValue: Clone + PartialEq + Send + Sync + fmt::Debug + 'static {}
impl<T: Clone + PartialEq + Send + Sync + fmt::Debug + 'static> EnvValue for T {}

/// Formats a type-erased stored value for diagnostics.
type DebugFn = dyn Fn(&(dyn Any + Send + Sync)) -> String + Send + Sync;

/// One stored value and the version it was set at.
#[derive(Clone)]
pub(crate) struct Stored {
    pub(crate) version: u64,
    pub(crate) value: Arc<dyn Any + Send + Sync>,
    pub(crate) debug: Arc<DebugFn>,
}

impl Stored {
    pub(crate) fn new<T: EnvValue>(version: u64, value: T) -> Self {
        Self {
            version,
            value: Arc::new(value),
            debug: Arc::new(|value| {
                value.downcast_ref::<T>().map_or_else(String::new, |value| format!("{value:?}"))
            }),
        }
    }

    pub(crate) fn get<T: EnvValue>(&self) -> Option<T> {
        self.value.downcast_ref::<T>().cloned()
    }

    pub(crate) fn same_as<T: EnvValue>(&self, value: &T) -> bool {
        self.value.downcast_ref::<T>().is_some_and(|current| current == value)
    }
}

/// A set of environment values — the root of a window's environment, or
/// what one subtree provides.
#[derive(Clone, Default)]
pub struct Environment {
    pub(crate) values: HashMap<&'static str, Stored>,
}

impl fmt::Debug for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        let mut names: Vec<_> = self.values.keys().collect();
        names.sort();
        for name in names {
            let stored = &self.values[name];
            map.entry(name, &(stored.debug)(&*stored.value));
        }
        map.finish()
    }
}

impl Environment {
    /// An empty environment: every key reads as its default.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The value of `key`, or its default.
    #[must_use]
    pub fn get<T: EnvValue>(&self, key: &EnvKey<T>) -> T {
        self.values.get(key.name).and_then(Stored::get).unwrap_or_else(|| key.default_value())
    }

    /// Sets `key` to `value` at `version`, returning whether the value
    /// changed.
    pub(crate) fn set<T: EnvValue>(&mut self, key: &EnvKey<T>, value: T, version: u64) -> bool {
        if self.values.get(key.name).is_some_and(|stored| stored.same_as(&value)) {
            return false;
        }
        self.values.insert(key.name, Stored::new(version, value));
        true
    }

    /// The names of every value set here.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.values.keys().copied()
    }

    /// Each value, formatted for diagnostics (the inspector's view).
    #[must_use]
    pub fn describe(&self) -> Vec<(&'static str, String)> {
        let mut out: Vec<_> = self
            .values
            .iter()
            .map(|(name, stored)| (*name, (stored.debug)(&*stored.value)))
            .collect();
        out.sort_by_key(|(name, _)| *name);
        out
    }
}

/// A value descendants publish upward, combined across publishers.
pub trait Preference: EnvValue {
    /// Combines `self` (what was accumulated so far, in tree order) with
    /// `next` (the next publisher's value).
    #[must_use]
    fn reduce(self, next: Self) -> Self;
}

/// A typed key for an upward preference.
pub struct PreferenceKey<T: 'static> {
    name: &'static str,
    _value: std::marker::PhantomData<fn() -> T>,
}

impl<T> PreferenceKey<T> {
    /// A preference named `name`.
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self { name, _value: std::marker::PhantomData }
    }

    /// The preference's name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }
}

impl<T> fmt::Debug for PreferenceKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PreferenceKey").field(&self.name).finish()
    }
}

// ---------------------------------------------------------------------------
// The framework's own values
// ---------------------------------------------------------------------------

/// The host's colour scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorScheme {
    /// Dark text on light surfaces.
    #[default]
    Light,
    /// Light text on dark surfaces.
    Dark,
}

/// The host's contrast setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Contrast {
    /// Ordinary contrast.
    #[default]
    Standard,
    /// The person asked for high contrast; the host's own colours win.
    High,
}

/// A coarse width or height class (`C22`): what an adaptive layout decides
/// by, rather than by device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum SizeClass {
    /// Narrower (or shorter) than 600 logical pixels: a phone held upright.
    Compact,
    /// 600 to 839: a small tablet, a phone on its side, a narrow window.
    #[default]
    Medium,
    /// 840 and up: a tablet, a desktop window.
    Expanded,
}

impl SizeClass {
    /// The class of a `length` in logical pixels.
    #[must_use]
    pub const fn of(length: u32) -> Self {
        if length < 600 {
            Self::Compact
        } else if length < 840 {
            Self::Medium
        } else {
            Self::Expanded
        }
    }
}

/// The size classes of both axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SizeClasses {
    /// The width class.
    pub width: SizeClass,
    /// The height class.
    pub height: SizeClass,
}

impl SizeClasses {
    /// The classes of a `width × height` area.
    #[must_use]
    pub const fn of(width: u32, height: u32) -> Self {
        Self { width: SizeClass::of(width), height: SizeClass::of(height) }
    }
}

/// A named minimum width, as the utility vocabulary's responsive variants
/// (`sm:`, `md:`, …) use it. The values are Tailwind CSS v4's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Breakpoint {
    /// 640 logical pixels.
    Sm,
    /// 768.
    Md,
    /// 1024.
    Lg,
    /// 1280.
    Xl,
    /// 1536.
    Xxl,
}

impl Breakpoint {
    /// Every breakpoint, smallest first.
    pub const ALL: [Self; 5] = [Self::Sm, Self::Md, Self::Lg, Self::Xl, Self::Xxl];

    /// Its minimum width in logical pixels.
    #[must_use]
    pub const fn min_width(self) -> u32 {
        match self {
            Self::Sm => 640,
            Self::Md => 768,
            Self::Lg => 1024,
            Self::Xl => 1280,
            Self::Xxl => 1536,
        }
    }

    /// Whether a width of `width` reaches this breakpoint.
    #[must_use]
    pub const fn reached_by(self, width: u32) -> bool {
        width >= self.min_width()
    }
}

/// How a foldable (or any multi-panel) device is held (`C22`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Posture {
    /// One flat surface — every host without a hinge.
    #[default]
    Flat,
    /// Half-open, with the hinge (in window coordinates) that content should
    /// not straddle.
    HalfOpened {
        /// The hinge's area.
        hinge: Rect,
    },
    /// Fully open with a hinge that occludes nothing but still divides.
    Separated {
        /// The hinge's area.
        hinge: Rect,
    },
}

/// Whether the window has the display to itself.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum WindowMode {
    /// A normal window, or a full-screen app.
    #[default]
    Full,
    /// Sharing the display side by side (split-screen, a snap layout):
    /// the fraction of the display's width it has, from 0 to 1.
    Split {
        /// The share of the display.
        fraction: Scalar,
    },
}

/// A locale, as a BCP 47 tag (`en-US`, `ar-EG`, `pl`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Locale {
    tag: String,
}

impl Default for Locale {
    fn default() -> Self {
        Self::new("en-US")
    }
}

impl Locale {
    /// The locale tagged `tag`.
    #[must_use]
    pub fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into() }
    }

    /// Its BCP 47 tag.
    #[must_use]
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// Its primary language subtag, lower-cased (`ar` for `ar-EG`).
    #[must_use]
    pub fn language(&self) -> String {
        self.tag.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase()
    }

    /// The direction its script is written in: right-to-left for Arabic,
    /// Hebrew, Persian, Urdu, Pashto, Sindhi, Uyghur, Yiddish, Dhivehi, and
    /// Kurdish (Sorani); left-to-right otherwise. A tag with an explicit
    /// script subtag (`az-Arab`) decides by the script.
    #[must_use]
    pub fn direction(&self) -> LayoutDirection {
        const RTL_LANGUAGES: [&str; 11] =
            ["ar", "he", "iw", "fa", "ur", "ps", "sd", "ug", "yi", "dv", "ckb"];
        const RTL_SCRIPTS: [&str; 6] = ["arab", "hebr", "thaa", "syrc", "nkoo", "adlm"];
        let lower = self.tag.to_ascii_lowercase();
        let mut parts = lower.split(['-', '_']);
        let language = parts.next().unwrap_or("");
        if let Some(script) = parts.find(|part| part.len() == 4) {
            return if RTL_SCRIPTS.contains(&script) {
                LayoutDirection::Rtl
            } else {
                LayoutDirection::Ltr
            };
        }
        if RTL_LANGUAGES.contains(&language) { LayoutDirection::Rtl } else { LayoutDirection::Ltr }
    }
}

/// The framework's own environment keys, fed from host traits by every
/// backend so no component queries the host directly.
pub mod keys {
    use super::{
        ColorScheme, Contrast, EnvKey, Locale, PointerPrecision, Posture, SizeClasses, WindowMode,
    };
    use crate::animation::MotionPreference;
    use crate::input::Scalar;
    use crate::layout::{EdgeInsets, LayoutDirection};

    /// The locale strings and formats follow.
    pub const LOCALE: EnvKey<Locale> = EnvKey::new("rustnative.locale", Locale::default);
    /// The layout direction, normally the locale's.
    pub const LAYOUT_DIRECTION: EnvKey<LayoutDirection> =
        EnvKey::new("rustnative.layout-direction", LayoutDirection::default);
    /// The person's text size, as a multiple of the host's default.
    pub const TEXT_SCALE: EnvKey<Scalar> = EnvKey::new("rustnative.text-scale", || Scalar::ONE);
    /// The window's size classes.
    pub const SIZE_CLASS: EnvKey<SizeClasses> =
        EnvKey::new("rustnative.size-class", SizeClasses::default);
    /// The host's colour scheme.
    pub const COLOR_SCHEME: EnvKey<ColorScheme> =
        EnvKey::new("rustnative.color-scheme", ColorScheme::default);
    /// Whether motion should be reduced.
    pub const REDUCED_MOTION: EnvKey<MotionPreference> =
        EnvKey::new("rustnative.reduced-motion", MotionPreference::default);
    /// The host's contrast setting.
    pub const CONTRAST: EnvKey<Contrast> = EnvKey::new("rustnative.contrast", Contrast::default);
    /// The device's posture.
    pub const POSTURE: EnvKey<Posture> = EnvKey::new("rustnative.posture", Posture::default);
    /// Insets content must stay clear of: notches, rounded corners, system
    /// bars.
    pub const SAFE_AREA: EnvKey<EdgeInsets> =
        EnvKey::new("rustnative.safe-area", EdgeInsets::default);
    /// Whether the window shares the display.
    pub const WINDOW_MODE: EnvKey<WindowMode> =
        EnvKey::new("rustnative.window-mode", WindowMode::default);
    /// The window's width in logical pixels — what the `sm:`/`md:`/…
    /// style variants key to (`PLAN.md` Milestone 58). Read it only when
    /// you need the number: every resize changes it; the size classes
    /// change only at their thresholds.
    pub const WINDOW_WIDTH: EnvKey<u32> = EnvKey::new("rustnative.window-width", || 1024);
    /// The primary pointer's precision (`pointer-coarse:`/`pointer-fine:`).
    pub const POINTER: EnvKey<PointerPrecision> =
        EnvKey::new("rustnative.pointer", PointerPrecision::default);
}

/// The precision of the host's primary pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PointerPrecision {
    /// A mouse, trackpad, or pen.
    #[default]
    Fine,
    /// A finger.
    Coarse,
}

/// Whether `insets` is all zero — a host with no safe-area constraints.
#[must_use]
pub fn is_unconstrained(insets: EdgeInsets) -> bool {
    insets == EdgeInsets::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_know_their_direction() {
        assert_eq!(Locale::new("ar-EG").direction(), LayoutDirection::Rtl);
        assert_eq!(Locale::new("he").direction(), LayoutDirection::Rtl);
        assert_eq!(Locale::new("en-US").direction(), LayoutDirection::Ltr);
        assert_eq!(Locale::new("pl").direction(), LayoutDirection::Ltr);
        assert_eq!(Locale::new("az-Arab").direction(), LayoutDirection::Rtl);
        assert_eq!(Locale::new("ar-Latn").direction(), LayoutDirection::Ltr);
        assert_eq!(Locale::new("ar-EG").language(), "ar");
    }

    #[test]
    fn size_classes_follow_the_documented_breakpoints() {
        assert_eq!(SizeClass::of(599), SizeClass::Compact);
        assert_eq!(SizeClass::of(600), SizeClass::Medium);
        assert_eq!(SizeClass::of(839), SizeClass::Medium);
        assert_eq!(SizeClass::of(840), SizeClass::Expanded);
        assert!(Breakpoint::Md.reached_by(768));
        assert!(!Breakpoint::Md.reached_by(767));
    }

    #[test]
    fn setting_an_equal_value_changes_nothing() {
        let mut environment = Environment::new();
        assert!(environment.set(&keys::COLOR_SCHEME, ColorScheme::Dark, 1));
        assert!(!environment.set(&keys::COLOR_SCHEME, ColorScheme::Dark, 2));
        assert_eq!(environment.get(&keys::COLOR_SCHEME), ColorScheme::Dark);
        assert_eq!(environment.get(&keys::CONTRAST), Contrast::Standard, "unset reads as default");
        assert_eq!(environment.describe(), vec![("rustnative.color-scheme", "Dark".to_owned())]);
    }
}
