//! A scoped worker pool: map a function over a slice on several threads, results in input order.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Every core, or 1 when the count is unknown.
pub fn default_jobs() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// Map `f` over `items` on `jobs` threads; results in input order, whatever the thread count.
/// Workers get a large stack: the parser and the tree walks recurse on deeply nested code.
pub fn par_map<T: Sync, R: Send>(items: &[T], jobs: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let jobs = jobs.max(1).min(items.len().max(1));
    if jobs == 1 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let mut all: Vec<(usize, R)> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..jobs)
            .map(|_| {
                std::thread::Builder::new()
                    .stack_size(256 << 20)
                    .spawn_scoped(s, || {
                        let mut mine = Vec::new();
                        loop {
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            let Some(item) = items.get(i) else { break };
                            mine.push((i, f(item)));
                        }
                        mine
                    })
                    .expect("spawn a worker thread")
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a worker thread panicked"))
            .collect()
    });
    all.sort_by_key(|(i, _)| *i);
    all.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_input_order() {
        let v: Vec<u32> = (0..1000).collect();
        assert_eq!(par_map(&v, 7, |x| x * 2), par_map(&v, 1, |x| x * 2));
    }
}
