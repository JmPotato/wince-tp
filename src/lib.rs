use std::{
    future::Future,
    mem,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Wake},
    thread::{self, yield_now, Builder, JoinHandle},
};

use crossbeam_deque::{Injector, Steal, Worker};

struct Task {
    future: Mutex<Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>>,
    will_block: bool,
}

impl Task {
    fn new(
        future: impl Future<Output = ()> + Send + Sync + 'static,
        will_block: bool,
    ) -> Arc<Self> {
        Arc::new(Task {
            future: Mutex::new(Box::pin(future)),
            will_block,
        })
    }

    fn will_block(&self) -> bool {
        self.will_block
    }

    fn poll(self: &Arc<Self>, cx: &mut Context) -> Poll<()> {
        self.future.lock().unwrap().as_mut().poll(cx)
    }
}

struct TaskWaker {
    arc_task: Arc<Task>,
    // injector is used to wake the task by injecting it to the global queue.
    injector: Arc<Injector<Arc<Task>>>,
}

impl TaskWaker {
    fn new(arc_task: Arc<Task>, injector: Arc<Injector<Arc<Task>>>) -> Self {
        TaskWaker { arc_task, injector }
    }
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.injector.push(self.arc_task.clone());
    }
}

struct WorkerCore {
    is_shutdown: AtomicBool,
}

impl WorkerCore {
    fn new() -> Self {
        WorkerCore {
            is_shutdown: AtomicBool::new(false),
        }
    }

    fn is_shutdown(&self) -> bool {
        self.is_shutdown.load(Ordering::Relaxed)
    }

    fn shutdown(&self) {
        self.is_shutdown.store(true, Ordering::Relaxed);
    }
}

struct WorkerThread {
    core: Arc<WorkerCore>,
    local_queue: Worker<Arc<Task>>,
    injector: Arc<Injector<Arc<Task>>>,
    // TODO: support stealing tasks from other WorkerThreads.
}

impl WorkerThread {
    fn new(core: Arc<WorkerCore>, injector: Arc<Injector<Arc<Task>>>) -> Self {
        WorkerThread {
            core,
            local_queue: Worker::new_fifo(),
            injector,
        }
    }

    fn run(self) {
        loop {
            if self.core.is_shutdown() {
                return;
            }
            let mut task = self.local_queue.pop();
            if task.is_none() {
                // Just retry if the local queue is not empty.
                if !self.local_queue.is_empty() {
                    continue;
                }
                task = match self.injector.steal() {
                    Steal::Success(task) => Some(task),
                    Steal::Empty => {
                        // TODO: park the thread and wake it up once there is a task to run.
                        yield_now();
                        continue;
                    }
                    Steal::Retry => continue,
                };
            }
            let arc_task = task.unwrap();
            let waker = Arc::new(TaskWaker::new(arc_task.clone(), self.injector.clone())).into();
            let mut cx = Context::from_waker(&waker);
            if arc_task.will_block() {
                while arc_task.poll(&mut cx).is_pending() {
                    yield_now();
                }
            } else if arc_task.poll(&mut cx).is_pending() {
                waker.wake();
            }
        }
    }
}

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

#[cfg(test)]
mod test {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    };

    use super::ThreadPool;

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
