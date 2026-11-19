//! Work the page can name by id (runs, plans, scans, analyses, estimates, keyframe requests):
//! one generic map instead of a `Mutex<HashMap>` and its insert/remove/cancel code per feature
//! (review item 18).

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use tokio_util::sync::CancellationToken;

/// Values by id. A poisoned lock is used as it is: every value is valid on its own.
#[derive(Debug)]
pub(crate) struct Registry<T> {
    map: Mutex<HashMap<String, T>>,
}

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Registry { map: Mutex::new(HashMap::new()) }
    }
}

impl<T> Registry<T> {
    /// The map, for a check and a change made together (keep the guard short).
    pub(crate) fn lock(&self) -> MutexGuard<'_, HashMap<String, T>> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn insert(&self, id: &str, value: T) {
        self.lock().insert(id.to_owned(), value);
    }

    pub(crate) fn remove(&self, id: &str) -> Option<T> {
        self.lock().remove(id)
    }

    /// `f` on the value of `id`, if there is one.
    pub(crate) fn with<R>(&self, id: &str, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.lock().get(id).map(f)
    }

    pub(crate) fn for_each(&self, f: impl FnMut(&T)) {
        self.lock().values().for_each(f);
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().len()
    }
}

/// Cancellable work: one token per id.
pub(crate) type Tasks = Registry<CancellationToken>;

/// Removes its id from the registry when dropped: however the work ends, also when the
/// command's future is dropped half-way.
#[derive(Debug)]
pub(crate) struct Registered<'a> {
    tasks: &'a Tasks,
    id: String,
    pub(crate) cancel: CancellationToken,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        let mut map = self.tasks.lock();
        // Only our own entry (a later request may reuse the id once this one is gone).
        if map.get(&self.id).is_some_and(|c| *c == self.cancel) {
            map.remove(&self.id);
        }
    }
}

/// What to do with the other tasks when one starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Others {
    Keep,
    /// Only the latest is wanted (analyses, estimates).
    Cancel,
}

impl Tasks {
    /// Registers a new task under `id`; `Err(())` when one with that id is already running.
    pub(crate) fn start(&self, id: &str, others: Others) -> Result<Registered<'_>, ()> {
        self.start_in(&mut self.lock(), id, others)
    }

    /// [`Tasks::start`] with the map already locked (`map` must be this registry's).
    pub(crate) fn start_in(
        &self,
        map: &mut HashMap<String, CancellationToken>,
        id: &str,
        others: Others,
    ) -> Result<Registered<'_>, ()> {
        if map.contains_key(id) {
            return Err(());
        }
        if others == Others::Cancel {
            map.values().for_each(CancellationToken::cancel);
        }
        let cancel = CancellationToken::new();
        map.insert(id.to_owned(), cancel.clone());
        Ok(Registered { tasks: self, id: id.to_owned(), cancel })
    }

    pub(crate) fn cancel(&self, id: &str) {
        self.with(id, CancellationToken::cancel);
    }

    pub(crate) fn cancel_all(&self) {
        self.for_each(CancellationToken::cancel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_runs_once_and_is_freed_when_its_work_ends() {
        let tasks = Tasks::default();
        let a = tasks.start("a", Others::Keep).unwrap();
        assert!(tasks.start("a", Others::Keep).is_err(), "already running");
        tasks.cancel("a");
        assert!(a.cancel.is_cancelled());
        drop(a);
        assert_eq!(tasks.len(), 0);
        assert!(tasks.start("a", Others::Keep).is_ok(), "free again");
    }

    #[test]
    fn starting_the_latest_cancels_the_others() {
        let tasks = Tasks::default();
        let a = tasks.start("a", Others::Keep).unwrap();
        let b = tasks.start("b", Others::Cancel).unwrap();
        assert!(a.cancel.is_cancelled() && !b.cancel.is_cancelled());
        tasks.cancel_all();
        assert!(b.cancel.is_cancelled());
    }

    #[test]
    fn a_dropped_guard_removes_only_its_own_entry() {
        let tasks = Tasks::default();
        let a = tasks.start("a", Others::Keep).unwrap();
        // Replaced behind the guard's back (cannot happen through `start`, but stay safe).
        tasks.insert("a", CancellationToken::new());
        drop(a);
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn a_poisoned_lock_is_still_usable() {
        let reg: Registry<u32> = Registry::default();
        reg.insert("x", 1);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = reg.lock();
            panic!("poison");
        }));
        assert_eq!(reg.with("x", |v| *v), Some(1));
        assert_eq!(reg.remove("x"), Some(1));
        assert!(reg.is_empty());
    }
}
