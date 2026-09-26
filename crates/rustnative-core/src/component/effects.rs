//! Reactive effects: work that runs after a render commits and is retained
//! across renders while its declared dependencies compare equal.

use std::any::Any;
use std::future::Future;
use std::time::Duration;

use crate::scheduler::{SleepFuture, TaskHandle, TaskScope};

/// Work performed when an effect is replaced or its component is unmounted.
///
/// Effect cleanups run before their effect-owned tasks are cancelled,
/// allowing subscriptions to be detached at a well-defined lifecycle
/// boundary.
pub type EffectCleanup = Box<dyn FnOnce()>;

/// Capability passed to an effect after its component's render is
/// committed. Tasks spawned through this context belong to this particular
/// effect run, rather than merely to the whole component.
#[derive(Clone)]
pub struct EffectContext {
    pub(crate) task_scope: TaskScope,
}

impl EffectContext {
    /// Spawns `future` on this effect's task scope.
    pub fn spawn<M, F>(&self, future: F) -> TaskHandle
    where
        M: Send + 'static,
        F: Future<Output = M> + Send + 'static,
    {
        self.task_scope.spawn(future)
    }

    /// Returns this effect's task scope.
    #[must_use]
    pub fn task_scope(&self) -> TaskScope {
        self.task_scope.clone()
    }

    /// Returns a future that completes after `duration` (see
    /// [`crate::scheduler::Scheduler::sleep`]).
    #[must_use]
    pub fn sleep(&self, duration: Duration) -> SleepFuture {
        self.task_scope.scheduler().sleep(duration)
    }
}

/// Type-erased effect dependencies that retain the API's equality contract.
///
/// Hashes are deliberately not used for lifecycle decisions: equal hashes do
/// not prove equal dependency values, so an effect whose dependency type
/// simply implements `PartialEq` (not `Hash`) is compared exactly, by
/// downcasting back to its original concrete type and calling `==` — never
/// approximated.
pub(crate) struct EffectDependencies {
    value: Box<dyn Any>,
    type_id: std::any::TypeId,
    equals: fn(&dyn Any, &dyn Any) -> bool,
}

impl EffectDependencies {
    pub(crate) fn new<D: PartialEq + 'static>(value: D) -> Self {
        Self {
            value: Box::new(value),
            type_id: std::any::TypeId::of::<D>(),
            equals: |left, right| {
                left.downcast_ref::<D>()
                    .zip(right.downcast_ref::<D>())
                    .is_some_and(|(left, right)| left == right)
            },
        }
    }

    pub(crate) fn equals(&self, other: &Self) -> bool {
        self.type_id == other.type_id && (self.equals)(self.value.as_ref(), other.value.as_ref())
    }
}

pub(crate) struct DeclaredEffect {
    pub(crate) key: String,
    pub(crate) dependencies: EffectDependencies,
    pub(crate) run: Box<dyn FnOnce(EffectContext) -> EffectCleanup>,
}

pub(crate) struct EffectEntry {
    pub(crate) dependencies: EffectDependencies,
    pub(crate) task_scope: TaskScope,
    pub(crate) cleanup: Option<EffectCleanup>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependencies_of_different_types_are_never_equal() {
        let a = EffectDependencies::new(1i32);
        let b = EffectDependencies::new(1u32);
        assert!(!a.equals(&b));
    }

    #[test]
    fn dependencies_use_partial_eq_not_hashing() {
        let a = EffectDependencies::new(vec![1, 2, 3]);
        let b = EffectDependencies::new(vec![1, 2, 3]);
        let c = EffectDependencies::new(vec![1, 2, 4]);
        assert!(a.equals(&b));
        assert!(!a.equals(&c));
    }
}
