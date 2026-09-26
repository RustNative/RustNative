//! Previews and the catalogue (`PLAN.md` Milestone 43, `C55`).
//!
//! A [`Preview`] renders one component, or one view, on its own. A
//! [`PreviewMatrix`] lists the configurations to show it in: colour scheme,
//! locale (including the pseudo-locale), text scale, direction, contrast,
//! and width. The application declares its previews in an ordinary function
//! (`pub fn previews() -> Vec<Preview>`), written in either syntax. Three
//! things read that function:
//!
//! - the [`Catalogue`], a component that browses the previews across the
//!   matrix. The application's `main` runs it on its own backend when
//!   `rustnative preview` asks (`RUSTNATIVE_PREVIEW`, [`requested`]);
//! - the headless backend's `preview_goldens`, which makes every preview in
//!   every configuration a golden test (`C55-3`), so previews cannot rot;
//! - [`PreviewFrame`], the component both of those render a preview inside,
//!   with the configuration provided to its subtree through the
//!   environment.
//!
//! ```
//! use rustnative_core::preview::{Preview, PreviewMatrix};
//! use rustnative_core::{Component, ComponentTree, Node};
//!
//! let greeting = Preview::new("greeting", || Node::label("hello", "Hello"))
//!     .with_matrix(PreviewMatrix::default().with_text_scales([1.0, 2.0]));
//! assert_eq!(greeting.matrix().configurations().len(), 2);
//!
//! // Rendered the way the catalogue and the goldens render it:
//! let configuration = greeting.matrix().configurations()[0].clone();
//! let tree = ComponentTree::new(rustnative_core::preview::PreviewFrame::new((greeting, configuration)));
//! assert!(tree.view().contains_id(rustnative_core::NodeId::from_key("preview-frame")));
//! ```
//!
//! <!-- single-syntax: previews wrap views written in either syntax; the example is about the preview API -->

use std::fmt;
use std::rc::Rc;

use crate::component::{Component, ComponentContext};
use crate::environment::{ColorScheme, Contrast, Locale, keys};
use crate::event::Event;
use crate::identity::NodeId;
use crate::input::Scalar;
use crate::layout::{LayoutDirection, LayoutStyle, SizeMode};
use crate::node::Node;

/// The environment variable `rustnative preview` sets for the application
/// it runs: its value is the name of the preview to open first, or empty.
pub const PREVIEW_VARIABLE: &str = "RUSTNATIVE_PREVIEW";

/// Whether this run was asked to show the catalogue rather than the
/// application — and, if so, which preview to open first (`""` for the
/// first).
#[must_use]
pub fn requested() -> Option<String> {
    std::env::var(PREVIEW_VARIABLE).ok()
}

/// The pseudo-locale's tag: accented, lengthened, bracketed text
/// (`crate::localization::pseudo_localize`).
pub const PSEUDO_LOCALE: &str = "en-XA";

/// One configuration a preview is shown in.
#[derive(Debug, Clone, PartialEq)]
pub struct Configuration {
    /// The colour scheme.
    pub scheme: ColorScheme,
    /// The locale; [`PSEUDO_LOCALE`] for the pseudo-locale.
    pub locale: Locale,
    /// The text scale.
    pub text_scale: f32,
    /// The layout direction.
    pub direction: LayoutDirection,
    /// The contrast preference.
    pub contrast: Contrast,
    /// The width the preview is laid out at, in logical pixels — which
    /// decides its size class.
    pub width: u32,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            scheme: ColorScheme::Light,
            locale: Locale::default(),
            text_scale: 1.0,
            direction: LayoutDirection::Ltr,
            contrast: Contrast::Standard,
            width: 480,
        }
    }
}

impl fmt::Display for Configuration {
    /// A short name, usable as part of a file name:
    /// `light-en-US-1x-ltr-standard-480`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}-{}-{}x-{}-{}-{}",
            match self.scheme {
                ColorScheme::Light => "light",
                ColorScheme::Dark => "dark",
            },
            self.locale.tag(),
            self.text_scale,
            match self.direction {
                LayoutDirection::Ltr => "ltr",
                LayoutDirection::Rtl => "rtl",
            },
            match self.contrast {
                Contrast::Standard => "standard",
                Contrast::High => "high",
            },
            self.width
        )
    }
}

/// The configurations a preview is shown in: every combination of the
/// values listed.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewMatrix {
    schemes: Vec<ColorScheme>,
    locales: Vec<Locale>,
    text_scales: Vec<f32>,
    directions: Vec<LayoutDirection>,
    contrasts: Vec<Contrast>,
    widths: Vec<u32>,
}

impl Default for PreviewMatrix {
    /// The one default configuration.
    fn default() -> Self {
        let base = Configuration::default();
        Self {
            schemes: vec![base.scheme],
            locales: vec![base.locale],
            text_scales: vec![base.text_scale],
            directions: vec![base.direction],
            contrasts: vec![base.contrast],
            widths: vec![base.width],
        }
    }
}

impl PreviewMatrix {
    /// Every scheme, the default and pseudo-locale, text scales 1, 1.5, and
    /// 2, both directions, and both contrasts — 48 configurations.
    #[must_use]
    pub fn full() -> Self {
        Self::default()
            .with_schemes([ColorScheme::Light, ColorScheme::Dark])
            .with_locales([Locale::default(), Locale::new(PSEUDO_LOCALE)])
            .with_text_scales([1.0, 1.5, 2.0])
            .with_directions([LayoutDirection::Ltr, LayoutDirection::Rtl])
            .with_contrasts([Contrast::Standard, Contrast::High])
    }

    /// The colour schemes.
    #[must_use]
    pub fn with_schemes(mut self, schemes: impl IntoIterator<Item = ColorScheme>) -> Self {
        self.schemes = schemes.into_iter().collect();
        self
    }

    /// The locales.
    #[must_use]
    pub fn with_locales(mut self, locales: impl IntoIterator<Item = Locale>) -> Self {
        self.locales = locales.into_iter().collect();
        self
    }

    /// The text scales.
    #[must_use]
    pub fn with_text_scales(mut self, scales: impl IntoIterator<Item = f32>) -> Self {
        self.text_scales = scales.into_iter().collect();
        self
    }

    /// The directions.
    #[must_use]
    pub fn with_directions(
        mut self,
        directions: impl IntoIterator<Item = LayoutDirection>,
    ) -> Self {
        self.directions = directions.into_iter().collect();
        self
    }

    /// The contrasts.
    #[must_use]
    pub fn with_contrasts(mut self, contrasts: impl IntoIterator<Item = Contrast>) -> Self {
        self.contrasts = contrasts.into_iter().collect();
        self
    }

    /// The widths.
    #[must_use]
    pub fn with_widths(mut self, widths: impl IntoIterator<Item = u32>) -> Self {
        self.widths = widths.into_iter().collect();
        self
    }

    /// Every configuration, in a stable order.
    #[must_use]
    pub fn configurations(&self) -> Vec<Configuration> {
        let mut out = Vec::new();
        for scheme in &self.schemes {
            for locale in &self.locales {
                for text_scale in &self.text_scales {
                    for direction in &self.directions {
                        for contrast in &self.contrasts {
                            for width in &self.widths {
                                out.push(Configuration {
                                    scheme: *scheme,
                                    locale: locale.clone(),
                                    text_scale: *text_scale,
                                    direction: *direction,
                                    contrast: *contrast,
                                    width: *width,
                                });
                            }
                        }
                    }
                }
            }
        }
        out
    }
}

type Render = dyn Fn(&mut ComponentContext<'_, ()>) -> Node;

/// One preview: a named view or component, and the matrix it is shown in.
#[derive(Clone)]
pub struct Preview {
    name: String,
    render: Rc<Render>,
    matrix: PreviewMatrix,
}

impl fmt::Debug for Preview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Preview").field("name", &self.name).finish_non_exhaustive()
    }
}

impl PartialEq for Preview {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && Rc::ptr_eq(&self.render, &other.render)
    }
}

impl Preview {
    /// A preview of the view `view` builds.
    pub fn new(name: impl Into<String>, view: impl Fn() -> Node + 'static) -> Self {
        Self {
            name: name.into(),
            render: Rc::new(move |_: &mut ComponentContext<'_, ()>| view()),
            matrix: PreviewMatrix::default(),
        }
    }

    /// A preview of component `C` created with `props` — its state, its
    /// events, and its tasks all live, as in the application.
    pub fn component<C>(name: impl Into<String>, props: C::Props) -> Self
    where
        C: Component,
    {
        Self {
            name: name.into(),
            render: Rc::new(move |context: &mut ComponentContext<'_, ()>| {
                context.child_with_props::<C, _>("previewed", props.clone(), C::new)
            }),
            matrix: PreviewMatrix::default(),
        }
    }

    /// Shows it across `matrix`.
    #[must_use]
    pub fn with_matrix(mut self, matrix: PreviewMatrix) -> Self {
        self.matrix = matrix;
        self
    }

    /// Its name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The configurations it is shown in.
    #[must_use]
    pub const fn matrix(&self) -> &PreviewMatrix {
        &self.matrix
    }
}

/// Renders one preview in one configuration, which it provides to the
/// preview's subtree through the environment.
#[derive(Debug)]
pub struct PreviewFrame {
    props: (Preview, Configuration),
}

impl Component for PreviewFrame {
    type Props = (Preview, Configuration);
    type Message = ();

    fn new(props: Self::Props) -> Self {
        Self { props }
    }
    fn props(&self) -> &Self::Props {
        &self.props
    }
    fn set_props(&mut self, props: Self::Props) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("preview-frame", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let (preview, configuration) = &self.props;
        context.provide_env(&keys::COLOR_SCHEME, configuration.scheme);
        context.provide_env(&keys::LOCALE, configuration.locale.clone());
        context.provide_env(&keys::TEXT_SCALE, Scalar::new(configuration.text_scale));
        context.provide_env(&keys::LAYOUT_DIRECTION, configuration.direction);
        context.provide_env(&keys::CONTRAST, configuration.contrast);
        context.provide_env(&keys::WINDOW_WIDTH, configuration.width);
        let previewed = (preview.render)(context);
        Node::column_with_layout(
            "preview-frame",
            [previewed],
            LayoutStyle::new()
                .width(SizeMode::Fixed(i32::try_from(configuration.width).unwrap_or(i32::MAX)))
                .direction(configuration.direction),
            crate::layout::ColumnStyle::new(),
        )
    }
}

/// The catalogue: every preview, browsable across its configurations.
///
/// A toolbar cycles the configuration (scheme, locale and pseudo-locale,
/// text scale, direction, contrast), a list chooses the preview, and the
/// stage shows it in a [`PreviewFrame`].
#[derive(Debug)]
pub struct Catalogue {
    previews: Rc<Vec<Preview>>,
    selected: usize,
    configuration: Configuration,
}

impl Catalogue {
    /// A catalogue of `previews`, opened at the preview named `first` (or
    /// the first).
    #[must_use]
    pub fn open(previews: Vec<Preview>, first: &str) -> Self {
        let selected = previews.iter().position(|preview| preview.name == first).unwrap_or(0);
        Self { previews: Rc::new(previews), selected, configuration: Configuration::default() }
    }

    /// The preview shown.
    #[must_use]
    pub fn selected(&self) -> Option<&Preview> {
        self.previews.get(self.selected)
    }

    /// The configuration it is shown in.
    #[must_use]
    pub const fn configuration(&self) -> &Configuration {
        &self.configuration
    }

    fn toolbar(&self) -> Node {
        let configuration = &self.configuration;
        Node::row(
            "toolbar",
            [
                Node::button("scheme", format!("Scheme: {:?}", configuration.scheme)),
                Node::button("locale", format!("Locale: {}", configuration.locale.tag())),
                Node::button("scale", format!("Text: {}×", configuration.text_scale)),
                Node::button("direction", format!("Direction: {:?}", configuration.direction)),
                Node::button("contrast", format!("Contrast: {:?}", configuration.contrast)),
            ],
        )
    }
}

impl Component for Catalogue {
    type Props = Rc<Vec<Preview>>;
    type Message = ();

    fn new(previews: Self::Props) -> Self {
        Self { previews, selected: 0, configuration: Configuration::default() }
    }
    fn props(&self) -> &Self::Props {
        &self.previews
    }
    fn set_props(&mut self, previews: Self::Props) {
        self.previews = previews;
    }
    fn view(&self) -> Node {
        Node::column("catalogue", [])
    }
    fn update(&mut self, event: Event) {
        let Event::Click { target } = event else { return };
        let configuration = &mut self.configuration;
        if target == NodeId::from_key("scheme") {
            configuration.scheme = match configuration.scheme {
                ColorScheme::Light => ColorScheme::Dark,
                ColorScheme::Dark => ColorScheme::Light,
            };
        } else if target == NodeId::from_key("locale") {
            configuration.locale = if configuration.locale.tag() == PSEUDO_LOCALE {
                Locale::default()
            } else {
                Locale::new(PSEUDO_LOCALE)
            };
        } else if target == NodeId::from_key("scale") {
            configuration.text_scale = match configuration.text_scale {
                scale if scale < 1.25 => 1.5,
                scale if scale < 1.75 => 2.0,
                _ => 1.0,
            };
        } else if target == NodeId::from_key("direction") {
            configuration.direction = match configuration.direction {
                LayoutDirection::Ltr => LayoutDirection::Rtl,
                LayoutDirection::Rtl => LayoutDirection::Ltr,
            };
        } else if target == NodeId::from_key("contrast") {
            configuration.contrast = match configuration.contrast {
                Contrast::Standard => Contrast::High,
                Contrast::High => Contrast::Standard,
            };
        } else if let Some(index) = (0..self.previews.len())
            .find(|index| target == NodeId::from_key(&format!("preview-{index}")))
        {
            self.selected = index;
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let list = Node::column(
            "list",
            self.previews.iter().enumerate().map(|(index, preview)| {
                Node::button(format!("preview-{index}"), preview.name.clone())
            }),
        );
        let stage = match self.selected() {
            Some(preview) => context.child_with_props::<PreviewFrame, _>(
                "stage",
                (preview.clone(), self.configuration.clone()),
                PreviewFrame::new,
            ),
            None => Node::label("stage", "This application declares no previews."),
        };
        Node::column("catalogue", [self.toolbar(), Node::row("body", [list, stage])])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::ComponentTree;

    #[test]
    fn the_full_matrix_is_every_combination() {
        let configurations = PreviewMatrix::full().configurations();
        assert_eq!(configurations.len(), 48);
        let names: std::collections::HashSet<String> =
            configurations.iter().map(ToString::to_string).collect();
        assert_eq!(names.len(), 48, "every configuration has its own name");
    }

    #[test]
    fn the_catalogue_cycles_configurations_and_selects_previews() {
        let previews = vec![
            Preview::new("one", || Node::label("a", "One")),
            Preview::new("two", || Node::label("b", "Two")),
        ];
        let mut tree = ComponentTree::new(Catalogue::open(previews, "two"));
        let shows = |tree: &ComponentTree, key: &str| tree.find_node(None, key).is_some();
        assert!(shows(&tree, "b"), "opened at the named preview");
        tree.dispatch(Event::Click { target: NodeId::from_key("preview-0") });
        assert!(shows(&tree, "a"));
        for key in ["scheme", "locale", "scale", "direction", "contrast"] {
            tree.dispatch(Event::Click { target: NodeId::from_key(key) });
        }
        let view = tree.view();
        let mut labels = Vec::new();
        view.visit(&mut |node, _, _| {
            if let Node::Button(button) = node {
                labels.push(button.text().to_owned());
            }
        });
        assert!(labels.contains(&"Scheme: Dark".to_owned()), "{labels:?}");
        assert!(labels.contains(&format!("Locale: {PSEUDO_LOCALE}")));
        assert!(labels.contains(&"Text: 1.5×".to_owned()));
    }
}
