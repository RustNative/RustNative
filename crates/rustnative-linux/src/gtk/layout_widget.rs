//! `RnLayout`: the framework's container widget.
//!
//! GTK's own containers (`GtkBox`, `GtkGrid`) run GTK's layout. This
//! framework's layout is its own — the portable engine in
//! `rustnative_core::layout`, the same on every backend — so a container
//! here is a widget that *places* its children rather than arranging them:
//! the renderer computes every rectangle and hands each child its
//! rectangle ([`RnLayout::place`]), and `size_allocate` gives each child
//! exactly that. This is the GTK form of the Windows backend positioning
//! child `HWND`s with `SetWindowPos`.
//!
//! A container never asks for space of its own (its parent's rectangle
//! for it is final), except the content of a scroll container, which
//! reports its content size so `GtkScrolledWindow` knows how far it
//! scrolls ([`RnLayout::set_content_size`]).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use rustnative_core::Rect;

mod imp {
    use super::{Cell, HashMap, Rect, RefCell, glib};
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    /// A callback told the container's new size.
    pub(crate) type ResizeCallback = Box<dyn Fn(i32, i32)>;

    #[derive(Default)]
    pub struct RnLayout {
        /// Each child's rectangle, in this container's coordinates.
        pub(super) placements: RefCell<HashMap<gtk::Widget, Rect>>,
        /// The size this container asks for: `None` for an ordinary
        /// container (its parent decides), the content size for the
        /// content of a scroll container.
        pub(super) content_size: Cell<Option<(i32, i32)>>,
        /// Told the new size when an allocation changes it (a window's
        /// root, whose size is the window's).
        pub(super) on_resize: RefCell<Option<ResizeCallback>>,
        /// Told the size on every allocation (a native surface's host,
        /// which must follow moves as well as resizes).
        pub(super) on_allocate: RefCell<Option<ResizeCallback>>,
        pub(super) last_size: Cell<(i32, i32)>,
        /// Virtual elements, listed before the widget children to
        /// assistive technology (`gtk::virtual_accessible`).
        pub(super) virtual_children: RefCell<Vec<crate::gtk::virtual_accessible::RnVirtual>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RnLayout {
        const NAME: &'static str = "RnLayout";
        type Type = super::RnLayout;
        type ParentType = gtk::Widget;
        // Re-implemented so its accessible children can lead with the
        // virtual elements.
        type Interfaces = (gtk::Accessible,);

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("rn-layout");
        }
    }

    impl ObjectImpl for RnLayout {
        fn dispose(&self) {
            // The map holds its children strongly; a disposed container
            // must not keep them alive.
            self.placements.borrow_mut().clear();
            self.on_resize.borrow_mut().take();
            self.on_allocate.borrow_mut().take();
            self.virtual_children.borrow_mut().clear();
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl AccessibleImpl for RnLayout {
        fn first_accessible_child(&self) -> Option<gtk::Accessible> {
            self.virtual_children
                .borrow()
                .first()
                .map(|element| element.clone().upcast())
                .or_else(|| self.parent_first_accessible_child())
        }
    }

    impl WidgetImpl for RnLayout {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::ConstantSize
        }

        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let size = self.content_size.get().map_or(0, |(width, height)| {
                if orientation == gtk::Orientation::Horizontal { width } else { height }
            });
            (size, size, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            if self.last_size.replace((width, height)) != (width, height) {
                if let Some(callback) = self.on_resize.borrow().as_ref() {
                    callback(width, height);
                }
            }
            if let Some(callback) = self.on_allocate.borrow().as_ref() {
                callback(width, height);
            }
            let placements = self.placements.borrow();
            let mut child = self.obj().first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                let rect = placements.get(&widget).copied().unwrap_or_default();
                // GTK requires a measure before every allocation; the
                // result is not used, because the framework's layout has
                // already decided the size.
                let _ = widget.measure(gtk::Orientation::Horizontal, -1);
                let _ = widget.measure(gtk::Orientation::Vertical, rect.width.max(0));
                widget.size_allocate(
                    &gtk::Allocation::new(rect.x, rect.y, rect.width.max(0), rect.height.max(0)),
                    -1,
                );
            }
        }
    }
}

glib::wrapper! {
    /// The framework's container widget (see the module documentation).
    pub struct RnLayout(ObjectSubclass<imp::RnLayout>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl RnLayout {
    /// A container with the accessible `role`, which GTK fixes at
    /// construction.
    pub(crate) fn with_role(role: gtk::AccessibleRole) -> Self {
        glib::Object::builder().property("accessible-role", role).build()
    }

    /// Adds `child` as this container's last child.
    pub(crate) fn append(&self, child: &impl IsA<gtk::Widget>) {
        child.as_ref().set_parent(self);
    }

    /// Removes `child`, which must be a child of this container.
    pub(crate) fn remove(&self, child: &impl IsA<gtk::Widget>) {
        let child = child.upcast_ref::<gtk::Widget>();
        self.imp().placements.borrow_mut().remove(child);
        if child.parent().as_ref() == Some(self.upcast_ref()) {
            child.unparent();
        }
    }

    /// Gives `child` its rectangle. Returns whether it moved.
    pub(crate) fn place(&self, child: &impl IsA<gtk::Widget>, rect: Rect) -> bool {
        let previous = self
            .imp()
            .placements
            .borrow_mut()
            .insert(child.upcast_ref::<gtk::Widget>().clone(), rect);
        let moved = previous != Some(rect);
        if moved {
            self.queue_allocate();
        }
        moved
    }

    /// The rectangle `child` was given, if any.
    #[cfg(test)]
    pub(crate) fn placement(&self, child: &impl IsA<gtk::Widget>) -> Option<Rect> {
        self.imp().placements.borrow().get(child.upcast_ref::<gtk::Widget>()).copied()
    }

    /// Asks for `size` (the content of a scroll container), or nothing.
    pub(crate) fn set_content_size(&self, size: Option<(i32, i32)>) {
        if self.imp().content_size.replace(size) != size {
            self.queue_resize();
        }
    }

    /// Lists `elements` as this container's first accessible children.
    pub(crate) fn set_virtual_children(
        &self,
        elements: Vec<crate::gtk::virtual_accessible::RnVirtual>,
    ) {
        *self.imp().virtual_children.borrow_mut() = elements;
    }

    /// Calls `callback` with the size on every allocation.
    pub(crate) fn connect_allocated(&self, callback: impl Fn(i32, i32) + 'static) {
        *self.imp().on_allocate.borrow_mut() = Some(Box::new(callback));
    }

    /// Calls `callback` with the new size whenever an allocation changes it.
    pub(crate) fn connect_resized(&self, callback: impl Fn(i32, i32) + 'static) {
        *self.imp().on_resize.borrow_mut() = Some(Box::new(callback));
    }

    /// The size GTK last allocated.
    #[cfg(test)]
    pub(crate) fn allocated_size(&self) -> (i32, i32) {
        self.imp().last_size.get()
    }
}

impl Default for RnLayout {
    fn default() -> Self {
        Self::with_role(gtk::AccessibleRole::Generic)
    }
}

/// What a subclass of [`RnLayout`] implements.
pub(crate) trait RnLayoutImpl: WidgetImpl {}

// SAFETY: `RnLayout`'s class and instance structs are GTK's widget structs
// with nothing added (its state lives in the Rust `imp` struct), so a
// subclass's class and instance extend them exactly as they would extend
// `GtkWidget`'s.
unsafe impl<T: RnLayoutImpl> IsSubclassable<T> for RnLayout {}

/// A container that is itself a custom control to assistive technology: it
/// declared an Invoke action (which AT-SPI performs as "activate", through
/// the widget's activate signal) or a range value (which AT-SPI sets
/// through the `Value` interface, `GtkAccessibleRange`). Kept a separate
/// type so that ordinary containers do not advertise either.
mod control_imp {
    use std::cell::RefCell;
    use std::sync::OnceLock;

    use gtk::glib;
    use gtk::glib::subclass::Signal;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    pub(crate) type ValueCallback = Box<dyn Fn(f64)>;

    #[derive(Default)]
    pub struct RnControl {
        pub(super) on_set_value: RefCell<Option<ValueCallback>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RnControl {
        const NAME: &'static str = "RnControl";
        type Type = super::RnControl;
        type ParentType = super::RnLayout;
        type Interfaces = (gtk::AccessibleRange,);

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("rn-control");
            klass.set_activate_signal_from_name("activate");
            klass.install_action("activate", None, |control, _, _| {
                control.emit_by_name::<()>("activate", &[]);
            });
        }
    }

    impl ObjectImpl for RnControl {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![Signal::builder("activate").action().build()])
        }

        fn dispose(&self) {
            self.on_set_value.borrow_mut().take();
        }
    }

    impl WidgetImpl for RnControl {}
    impl super::RnLayoutImpl for RnControl {}
    impl AccessibleImpl for RnControl {}

    impl AccessibleRangeImpl for RnControl {
        fn set_current_value(&self, value: f64) -> bool {
            match self.on_set_value.borrow().as_ref() {
                Some(callback) => {
                    callback(value);
                    true
                }
                None => false,
            }
        }
    }
}

glib::wrapper! {
    /// A container that is a custom control (see `control_imp`).
    pub struct RnControl(ObjectSubclass<control_imp::RnControl>)
        @extends RnLayout, gtk::Widget,
        @implements gtk::Accessible, gtk::AccessibleRange, gtk::Buildable, gtk::ConstraintTarget;
}

impl RnControl {
    /// A custom control with the accessible `role`.
    pub(crate) fn with_role(role: gtk::AccessibleRole) -> Self {
        glib::Object::builder().property("accessible-role", role).build()
    }

    /// Calls `callback` when assistive technology activates the control.
    pub(crate) fn connect_activated(&self, callback: impl Fn() + 'static) {
        self.connect_local("activate", false, move |_| {
            callback();
            None
        });
    }

    /// Calls `callback` when assistive technology sets its value.
    pub(crate) fn connect_value_requested(&self, callback: impl Fn(f64) + 'static) {
        *self.imp().on_set_value.borrow_mut() = Some(Box::new(callback));
    }
}
