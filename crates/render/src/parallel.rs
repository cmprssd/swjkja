//! Data parallelism without a thread pool dependency: scoped threads, one per core,
//! taking items in order from a shared counter.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// `items.iter().map(f).collect()`, with the calls spread over the cores.
pub fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(items.len());
    if threads <= 1 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= items.len() {
                        break;
                    }
                    let r = f(&items[i]);
                    results.lock().unwrap()[i] = Some(r);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|r| r.unwrap())
        .collect()
}

/// Calls `f` on every item, spread over the cores (items move to the threads, so
/// they can be disjoint mutable slices).
pub fn each<T: Send>(items: Vec<T>, f: impl Fn(T) + Sync) {
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(items.len());
    let queue = Mutex::new(items.into_iter());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let Some(item) = queue.lock().unwrap().next() else {
                        break;
                    };
                    f(item);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_the_order_of_the_items() {
        let items: Vec<u32> = (0..1000).collect();
        assert_eq!(
            super::map(&items, |x| x * 2),
            (0..1000).map(|x| x * 2).collect::<Vec<_>>()
        );
        assert_eq!(super::map(&[] as &[u32], |x| *x), Vec::<u32>::new());
    }
}
