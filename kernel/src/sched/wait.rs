//! Scheduler wait queues.
//!
//! This is the synchronous blocking primitive used by kernel services that
//! must keep working even when the optional async runtime is disabled.

use super::task::TaskControlBlock;
use crate::sync::Mutex;
#[cfg(feature = "async_rt")]
use core::future::Future;
#[cfg(feature = "async_rt")]
use core::pin::Pin;
#[cfg(feature = "async_rt")]
use core::task::{Context, Poll};

#[cfg(feature = "async_rt")]
const NO_TASK: usize = usize::MAX;
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

    fn remove(&mut self, task: *mut TaskControlBlock) {
        let old_len = self.len;
        let mut write = 0usize;
        for read in 0..old_len {
            if self.tasks[read] != task {
                self.tasks[write] = self.tasks[read];
                write += 1;
            }
        }
        for slot in self.tasks.iter_mut().take(old_len).skip(write) {
            *slot = core::ptr::null_mut();
        }
        self.len = write;
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
        let queued = {
            let mut inner = self.inner.lock();
            inner.push(current)
        };
        let blocked = if queued {
            super::Scheduler::block_current_task_irq_disabled()
        } else {
            false
        };
        crate::arch::enable_irq();

        if blocked {
            super::Scheduler::yield_task();
            true
        } else {
            if queued {
                self.remove_task(current);
            }
            false
        }
    }

    pub(crate) fn enqueue_current(&self) -> bool {
        let current = super::Scheduler::current_task_ptr();
        if current.is_null() {
            return false;
        }
        let mut inner = self.inner.lock();
        inner.push(current)
    }

    pub(crate) fn remove_task(&self, task: *mut TaskControlBlock) {
        let mut inner = self.inner.lock();
        inner.remove(task);
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
}

impl<const N: usize> Default for WaitQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EventStats {
    pub signaled: bool,
    pub waiting: usize,
}

struct EventInner<const N: usize> {
    signaled: bool,
    waiters: WaitQueue<N>,
    #[cfg(feature = "async_rt")]
    async_waiter: usize,
}

impl<const N: usize> EventInner<N> {
    const fn new() -> Self {
        Self {
            signaled: false,
            waiters: WaitQueue::new(),
            #[cfg(feature = "async_rt")]
            async_waiter: NO_TASK,
        }
    }
}

pub struct Event<const N: usize> {
    inner: Mutex<EventInner<N>>,
}

impl<const N: usize> Event<N> {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(EventInner::new()),
        }
    }

    pub fn wait(&self) -> bool {
        let current = super::Scheduler::current_task_ptr();
        if current.is_null() {
            return false;
        }

        crate::arch::disable_irq();
        let wait_result = {
            let mut inner = self.inner.lock();
            if inner.signaled {
                inner.signaled = false;
                Ok(false)
            } else if inner.waiters.enqueue_current() {
                Ok(true)
            } else {
                Err(())
            }
        };

        let blocked = match wait_result {
            Ok(true) => super::Scheduler::block_current_task_irq_disabled(),
            Ok(false) => false,
            Err(()) => false,
        };
        crate::arch::enable_irq();

        if blocked {
            super::Scheduler::yield_task();
            true
        } else if matches!(wait_result, Ok(true)) {
            let inner = self.inner.lock();
            inner.waiters.remove_task(current);
            false
        } else {
            matches!(wait_result, Ok(false))
        }
    }

    pub fn signal(&self) -> bool {
        #[cfg(feature = "async_rt")]
        {
            let mut inner = self.inner.lock();
            if inner.waiters.wake_one() {
                return true;
            }
            if inner.async_waiter != NO_TASK {
                let task = inner.async_waiter;
                inner.async_waiter = NO_TASK;
                inner.signaled = true;
                crate::sched::async_rt::wake_task(task);
                return true;
            }
            inner.signaled = true;
            false
        }

        #[cfg(not(feature = "async_rt"))]
        {
            let mut inner = self.inner.lock();
            if inner.waiters.wake_one() {
                return true;
            }
            inner.signaled = true;
            false
        }
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.signaled = false;
    }

    pub fn is_signaled(&self) -> bool {
        let inner = self.inner.lock();
        inner.signaled
    }

    pub fn stats(&self) -> EventStats {
        let inner = self.inner.lock();
        EventStats {
            signaled: inner.signaled,
            waiting: inner.waiters.stats().waiting,
        }
    }

    #[cfg(feature = "async_rt")]
    pub fn wait_async(&self) -> EventWait<'_, N> {
        EventWait {
            event: self,
            waiter: NO_TASK,
        }
    }
}

impl<const N: usize> Default for Event<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "async_rt")]
pub struct EventWait<'a, const N: usize> {
    event: &'a Event<N>,
    waiter: usize,
}

#[cfg(feature = "async_rt")]
impl<const N: usize> Future for EventWait<'_, N> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let current = crate::sched::async_rt::current_poll_task();
        if current == NO_TASK {
            return Poll::Pending;
        }

        crate::arch::disable_irq();
        let ready = {
            let mut inner = self.event.inner.lock();
            if inner.signaled {
                inner.signaled = false;
                if inner.async_waiter == current {
                    inner.async_waiter = NO_TASK;
                }
                true
            } else {
                if inner.async_waiter == NO_TASK || inner.async_waiter == current {
                    inner.async_waiter = current;
                    self.waiter = current;
                }
                false
            }
        };
        crate::arch::enable_irq();

        if ready {
            self.waiter = NO_TASK;
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[cfg(feature = "async_rt")]
impl<const N: usize> Drop for EventWait<'_, N> {
    fn drop(&mut self) {
        if self.waiter == NO_TASK {
            return;
        }
        let mut inner = self.event.inner.lock();
        if inner.async_waiter == self.waiter {
            inner.async_waiter = NO_TASK;
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SemaphoreStats {
    pub permits: usize,
    pub waiting: usize,
}

struct SemaphoreInner<const N: usize> {
    permits: usize,
    waiters: WaitQueue<N>,
}

impl<const N: usize> SemaphoreInner<N> {
    const fn new(permits: usize) -> Self {
        Self {
            permits,
            waiters: WaitQueue::new(),
        }
    }
}

pub struct Semaphore<const N: usize> {
    inner: Mutex<SemaphoreInner<N>>,
}

impl<const N: usize> Semaphore<N> {
    pub const fn new(permits: usize) -> Self {
        Self {
            inner: Mutex::new(SemaphoreInner::new(permits)),
        }
    }

    pub fn try_acquire(&self) -> bool {
        let mut inner = self.inner.lock();
        if inner.permits > 0 {
            inner.permits -= 1;
            true
        } else {
            false
        }
    }

    pub fn wait(&self) -> bool {
        let current = super::Scheduler::current_task_ptr();
        if current.is_null() {
            return false;
        }

        crate::arch::disable_irq();
        let wait_result = {
            let mut inner = self.inner.lock();
            if inner.permits > 0 {
                inner.permits -= 1;
                Ok(false)
            } else if inner.waiters.enqueue_current() {
                Ok(true)
            } else {
                Err(())
            }
        };

        let blocked = match wait_result {
            Ok(true) => super::Scheduler::block_current_task_irq_disabled(),
            Ok(false) => false,
            Err(()) => false,
        };
        crate::arch::enable_irq();

        if blocked {
            super::Scheduler::yield_task();
            true
        } else if matches!(wait_result, Ok(true)) {
            let inner = self.inner.lock();
            inner.waiters.remove_task(current);
            false
        } else {
            matches!(wait_result, Ok(false))
        }
    }

    pub fn release(&self) -> usize {
        self.release_many(1)
    }

    pub fn release_many(&self, count: usize) -> usize {
        if count == 0 {
            return 0;
        }

        let mut woke = 0usize;
        for _ in 0..count {
            let did_wake = {
                let inner = self.inner.lock();
                inner.waiters.wake_one()
            };
            if did_wake {
                woke += 1;
            } else {
                let mut inner = self.inner.lock();
                inner.permits = inner.permits.saturating_add(count - woke);
                break;
            }
        }
        woke
    }

    pub fn available(&self) -> usize {
        let inner = self.inner.lock();
        inner.permits
    }

    pub fn stats(&self) -> SemaphoreStats {
        let inner = self.inner.lock();
        SemaphoreStats {
            permits: inner.permits,
            waiting: inner.waiters.stats().waiting,
        }
    }
}

impl<const N: usize> Default for Semaphore<N> {
    fn default() -> Self {
        Self::new(0)
    }
}
