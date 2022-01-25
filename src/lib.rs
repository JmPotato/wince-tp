mod task;
pub mod thread_pool;
mod thread_worker;

pub use thread_pool::ThreadPool;

#[cfg(test)]
mod test {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    };

    use crate::ThreadPool;

    const FUTURE_COUNT: usize = 100;

    #[test]
    fn test_many_future() {
        let tp = ThreadPool::new(num_cpus::get());
        let (done_tx, done_rx) = mpsc::sync_channel(1000);
        let rem = Arc::new(AtomicUsize::new(0));

        rem.store(FUTURE_COUNT, Ordering::Relaxed);

        for _ in 0..FUTURE_COUNT {
            let done_tx = done_tx.clone();
            let rem = rem.clone();

            tp.spawn(async move {
                if 1 == rem.fetch_sub(1, Ordering::Relaxed) {
                    done_tx.send(()).unwrap();
                }
            });
        }

        done_rx.recv().unwrap();
    }

    #[test]
    fn test_chained_future() {
        fn iter(tp: Arc<ThreadPool>, done_tx: mpsc::SyncSender<()>, n: usize) {
            if n == 0 {
                done_tx.send(()).unwrap();
            } else {
                tp.clone().spawn(async move {
                    iter(tp.clone(), done_tx, n - 1);
                });
            }
        }

        let tp = Arc::new(ThreadPool::new(num_cpus::get()));
        let (done_tx, done_rx) = mpsc::sync_channel(1000);

        let done_tx = done_tx.clone();
        tp.clone().spawn(async move {
            iter(tp.clone(), done_tx, FUTURE_COUNT);
        });

        done_rx.recv().unwrap();
    }
}
