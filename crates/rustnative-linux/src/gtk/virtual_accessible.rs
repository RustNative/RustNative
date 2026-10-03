//! Virtual elements (`rustnative_core::VirtualElement`): semantic children
//! of a node with no widget of their own — a chart's bars, a canvas' hit
//! regions — exposed to AT-SPI as accessible objects GTK carries like any
//! widget's (GTK ≥ 4.10 lets any object implement `GtkAccessible`).
//!
//! Each element is an `RnVirtual`: its role, name, value, states, and
//! bounds are the element's; its accessible parent is the node's widget,
//! which lists its elements before its widget children
//! (`RnLayout`'s `first_accessible_child`). An element with a range value
//! takes a screen reader's value change (`GtkAccessibleRange`), delivered
//! as `Event::AccessibilityAction` with the element's id.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use rustnative_core::{NodeId, Rect};

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use rustnative_core::Rect;

    pub(crate) type ValueCallback = Box<dyn Fn(f64)>;

    #[derive(Default)]
    pub struct RnVirtual {
        pub(super) host: glib::WeakRef<gtk::Widget>,
        pub(super) next: RefCell<Option<gtk::Accessible>>,
        pub(super) bounds: Cell<Rect>,
        pub(super) role: Cell<Option<gtk::AccessibleRole>>,
        pub(super) context: OnceCell<Option<gtk::ATContext>>,
        pub(super) on_set_value: RefCell<Option<ValueCallback>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RnVirtual {
        const NAME: &'static str = "RnVirtual";
        type Type = super::RnVirtual;
        type Interfaces = (gtk::Accessible, gtk::AccessibleRange);
    }

    impl ObjectImpl for RnVirtual {
        fn properties() -> &'static [glib::ParamSpec] {
            // `GtkAccessible` requires an `accessible-role` property.
            static PROPERTIES: std::sync::OnceLock<Vec<glib::ParamSpec>> =
                std::sync::OnceLock::new();
            PROPERTIES.get_or_init(|| {
                vec![glib::ParamSpecOverride::for_interface::<gtk::Accessible>("accessible-role")]
            })
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            if pspec.name() == "accessible-role" {
                self.role.set(value.get().ok());
            }
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "accessible-role" => {
                    self.role.get().unwrap_or(gtk::AccessibleRole::Generic).to_value()
                }
                _ => glib::Value::from_type(pspec.value_type()),
            }
        }

        fn dispose(&self) {
            self.on_set_value.borrow_mut().take();
            self.next.borrow_mut().take();
        }
    }

    impl AccessibleImpl for RnVirtual {
        fn platform_state(&self, _state: gtk::AccessiblePlatformState) -> bool {
            false
        }

        fn bounds(&self) -> Option<(i32, i32, i32, i32)> {
            let rect = self.bounds.get();
            Some((rect.x, rect.y, rect.width, rect.height))
        }

        fn at_context(&self) -> Option<gtk::ATContext> {
            // GTK can ask during finalization, when `obj()` must not be
            // touched; by then the context exists.
            let context = self.context.get_or_init(|| {
                let obj = self.obj();
                let display = self
                    .host
                    .upgrade()
                    .map(|host| host.display())
                    .or_else(gtk::gdk::Display::default)?;
                gtk::ATContext::create(
                    self.role.get().unwrap_or(gtk::AccessibleRole::Generic),
                    obj.upcast_ref::<gtk::Accessible>(),
                    &display,
                )
            });
            context.clone()
        }

        fn accessible_parent(&self) -> Option<gtk::Accessible> {
            self.host.upgrade().map(glib::object::Cast::upcast)
        }

        fn first_accessible_child(&self) -> Option<gtk::Accessible> {
            None
        }

        fn next_accessible_sibling(&self) -> Option<gtk::Accessible> {
            self.next.borrow().clone()
        }
    }

    impl AccessibleRangeImpl for RnVirtual {
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
    /// One virtual element (see the module documentation).
    pub struct RnVirtual(ObjectSubclass<imp::RnVirtual>)
        @implements gtk::Accessible, gtk::AccessibleRange;
}

impl RnVirtual {
    /// An element of `host`, with `role`, at `bounds` in the host's
    /// coordinates.
    pub(crate) fn new(host: &gtk::Widget, role: gtk::AccessibleRole, bounds: Rect) -> Self {
        let element: Self = glib::Object::builder().property("accessible-role", role).build();
        element.imp().host.set(Some(host));
        element.imp().bounds.set(bounds);
        element
    }

    /// Moves the element.
    pub(crate) fn set_bounds(&self, bounds: Rect) {
        self.imp().bounds.set(bounds);
    }

    /// The element's role.
    pub(crate) fn role(&self) -> gtk::AccessibleRole {
        self.imp().role.get().unwrap_or(gtk::AccessibleRole::Generic)
    }

    /// Sets the accessible that follows this element among its host's
    /// accessible children.
    pub(crate) fn set_next(&self, next: Option<gtk::Accessible>) {
        *self.imp().next.borrow_mut() = next;
    }

    /// Calls `callback` when assistive technology sets the element's value.
    pub(crate) fn connect_value_requested(&self, callback: impl Fn(f64) + 'static) {
        *self.imp().on_set_value.borrow_mut() = Some(Box::new(callback));
    }
}

/// One node's virtual elements, by element id, in declaration order.
#[derive(Debug, Default)]
pub(crate) struct Elements {
    pub(crate) items: Vec<(NodeId, RnVirtual)>,
}

impl Elements {
    /// Links the elements to each other and to the host's first widget
    /// child, and hands the first to the host.
    pub(crate) fn link(&self, host: &super::layout_widget::RnLayout) {
        let widget_child = host.first_child().map(glib::object::Cast::upcast::<gtk::Accessible>);
        for (index, (_, element)) in self.items.iter().enumerate() {
            let next = self
                .items
                .get(index + 1)
                .map(|(_, next)| next.clone().upcast::<gtk::Accessible>())
                .or_else(|| widget_child.clone());
            element.set_next(next);
        }
        host.set_virtual_children(self.items.iter().map(|(_, element)| element.clone()).collect());
    }
}
