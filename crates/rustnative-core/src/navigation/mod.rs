//! Moving between screens: routes, navigation stacks, and tabs.
//!
//! None of this is a second component runtime. A [`NavigationStack`] is
//! data in the state of the component that shows it, each screen is that
//! component's keyed child, and screens below the top are kept alive by
//! still being rendered — [hidden](crate::Node::hidden). Tabs work the same
//! way: a [`crate::Node::tab_bar`] reports the choice
//! ([`crate::Event::TabSelected`]) and every tab's content stays mounted,
//! hidden unless selected. Navigation integrates with the managed
//! component tree by *being* the managed component tree.
//!
//! [`Route`] and [`Router`] turn paths into named routes with typed
//! parameters, which is what a deep link ([`crate::Event::DeepLink`]) is
//! routed by.

mod route;
mod stack;

pub use route::{Route, RouteError, RouteParams, Router, url_path};
pub use stack::{
    EntryId, NavigationCommand, NavigationEntry, NavigationStack, Navigator, SAVED_STATE_BUDGET,
    SavedStateTooLarge,
};
