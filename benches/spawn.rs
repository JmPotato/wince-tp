use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc,
};

use criterion::{criterion_group, criterion_main, Bencher, BenchmarkId, Criterion};
use wince_tp::ThreadPool;

pub fn spawn(b: &mut Bencher<'_>, spawn_count: usize) {
    let tp = ThreadPool::new(num_cpus::get());
    let (tx, rx) = mpsc::sync_channel(1000);
    let rem = Arc::new(AtomicUsize::new(0));

    b.iter(|| {
        rem.store(spawn_count, Ordering::Relaxed);

        for _ in 0..spawn_count {
            let tx = tx.clone();
            let rem = rem.clone();

            tp.spawn(async move {
                if 1 == rem.fetch_sub(1, Ordering::Relaxed) {
                    tx.send(()).unwrap();
                }
            });
        }

        let _ = rx.recv().unwrap();
    });
}

pub fn bench_spawn(b: &mut Criterion) {
    let mut group = b.benchmark_group("spawn");
    for i in &[1024, 4096, 8192, 16384] {
        group.bench_with_input(BenchmarkId::new("wince-tp", i), i, |b, i| spawn(b, *i));
    }
    group.finish();
}

criterion_group!(spawn_group, bench_spawn);
criterion_main!(spawn_group);
