use std::sync::{mpsc, Arc};

use criterion::{criterion_group, criterion_main, Bencher, BenchmarkId, Criterion};
use wince_tp::ThreadPool;

pub fn spawn_chain(b: &mut Bencher, iter_count: usize) {
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

    b.iter(|| {
        let inside_tp = tp.clone();
        let done_tx = done_tx.clone();
        inside_tp.clone().spawn(async move {
            iter(inside_tp, done_tx, iter_count);
        });

        done_rx.recv().unwrap();
    });
}

pub fn bench_spawn_chain(b: &mut Criterion) {
    let mut group = b.benchmark_group("spawn_chain");
    for i in &[1024, 4096, 8192, 16384] {
        group.bench_with_input(BenchmarkId::new("wince-tp", i), i, |b, i| {
            spawn_chain(b, *i)
        });
    }
    group.finish();
}

criterion_group!(spawn_chain_group, bench_spawn_chain);
criterion_main!(spawn_chain_group);
