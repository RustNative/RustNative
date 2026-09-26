//! Batching colocated data requirements (`C29`): each component asks for
//! its own item, and one request fetches them all.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use rustnative_core::Background;

use crate::query::{LocalFuture, QueryError};

type Answer<V> = Rc<RefCell<(Option<Result<V, QueryError>>, Option<Waker>)>>;
type Load<K, V> = Rc<dyn Fn(Vec<K>) -> LocalFuture<Result<HashMap<K, V>, QueryError>>>;

struct Inner<K, V> {
    background: Background,
    load: Load<K, V>,
    pending: RefCell<Vec<(K, Answer<V>)>>,
    scheduled: Cell<bool>,
    batches: Cell<u64>,
}

/// Collects the keys asked for while the UI thread is busy, and loads them
/// in one request when it is next free — so a list whose every row asks for
/// its own author makes one request for all the authors, and each row still
/// receives only its own.
///
/// Use it from a [`crate::Query::local`] fetch:
///
/// ```no_run
/// # use std::collections::HashMap;
/// # use rustnative_data::{BatchLoader, Query, QueryError};
/// # fn f(background: rustnative_core::Background) {
/// let authors = BatchLoader::new(&background, |ids: Vec<u32>| async move {
///     // One request for every id asked for at once.
///     Ok::<_, QueryError>(ids.into_iter().map(|id| (id, format!("Author {id}"))).collect::<HashMap<_, _>>())
/// });
/// let loader = authors.clone();
/// let query = Query::local(["author", "7"], move || loader.load(7));
/// # let _ = query;
/// # }
/// ```
pub struct BatchLoader<K, V> {
    inner: Rc<Inner<K, V>>,
}

impl<K, V> Clone for BatchLoader<K, V> {
    fn clone(&self) -> Self {
        Self { inner: Rc::clone(&self.inner) }
    }
}

impl<K, V> PartialEq for BatchLoader<K, V> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<K, V> fmt::Debug for BatchLoader<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BatchLoader").field("batches", &self.inner.batches.get()).finish()
    }
}

/// A value a [`BatchLoader`] will deliver.
pub struct Loading<V> {
    answer: Answer<V>,
}

impl<V> Future for Loading<V> {
    type Output = Result<V, QueryError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut answer = self.answer.borrow_mut();
        if let Some(result) = answer.0.take() {
            return Poll::Ready(result);
        }
        answer.1 = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl<K, V> BatchLoader<K, V>
where
    K: Clone + Eq + Hash + Send + 'static,
    V: Clone + Send + 'static,
{
    /// A loader whose `load` fetches every key of a batch, off the UI thread.
    pub fn new<F, Fut>(background: &Background, load: F) -> Self
    where
        F: Fn(Vec<K>) -> Fut + 'static,
        Fut: Future<Output = Result<HashMap<K, V>, QueryError>> + Send + 'static,
    {
        let runner = background.clone();
        let load: Load<K, V> = Rc::new(move |keys| {
            let running = runner.offload(load(keys));
            Box::pin(running)
        });
        Self {
            inner: Rc::new(Inner {
                background: background.clone(),
                load,
                pending: RefCell::new(Vec::new()),
                scheduled: Cell::new(false),
                batches: Cell::new(0),
            }),
        }
    }

    /// Asks for `key`'s value, loaded with every other key asked for before
    /// the batch goes out.
    #[must_use]
    pub fn load(&self, key: K) -> Loading<V> {
        let answer: Answer<V> = Rc::new(RefCell::new((None, None)));
        self.inner.pending.borrow_mut().push((key, Rc::clone(&answer)));
        if !self.inner.scheduled.replace(true) {
            let inner = Rc::clone(&self.inner);
            self.inner.background.spawn_local(async move {
                inner.scheduled.set(false);
                let batch = std::mem::take(&mut *inner.pending.borrow_mut());
                let mut keys = batch.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>();
                let mut seen = std::collections::HashSet::new();
                keys.retain(|key| seen.insert(key.clone()));
                inner.batches.set(inner.batches.get() + 1);
                let loaded = (inner.load)(keys).await;
                for (key, answer) in batch {
                    let result = match &loaded {
                        Ok(values) => values
                            .get(&key)
                            .cloned()
                            .ok_or_else(|| QueryError::fatal("the batch did not include this key")),
                        Err(error) => Err(error.clone()),
                    };
                    let waker = {
                        let mut answer = answer.borrow_mut();
                        answer.0 = Some(result);
                        answer.1.take()
                    };
                    if let Some(waker) = waker {
                        waker.wake();
                    }
                }
            });
        }
        Loading { answer }
    }

    /// How many batches have been sent.
    #[must_use]
    pub fn batches(&self) -> u64 {
        self.inner.batches.get()
    }
}
