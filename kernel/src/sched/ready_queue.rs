use super::task::{Priority, TaskControlBlock};

const MAX_TASKS: usize = 32;
pub const MAX_PRIORITIES: usize = 8;
pub const MIN_PRIORITY: Priority = 0;
pub const MAX_PRIORITY: Priority = (MAX_PRIORITIES - 1) as Priority;

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
    current_priority: Priority,
}

impl ReadyQueue {
    pub const fn new() -> Self {
        Self {
            queues: [Queue::new(); MAX_PRIORITIES],
            task_count: 0,
            current_priority: 0,
        }
    }

    pub fn push(&mut self, task: &mut TaskControlBlock) -> bool {
        if task.queued {
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
        true
    }

    pub fn pop_highest(&mut self) -> Option<&mut TaskControlBlock> {
        for priority in (0..MAX_PRIORITIES).rev() {
            let q = &mut self.queues[priority];
            if !q.head.is_null() {
                let task_ptr = unsafe { q.pop_front() };
                self.task_count -= 1;
                self.current_priority = priority as Priority;
                let task = unsafe { &mut *task_ptr };
                task.queued = false;
                return Some(task);
            }
        }
        None
    }

    pub fn peek_highest_priority(&self) -> Priority {
        for priority in (0..MAX_PRIORITIES).rev() {
            if !self.queues[priority].head.is_null() {
                return priority as Priority;
            }
        }
        0
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
