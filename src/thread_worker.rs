use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::Context,
    thread::yield_now,
};

use crossbeam_deque::{Injector, Steal, Worker};

use crate::task::{Task, TaskWaker};

pub struct WorkerCore {
    is_shutdown: AtomicBool,
}

impl WorkerCore {
    pub fn new() -> Self {
        WorkerCore {
            is_shutdown: AtomicBool::new(false),
        }
    }

    pub fn is_shutdown(&self) -> bool {
        self.is_shutdown.load(Ordering::Relaxed)
    }

    pub fn shutdown(&self) {
        self.is_shutdown.store(true, Ordering::Relaxed);
    }
}

pub struct WorkerThread {
    core: Arc<WorkerCore>,
    local_queue: Worker<Arc<Task>>,
    injector: Arc<Injector<Arc<Task>>>,
    // TODO: support stealing tasks from other WorkerThreads.
}

impl WorkerThread {
    pub fn new(core: Arc<WorkerCore>, injector: Arc<Injector<Arc<Task>>>) -> Self {
        WorkerThread {
            core,
            local_queue: Worker::new_fifo(),
            injector,
        }
    }

    pub fn run(self) {
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
                task = match self.injector.steal_batch_and_pop(&self.local_queue) {
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
