//! Scheduler wait queues.
//!
//! This is the synchronous blocking primitive used by kernel services that
//! must keep working even when the optional async runtime is disabled.

use super::task::TaskControlBlock;
use crate::sync::Mutex;

#[derive(Clone, Copy)]
pub struct WaitQueueStats {
    pub waiting: usize,
    pub wakes: u64,
    pub failed_wakes: u64,
}

struct WaitQueueInner<const N: usize> {
    tasks: [*mut TaskControlBlock; N],
    len: usize,
    wakes: u64,
    failed_wakes: u64,
}

impl<const N: usize> WaitQueueInner<N> {
    const fn new() -> Self {
        Self {
            tasks: [core::ptr::null_mut(); N],
            len: 0,
            wakes: 0,
            failed_wakes: 0,
        }
    }

    fn contains(&self, task: *mut TaskControlBlock) -> bool {
        self.tasks[..self.len].contains(&task)
    }

    fn push(&mut self, task: *mut TaskControlBlock) -> bool {
        if task.is_null() || self.contains(task) || self.len >= N {
            return false;
        }
        self.tasks[self.len] = task;
        self.len += 1;
        true
    }

    fn pop_front(&mut self) -> Option<*mut TaskControlBlock> {
        if self.len == 0 {
            return None;
        }
        let task = self.tasks[0];
        let last = self.len - 1;
        for i in 0..last {
            self.tasks[i] = self.tasks[i + 1];
        }
        self.tasks[last] = core::ptr::null_mut();
        self.len = last;
        Some(task)
    }

    fn take_all(&mut self, out: &mut [*mut TaskControlBlock; N]) -> usize {
        let count = self.len;
        let mut i = 0;
        while i < count {
            out[i] = self.tasks[i];
            self.tasks[i] = core::ptr::null_mut();
            i += 1;
        }
        self.len = 0;
        count
    }
}

unsafe impl<const N: usize> Send for WaitQueueInner<N> {}
unsafe impl<const N: usize> Sync for WaitQueueInner<N> {}

pub struct WaitQueue<const N: usize> {
    inner: Mutex<WaitQueueInner<N>>,
}

impl<const N: usize> WaitQueue<N> {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(WaitQueueInner::new()),
        }
    }

    pub fn wait(&self) -> bool {
        let current = super::Scheduler::current_task_ptr();
        if current.is_null() {
            return false;
        }

        crate::arch::disable_irq();
        let queued_and_blocked = {
            let mut inner = self.inner.lock();
            if inner.push(current) {
                super::Scheduler::block_current_task_irq_disabled()
            } else {
                false
            }
        };
        crate::arch::enable_irq();

        if queued_and_blocked {
            super::Scheduler::yield_task();
            true
        } else {
            self.remove(current);
            false
        }
    }

    pub fn wake_one(&self) -> bool {
        let task = {
            let mut inner = self.inner.lock();
            inner.pop_front()
        };

        let Some(task) = task else {
            return false;
        };

        let woke = super::Scheduler::wake_blocked_task(task);
        let mut inner = self.inner.lock();
        if woke {
            inner.wakes = inner.wakes.wrapping_add(1);
        } else {
            inner.failed_wakes = inner.failed_wakes.wrapping_add(1);
        }
        woke
    }

    pub fn wake_all(&self) -> usize {
        let mut tasks = [core::ptr::null_mut(); N];
        let count = {
            let mut inner = self.inner.lock();
            inner.take_all(&mut tasks)
        };

        let mut woke_count = 0usize;
        let mut failed_count = 0usize;
        for task in tasks.iter().take(count) {
            if super::Scheduler::wake_blocked_task(*task) {
                woke_count += 1;
            } else {
                failed_count += 1;
            }
        }

        let mut inner = self.inner.lock();
        inner.wakes = inner.wakes.wrapping_add(woke_count as u64);
        inner.failed_wakes = inner.failed_wakes.wrapping_add(failed_count as u64);
        woke_count
    }

    pub fn stats(&self) -> WaitQueueStats {
        let inner = self.inner.lock();
        WaitQueueStats {
            waiting: inner.len,
            wakes: inner.wakes,
            failed_wakes: inner.failed_wakes,
        }
    }

    fn remove(&self, task: *mut TaskControlBlock) {
        let mut inner = self.inner.lock();
        let old_len = inner.len;
        let mut write = 0usize;
        for read in 0..old_len {
            if inner.tasks[read] != task {
                inner.tasks[write] = inner.tasks[read];
                write += 1;
            }
        }
        for slot in inner.tasks.iter_mut().take(old_len).skip(write) {
            *slot = core::ptr::null_mut();
        }
        inner.len = write;
    }
}

impl<const N: usize> Default for WaitQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}
