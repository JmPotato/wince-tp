use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake},
};

use crossbeam_deque::Injector;

pub struct Task {
    future: Mutex<Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>>,
    will_block: bool,
}

impl Task {
    pub fn new(
        future: impl Future<Output = ()> + Send + Sync + 'static,
        will_block: bool,
    ) -> Arc<Self> {
        Arc::new(Task {
            future: Mutex::new(Box::pin(future)),
            will_block,
        })
    }

    pub fn will_block(&self) -> bool {
        self.will_block
    }

    pub fn poll(self: &Arc<Self>, cx: &mut Context) -> Poll<()> {
        self.future.lock().unwrap().as_mut().poll(cx)
    }
}

pub struct TaskWaker {
    arc_task: Arc<Task>,
    // injector is used to wake the task by injecting it to the global queue.
    injector: Arc<Injector<Arc<Task>>>,
}

impl TaskWaker {
    pub fn new(arc_task: Arc<Task>, injector: Arc<Injector<Arc<Task>>>) -> Self {
        TaskWaker { arc_task, injector }
    }
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.injector.push(self.arc_task.clone());
    }
}
