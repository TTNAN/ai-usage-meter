//! The one way this app takes a mutex.
//!
//! Every lock in the app goes through `lock`, so one thread's panic cannot
//! poison the rest.

use std::sync::{Mutex, MutexGuard};

/// Locks a mutex without letting one thread's panic spread through the app.
///
/// A poisoned mutex still holds whatever was in it, and for every value here
/// a slightly stale map or list is far better than a tray app that cannot
/// answer again: a poll is retried in minutes, but a poisoned `unwrap()` makes
/// every later caller panic too, including the ones on the UI thread.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
