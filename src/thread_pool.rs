use std::{
    future::Future,
    mem,
    sync::{Arc, Mutex},
    thread::{self, Builder, JoinHandle},
};

use crossbeam_deque::Injector;

use crate::task::Task;
use crate::thread_worker::{WorkerCore, WorkerThread};

pub struct ThreadPool {
    core: Arc<WorkerCore>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    injector: Arc<Injector<Arc<Task>>>,
}

impl ThreadPool {
    pub fn new(thread_num: usize) -> Self {
        let mut threads = Vec::with_capacity(thread_num);
        let core = Arc::new(WorkerCore::new());
        let injector = Arc::new(Injector::new());
        for i in 0..thread_num {
            // TODO: support customizing some options, e.g, the stack size.
            let builder = Builder::new().name(format!("thread-{}", i));
            let worker_thread = WorkerThread::new(core.clone(), injector.clone());
            threads.push(builder.spawn(|| worker_thread.run()).unwrap())
        }
        ThreadPool {
            core,
            threads: Mutex::new(threads),
            injector,
        }
    }

    pub fn spawn(&self, future: impl Future<Output = ()> + Send + Sync + 'static) {
        self.injector.push(Task::new(future, false))
    }

    pub fn spawn_block(&self, future: impl Future<Output = ()> + Send + Sync + 'static) {
        self.injector.push(Task::new(future, true))
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        self.core.shutdown();
        let mut threads = mem::take(&mut *self.threads.lock().unwrap());
        let cur_tid = thread::current().id();
        for i in threads.drain(..) {
            if cur_tid != i.thread().id() {
                i.join().unwrap();
            }
        }
    }
}
