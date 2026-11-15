//! Work spread over threads with `std::thread::scope`, with results in the order of the input, so that the result
//! never depends on how many threads ran or which finished first.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Most threads used when the caller leaves the choice to this crate.
pub const MAX_THREADS: usize = 16;

/// The number of threads to use for `requested` (0: one per available core, at most [`MAX_THREADS`]).
#[must_use]
pub fn threads(requested: usize) -> usize {
    if requested > 0 {
        return requested;
    }
    std::thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(MAX_THREADS)
}

/// `f` applied to every item, on up to `threads` threads; the results are in the order of `items`.
///
/// Items are handed out one at a time from a shared counter, so a slow item does not hold up the others. With one
/// thread, or one item, nothing is spawned.
///
/// # Panics
/// When `f` panics on a thread, that panic is resumed on the caller's thread.
pub fn map<T, R, F>(items: &[T], threads: usize, f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let threads = threads.clamp(1, items.len().max(1));
    if threads == 1 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let mut parts: Vec<Vec<(usize, R)>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            return done;
                        };
                        done.push((index, f(item)));
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| match worker.join() {
                Ok(done) => done,
                Err(panic) => std::panic::resume_unwind(panic),
            })
            .collect()
    });
    let mut slots: Vec<Option<R>> = std::iter::repeat_with(|| None).take(items.len()).collect();
    for (index, result) in parts.iter_mut().flat_map(std::mem::take) {
        slots[index] = Some(result);
    }
    slots
        .into_iter()
        .map(|slot| slot.expect("every index is handed out exactly once"))
        .collect()
}

/// A flag a caller sets to stop a sort early (see `api::SortOptions::with_cancel`). Clones share the flag.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A flag that is not set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask every sort that holds this token (or a clone) to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// True once [`CancelToken::cancel`] was called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

impl From<Arc<AtomicBool>> for CancelToken {
    fn from(flag: Arc<AtomicBool>) -> Self {
        Self(flag)
    }
}

impl std::fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CancelToken")
            .field(&self.is_cancelled())
            .finish()
    }
}

/// Two tokens are equal when they share the same flag.
impl PartialEq for CancelToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CancelToken {}

/// True when `cancel` is set.
#[must_use]
pub fn cancelled(cancel: Option<&CancelToken>) -> bool {
    cancel.is_some_and(CancelToken::is_cancelled)
}

/// A count shared by threads that must stay at or below a limit, taken in chunks so that the threads rarely touch
/// it. With a chunk of 1 it is exact: an item is refused exactly when it would pass the limit.
#[derive(Debug)]
pub struct SharedBudget {
    used: AtomicUsize,
    limit: usize,
    chunk: usize,
}

impl SharedBudget {
    /// A budget of `limit` items, reserved `chunk` at a time.
    #[must_use]
    pub fn new(limit: usize, chunk: usize) -> Self {
        Self {
            used: AtomicUsize::new(0),
            limit,
            chunk: chunk.max(1),
        }
    }

    /// Items taken so far (reservations included).
    #[must_use]
    pub fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    /// A reservation for one user of the budget.
    #[must_use]
    pub const fn holder(&self) -> Holder<'_> {
        Holder {
            budget: self,
            reserved: 0,
            count: 0,
        }
    }
}

/// What one reader has taken from a [`SharedBudget`]: it counts items and reserves a chunk whenever it runs out.
/// Dropping it gives back everything it reserved; [`Holder::keep`] keeps `kept` items and gives back the rest.
#[derive(Debug)]
pub struct Holder<'a> {
    budget: &'a SharedBudget,
    reserved: usize,
    count: usize,
}

impl Holder<'_> {
    /// Take one more item; false when the reservation it needs passes the limit (nothing is taken then).
    ///
    /// With a chunk of 1 this is exact. With a larger chunk it can refuse up to a chunk per holder early, so a
    /// caller that gets a refusal from a chunked budget repeats the work with a chunk of 1 to get the exact answer.
    pub fn take(&mut self) -> bool {
        if self.count == self.reserved {
            let chunk = self.budget.chunk;
            let before = self.budget.used.fetch_add(chunk, Ordering::Relaxed);
            if before.saturating_add(chunk) > self.budget.limit {
                self.budget.used.fetch_sub(chunk, Ordering::Relaxed);
                return false;
            }
            self.reserved += chunk;
        }
        self.count += 1;
        true
    }

    /// Items taken by this holder.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.count
    }

    /// Keep `kept` items of the budget for good (at most what was taken) and give back the rest.
    pub fn keep(mut self, kept: usize) {
        let kept = kept.min(self.reserved);
        self.budget
            .used
            .fetch_sub(self.reserved - kept, Ordering::Relaxed);
        self.reserved = 0;
    }
}

impl Drop for Holder<'_> {
    fn drop(&mut self) {
        if self.reserved > 0 {
            self.budget.used.fetch_sub(self.reserved, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
#[path = "parallel_tests.rs"]
mod tests;
