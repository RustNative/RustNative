//! Native measurement behind the portable `IntrinsicMeasurer` contract.
//!
//! A node is measured on a prototype of the very widget that will realize
//! it (`RnMeasure.measure`): the device theme's own padding, minimum sizes,
//! and fonts, and `StaticLayout`'s shaping and line breaking of the text,
//! produce the number. The answer is in device pixels; the layout engine
//! works in dp, so it is divided by the density and rounded up (a node is
//! never laid out narrower than its text).

use std::cell::RefCell;
use std::collections::HashMap;

use rustnative_core::{Control, IntrinsicMeasurer, NodeKind, Size, Typography};

use crate::jni_host::{Arg, Class, JavaRef, Ret, call_static};
use crate::protocol;
use crate::styling::{family, font_pixels};
use crate::units::{to_dp, to_px};

/// Measurements kept before the cache starts over.
const MEASURED_CAP: usize = 8192;

/// What a measurement depends on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MeasureKey {
    kind: i32,
    text: String,
    items: Vec<String>,
    max_width: Option<i32>,
    font: Option<String>,
}

/// One window's measurement cache. Forgotten when the theme, density, or
/// font scale changes.
#[derive(Debug, Default)]
pub(crate) struct Measurements {
    measured: RefCell<HashMap<MeasureKey, Size>>,
}

impl Measurements {
    pub(crate) fn forget(&self) {
        self.measured.borrow_mut().clear();
        let _ = call_static(Class::Measure, "forget", "()V", &[]);
    }
}

/// Measures through the activity's prototype views.
pub(crate) struct AndroidMeasurer<'a> {
    pub(crate) activity: &'a JavaRef,
    pub(crate) density: f32,
    pub(crate) text_scale: f32,
    pub(crate) cache: &'a Measurements,
}

impl AndroidMeasurer<'_> {
    fn measure_kind(
        &self,
        kind: i32,
        text: Option<&str>,
        items: &[String],
        max_width: Option<i32>,
        typography: Option<&Typography>,
    ) -> Size {
        let key = MeasureKey {
            kind,
            text: text.unwrap_or_default().to_owned(),
            items: items.to_vec(),
            max_width,
            font: typography.map(|typography| format!("{typography:?}")),
        };
        if let Some(size) = self.cache.measured.borrow().get(&key) {
            return *size;
        }
        let (font_size, weight, family) = typography.map_or((0.0, 0, None), |typography| {
            (
                font_pixels(typography.size, self.text_scale, self.density),
                i32::from(typography.weight),
                family(&typography.family),
            )
        });
        let max_px = max_width.map_or(-1, |width| to_px(width.max(1), self.density));
        let items_arg = if items.is_empty() { Arg::Null } else { Arg::Strs(items) };
        let measured = call_static(
            Class::Measure,
            "measure",
            "(Landroid/app/Activity;ILjava/lang/String;[Ljava/lang/String;IFILjava/lang/String;)[I",
            &[
                Arg::Obj(self.activity),
                Arg::Int(kind),
                Arg::OptStr(text),
                items_arg,
                Arg::Int(max_px),
                Arg::Float(font_size),
                Arg::Int(weight),
                Arg::OptStr(family.as_deref()),
            ],
        )
        .map_or_else(
            |error| {
                crate::log::warn(&format!("measuring a node of kind {kind} failed: {error}"));
                Vec::new()
            },
            Ret::ints,
        );
        let size = Size::new(
            to_dp(measured.first().copied().unwrap_or(0), self.density),
            to_dp(measured.get(1).copied().unwrap_or(0), self.density),
        );
        let mut cache = self.cache.measured.borrow_mut();
        if cache.len() >= MEASURED_CAP {
            cache.clear();
        }
        cache.insert(key, size);
        size
    }
}

impl IntrinsicMeasurer for AndroidMeasurer<'_> {
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
        let java_kind = match kind {
            NodeKind::Label => protocol::LABEL,
            NodeKind::Button => protocol::BUTTON,
            NodeKind::TextInput => protocol::TEXT_INPUT,
            NodeKind::TabBar => protocol::TAB_BAR,
            _ => return Size::new(0, 0),
        };
        if java_kind == protocol::TAB_BAR {
            // The snapshot's text is the labels joined; the tab bar measures
            // its real tabs.
            let labels: Vec<String> =
                text.unwrap_or_default().split("     ").map(str::to_owned).collect();
            return self.measure_kind(java_kind, None, &labels, max_width, typography);
        }
        self.measure_kind(java_kind, text, &[], max_width, typography)
    }

    fn measure_foreign(&self, kind: &str) -> Size {
        crate::foreign::preferred_size(kind, self.density).unwrap_or(Size::new(0, 0))
    }

    fn measure_control(&self, control: &Control) -> Size {
        let kind = crate::rendering::controls::control_kind(control);
        match control {
            Control::Select { options, .. } => self.measure_kind(kind, None, options, None, None),
            Control::ListBox { items, .. } => {
                // A list shows its items; it is measured at its first few rows.
                let shown: Vec<String> = items.iter().take(6).cloned().collect();
                self.measure_kind(kind, None, &shown, None, None)
            }
            Control::Image { image } => Size::new(image.width(), image.height()),
            _ => self.measure_kind(kind, control.text().as_deref(), &[], None, None),
        }
    }
}
