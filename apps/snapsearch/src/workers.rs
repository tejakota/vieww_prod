//! How many background threads the app runs, and where its cache lives.
//!
//! `Threads` starts a thread per task: a 2000-photo scan was 2000 threads,
//! each holding a decode buffer, on a phone with 2–3 GB of RAM. A bounded
//! [`Pool`] keeps decode memory proportional to the pool, not the library.

use std::sync::Arc;

use vieww::foundation::task::{Pool, Spawn};
use vieww::foundation::DeviceTier;

/// Workers for a device: few on a small phone (memory and thermals), more
/// on a big one, never all the cores (the UI thread needs one).
#[must_use]
pub const fn pool_threads(tier: DeviceTier, cores: usize) -> usize {
    let cores = if cores == 0 { 1 } else { cores };
    match tier {
        DeviceTier::Low => 2,
        DeviceTier::Medium => 3,
        DeviceTier::High => {
            if cores > 1 { cores - 1 } else { 1 }
        }
    }
}

/// The pool this device should use.
#[must_use]
pub fn spawner_for(tier: DeviceTier, cores: usize) -> Arc<dyn Spawn> {
    Arc::new(Pool::with_threads(pool_threads(tier, cores)))
}

/// Where the Android app keeps its cache, supplied by `android_main` before
/// the tree is built (`build_root` does not receive the `AndroidApp`).
static ANDROID_DATA_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Record the app's private data directory. First call wins.
pub fn set_android_data_dir(dir: std::path::PathBuf) {
    let _ = ANDROID_DATA_DIR.set(dir);
}

/// The directory recorded by [`set_android_data_dir`], if any.
#[must_use]
pub fn android_data_dir() -> Option<&'static std::path::Path> {
    ANDROID_DATA_DIR.get().map(std::path::PathBuf::as_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pool_is_bounded_by_tier_not_by_library_size() {
        assert_eq!(pool_threads(DeviceTier::Low, 8), 2);
        assert_eq!(pool_threads(DeviceTier::Medium, 8), 3);
        assert_eq!(pool_threads(DeviceTier::High, 8), 7);
    }

    #[test]
    fn a_pool_is_never_empty_or_larger_than_the_machine() {
        assert_eq!(pool_threads(DeviceTier::High, 0), 1);
        assert_eq!(pool_threads(DeviceTier::High, 1), 1);
        assert_eq!(pool_threads(DeviceTier::High, 2), 1);
    }

    #[test]
    fn two_thousand_tasks_use_only_the_pools_threads() {
        use std::collections::HashSet;
        use std::sync::Mutex;
        let ids = Arc::new(Mutex::new(HashSet::new()));
        let (tx, rx) = std::sync::mpsc::channel();
        let pool = spawner_for(DeviceTier::Low, 8);
        for _ in 0..2000 {
            let (ids, tx) = (ids.clone(), tx.clone());
            pool.spawn(Box::new(move || {
                ids.lock().unwrap().insert(std::thread::current().id());
                let _ = tx.send(());
            }));
        }
        for _ in 0..2000 {
            rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap();
        }
        assert!(ids.lock().unwrap().len() <= 2, "a pool of two ran on more threads");
    }
}
