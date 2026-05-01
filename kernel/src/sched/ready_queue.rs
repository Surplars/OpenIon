use super::task::{TaskControlBlock, Priority};

const MAX_TASKS: usize = 32;
const MAX_PRIORITIES: usize = 8;

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
                return Some(unsafe { &mut *task_ptr });
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
}

unsafe impl Send for Queue {}
unsafe impl Sync for Queue {}

