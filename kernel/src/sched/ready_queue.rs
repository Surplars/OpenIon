use super::task::{Priority, TaskControlBlock, TaskState};

#[cfg(feature = "mcu_profile")]
const MAX_TASKS: usize = 8;
#[cfg(not(feature = "mcu_profile"))]
const MAX_TASKS: usize = crate::generated_config::OPENION_TASK_CAP;

/// Number of priority levels.
/// Levels 0-6: Normal priorities (0 = lowest, 6 = highest normal)
/// Level 7: Real-time priority (preempts all normal tasks)
pub const MAX_PRIORITIES: usize = 8;
pub const MIN_PRIORITY: Priority = 0;
pub const MAX_PRIORITY: Priority = (MAX_PRIORITIES - 1) as Priority;

/// Priority level at and above which tasks are considered real-time.
pub const REALTIME_PRIORITY_THRESHOLD: Priority = 7;

/// Check if a priority level is real-time.
pub const fn is_realtime_priority(prio: Priority) -> bool {
    prio >= REALTIME_PRIORITY_THRESHOLD
}

#[derive(Copy, Clone)]
struct Queue {
    head: *mut TaskControlBlock,
    tail: *mut TaskControlBlock,
}

impl Queue {
    const fn new() -> Self {
        Self {
            head: core::ptr::null_mut(),
            tail: core::ptr::null_mut(),
        }
    }

    unsafe fn push_back(&mut self, task: *mut TaskControlBlock) {
        unsafe {
            (*task).next = core::ptr::null_mut();
            if self.tail.is_null() {
                self.head = task;
                self.tail = task;
            } else {
                (*self.tail).next = task;
                self.tail = task;
            }
        }
    }

    unsafe fn pop_front(&mut self) -> *mut TaskControlBlock {
        unsafe {
            if self.head.is_null() {
                return core::ptr::null_mut();
            }
            let task = self.head;
            self.head = (*task).next;
            if self.head.is_null() {
                self.tail = core::ptr::null_mut();
            }
            (*task).next = core::ptr::null_mut();
            task
        }
    }
}

pub struct ReadyQueue {
    queues: [Queue; MAX_PRIORITIES],
    task_count: usize,
    non_empty: u8,
    current_priority: Priority,
}

impl ReadyQueue {
    pub const fn new() -> Self {
        Self {
            queues: [Queue::new(); MAX_PRIORITIES],
            task_count: 0,
            non_empty: 0,
            current_priority: 0,
        }
    }

    pub fn push(&mut self, task: &mut TaskControlBlock) -> bool {
        if task.queued {
            return false;
        }

        if task.state != TaskState::Ready || !task.next.is_null() {
            return false;
        }

        if self.task_count >= MAX_TASKS {
            return false;
        }

        let priority = task.priority as usize;
        if priority >= MAX_PRIORITIES {
            return false;
        }

        unsafe {
            self.queues[priority].push_back(task as *mut _);
        }
        task.queued = true;

        self.task_count += 1;
        self.non_empty |= 1u8 << priority;
        true
    }

    pub fn pop_highest(&mut self) -> Option<&mut TaskControlBlock> {
        if self.non_empty == 0 {
            return None;
        }

        let priority = highest_priority(self.non_empty);
        let q = &mut self.queues[priority];
        let task_ptr = unsafe { q.pop_front() };
        if q.head.is_null() {
            self.non_empty &= !(1u8 << priority);
        }
        self.task_count -= 1;
        self.current_priority = priority as Priority;
        let task = unsafe { &mut *task_ptr };
        task.queued = false;
        Some(task)
    }

    /// Pop the lowest priority task (for work-stealing).
    /// Stealers take low-priority work to avoid disrupting the victim's high-priority tasks.
    pub fn pop_lowest(&mut self) -> Option<&mut TaskControlBlock> {
        if self.non_empty == 0 {
            return None;
        }

        let priority = lowest_priority(self.non_empty);
        let q = &mut self.queues[priority];
        let task_ptr = unsafe { q.pop_front() };
        if q.head.is_null() {
            self.non_empty &= !(1u8 << priority);
        }
        self.task_count -= 1;
        let task = unsafe { &mut *task_ptr };
        task.queued = false;
        Some(task)
    }

    /// Pop lowest priority task returning raw pointer (for work-stealing with split borrows).
    pub fn pop_lowest_raw(&mut self) -> *mut TaskControlBlock {
        if self.non_empty == 0 {
            return core::ptr::null_mut();
        }

        let priority = lowest_priority(self.non_empty);
        let q = &mut self.queues[priority];
        let task_ptr = unsafe { q.pop_front() };
        if q.head.is_null() {
            self.non_empty &= !(1u8 << priority);
        }
        self.task_count -= 1;
        if !task_ptr.is_null() {
            unsafe {
                (*task_ptr).queued = false;
            }
        }
        task_ptr
    }

    /// Push a task using raw pointer (for work-stealing with split borrows).
    /// Safety: task_ptr must point to a valid TaskControlBlock.
    pub unsafe fn push_raw(&mut self, task_ptr: *mut TaskControlBlock) -> bool {
        if task_ptr.is_null() {
            return false;
        }
        let task = unsafe { &mut *task_ptr };
        self.push(task)
    }

    pub fn peek_highest_priority(&self) -> Priority {
        if self.non_empty == 0 {
            0
        } else {
            highest_priority(self.non_empty) as Priority
        }
    }

    pub fn is_empty(&self) -> bool {
        self.task_count == 0
    }

    pub fn len(&self) -> usize {
        self.task_count
    }

    pub fn for_each(&self, mut f: impl FnMut(&TaskControlBlock)) {
        for queue in self.queues.iter() {
            let mut curr = queue.head;
            while !curr.is_null() {
                let task = unsafe { &*curr };
                f(task);
                curr = task.next;
            }
        }
    }
}

unsafe impl Send for Queue {}
unsafe impl Sync for Queue {}

const fn highest_priority(mask: u8) -> usize {
    (u8::BITS - 1 - mask.leading_zeros()) as usize
}

const fn lowest_priority(mask: u8) -> usize {
    mask.trailing_zeros() as usize
}
