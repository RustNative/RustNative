//! Native text measurement behind the portable `IntrinsicMeasurer`
//! contract.
//!
//! A node is measured by asking a widget of the kind that will realize it
//! — GTK's own `gtk_widget_measure` on a prototype `GtkLabel`, `GtkButton`,
//! or `GtkEntry` carrying the same text and the same style classes — so
//! the number includes Pango's shaping of the text in the font GTK will
//! draw it in *and* the theme's own padding and borders. A measurer that
//! computed text size from font metrics and added a guessed chrome would
//! drift from the widget the first time a theme changed its padding.

use std::cell::RefCell;

use gtk::prelude::*;
use rustnative_core::{Control, IntrinsicMeasurer, NodeKind, Size, Typography};

use super::rendering::controls;
use super::rendering::styling::StyleSheet;

/// Measures with prototype widgets of each kind.
pub(crate) struct GtkMeasurer<'a> {
    prototypes: &'a Prototypes,
    styles: &'a RefCell<StyleSheet>,
}

impl<'a> GtkMeasurer<'a> {
    pub(crate) const fn new(prototypes: &'a Prototypes, styles: &'a RefCell<StyleSheet>) -> Self {
        Self { prototypes, styles }
    }
}

/// One unparented widget of each measured kind, kept for the life of a
/// window and reconfigured per measurement.
#[derive(Debug)]
pub(crate) struct Prototypes {
    label: gtk::Label,
    button: gtk::Button,
    entry: gtk::Entry,
    tabs: gtk::Label,
}

impl Default for Prototypes {
    fn default() -> Self {
        let label = gtk::Label::new(None);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_xalign(0.0);
        let tabs = gtk::Label::new(None);
        Self { label, button: gtk::Button::with_label(""), entry: gtk::Entry::new(), tabs }
    }
}

/// `(minimum, natural)` along `orientation`, for a width of `for_size`.
fn measure(
    widget: &impl IsA<gtk::Widget>,
    orientation: gtk::Orientation,
    for_size: i32,
) -> (i32, i32) {
    let (minimum, natural, _, _) = widget.as_ref().measure(orientation, for_size);
    (minimum, natural)
}

fn to_size(width: i32, height: i32) -> Size {
    Size::new(u32::try_from(width.max(0)).unwrap_or(0), u32::try_from(height.max(0)).unwrap_or(0))
}

impl GtkMeasurer<'_> {
    /// Puts the style classes a node with `typography` would carry on
    /// `widget`, so it measures in that node's font.
    fn dress(&self, widget: &impl IsA<gtk::Widget>, typography: Option<&Typography>) {
        let class =
            typography.and_then(|typography| self.styles.borrow_mut().font_class(typography));
        if class.is_some() {
            self.styles.borrow_mut().flush();
        }
        super::rendering::styling::apply_class(widget.as_ref(), class.as_deref());
    }

    fn label(&self, text: &str, max_width: Option<i32>, typography: Option<&Typography>) -> Size {
        let label = &self.prototypes.label;
        self.dress(label, typography);
        label.set_text(text);
        let (_, natural_width) = measure(label, gtk::Orientation::Horizontal, -1);
        let width = max_width.map_or(natural_width, |limit| natural_width.min(limit.max(1)));
        let (_, height) = measure(label, gtk::Orientation::Vertical, width);
        to_size(width, height)
    }

    fn widget(&self, widget: &impl IsA<gtk::Widget>, typography: Option<&Typography>) -> Size {
        self.dress(widget, typography);
        let (_, width) = measure(widget, gtk::Orientation::Horizontal, -1);
        let (_, height) = measure(widget, gtk::Orientation::Vertical, width);
        to_size(width, height)
    }
}

impl IntrinsicMeasurer for GtkMeasurer<'_> {
    fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size {
        self.measure_styled(kind, text, max_width, None)
    }

    fn measure_styled(
        &self,
        kind: NodeKind,
        text: Option<&str>,
        max_width: Option<i32>,
        typography: Option<&Typography>,
    ) -> Size {
        let text = text.unwrap_or_default();
        match kind {
            NodeKind::Label => self.label(text, max_width, typography),
            NodeKind::Button => {
                self.prototypes.button.set_label(text);
                self.widget(&self.prototypes.button, typography)
            }
            NodeKind::TextInput => {
                // An entry's natural width is GTK's default character
                // count, not its text: a field does not grow as it fills.
                self.prototypes.entry.set_text(text);
                self.widget(&self.prototypes.entry, typography)
            }
            NodeKind::TabBar => {
                // The tab labels side by side, as the snapshot's text says.
                self.prototypes.tabs.set_text(text);
                let size = self.widget(&self.prototypes.tabs, typography);
                Size::new(size.width, size.height.saturating_add(16))
            }
            NodeKind::Control
            | NodeKind::Column
            | NodeKind::Row
            | NodeKind::Canvas
            | NodeKind::Surface => Size::new(0, 0),
        }
    }

    fn measure_foreign(&self, kind: &str) -> Size {
        super::foreign::preferred_size(kind).unwrap_or(Size::new(0, 0))
    }

    fn measure_control(&self, control: &Control) -> Size {
        // Measured on a real widget of the kind that realizes it.
        let widget = controls::prototype(control);
        self.widget(&widget, None)
    }
}
