//! Thread affinity, proven by the compiler: every type that holds the
//! declarative tree is `!Send`, so it cannot be moved off the thread that
//! owns it. `assert_not_send::<_, T>()` only compiles when `T` is `!Send` —
//! for a `Send` type both `NotSend` impls apply and the marker is ambiguous
//! — so this file failing to build is the test failing.

use rustnative_core::affinity::NotSend;
use rustnative_core::{
    Application, ComponentContext, ComponentTree, LocalPool, NativeHandle, TaskScope, UiThread,
    Unchecked,
};

fn assert_not_send<M, T: ?Sized + NotSend<M>>() {}

#[test]
fn tree_holding_types_cannot_cross_threads() {
    assert_not_send::<_, Application>();
    assert_not_send::<_, ComponentTree>();
    assert_not_send::<_, ComponentContext<'static, ()>>();
    assert_not_send::<_, TaskScope>();
    assert_not_send::<_, LocalPool>();
    assert_not_send::<_, UiThread>();
    assert_not_send::<_, NativeHandle<Unchecked>>();
}
