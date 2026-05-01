pub mod ready_queue;
pub mod task;

use crate::mm::slab::Slab;
use core::sync::atomic::{AtomicU32, Ordering};
use ready_queue::ReadyQueue;
use crate::sync::Mutex;
use task::{Priority, TaskControlBlock, TaskId, TaskState};

static SCHEDULER: Mutex<Option<Scheduler>> = Mutex::new(None);

pub static TCB_POOL: Slab<TaskControlBlock, 32> = Slab::new();

// Accessed from assembly context-switch code via #[no_mangle] symbols.
// Safety: only written with IRQs disabled + SCHEDULER lock held.
#[unsafe(no_mangle)]
pub static mut CURRENT_TCB: *mut TaskControlBlock = core::ptr::null_mut();

#[unsafe(no_mangle)]
pub static mut NEXT_TCB: *mut TaskControlBlock = core::ptr::null_mut();

pub struct Scheduler {
    ready_queue: ReadyQueue,
    task_id_counter: AtomicU32,
    idle_task: Option<TaskId>,
    sleep_queue: *mut TaskControlBlock,
}

unsafe impl Send for Scheduler {}
unsafe impl Sync for Scheduler {}

impl Scheduler {
    pub fn init() {
        let mut sched = SCHEDULER.lock();
        *sched = Some(Scheduler {
            ready_queue: ReadyQueue::new(),
            task_id_counter: AtomicU32::new(0),
            idle_task: None,
            sleep_queue: core::ptr::null_mut(),
        });
    }

    pub fn init_system_tasks(root_entry: fn() -> !) {
        let idle_id = Self::create_task(
            idle_task_entry,
            unsafe { &mut *core::ptr::addr_of_mut!(IDLE_TASK_STACK) },
            0,
            "IDLE",
        );

        if let Some(sched) = SCHEDULER.lock().as_mut() {
            sched.idle_task = Some(idle_id);
        }

        Self::create_task(
            root_entry,
            unsafe { &mut *core::ptr::addr_of_mut!(ROOT_TASK_STACK) },
            1,
            "ROOT",
        );
    }

    pub fn create_task(
        entry: fn() -> !,
        stack: &'static mut [usize],
        priority: Priority,
        name: &'static str,
    ) -> TaskId {
        let entry_addr = entry as usize;
        let initial_sp = unsafe {
            (crate::arch::INIT_TASK_STACK_FN.expect("Arch not initialized"))(stack, entry_addr)
        };

        let tcb_val = TaskControlBlock::new(0, entry, initial_sp, stack.len() * core::mem::size_of::<usize>(), priority, name);
        let mut tcb_ptr = TCB_POOL.alloc(tcb_val).expect("No more TCB");

        crate::arch::disable_irq();
        let id = {
            let mut lock = SCHEDULER.lock();
            let sched = lock.as_mut().unwrap();

            let id = sched.task_id_counter.fetch_add(1, Ordering::Relaxed);
            unsafe {
                tcb_ptr.as_mut().id = id;
            }

            sched.ready_queue.push(unsafe { tcb_ptr.as_mut() });
            id
        };
        crate::arch::enable_irq();
        id
    }

    pub fn tick_update() {
        crate::arch::disable_irq();
        {
            let mut lock = SCHEDULER.lock();
            let sched = lock.as_mut().unwrap();
            let current_tick = crate::timer::ticks();

            let mut prev: *mut TaskControlBlock = core::ptr::null_mut();
            let mut curr = sched.sleep_queue;

            while !curr.is_null() {
                let task = unsafe { &mut *curr };
                let next = task.next;

                // Simple tick comparison, handles wrap-around assuming ticks are within an ok range
                if current_tick.wrapping_sub(task.wakeup_tick) < (u32::MAX / 2) {
                    // Wake up!
                    if prev.is_null() {
                        sched.sleep_queue = next;
                    } else {
                        unsafe {
                            (*prev).next = next;
                        }
                    }

                    task.state = TaskState::Ready;
                    task.next = core::ptr::null_mut();
                    sched.ready_queue.push(task);
                } else {
                    prev = curr;
                }

                curr = next;
            }
        }
        crate::arch::enable_irq();
    }

    pub fn schedule() -> bool {
        crate::arch::disable_irq();
        let ret = {
            let mut lock = SCHEDULER.lock();
            let sched = lock.as_mut().unwrap();

            let current = unsafe { CURRENT_TCB };
            if !current.is_null() {
                let task = unsafe { &mut *current };
                if task.state == TaskState::Running {
                    task.state = TaskState::Ready;
                    sched.ready_queue.push(task);
                }
            }

            if let Some(next) = sched.ready_queue.pop_highest() {
                next.state = TaskState::Running;
                let next_ptr = next as *mut _;
                if current != next_ptr {
                    unsafe {
                        NEXT_TCB = next_ptr;
                    }
                    true
                } else {
                    false
                }
        } else {
            // Ready queue empty: keep running the current task
            unsafe {
                NEXT_TCB = current;
            }
            false
        }
        };
        crate::arch::enable_irq();
        ret
    }

    pub fn delay(ticks: u32) {
        crate::arch::disable_irq();
        let wakeup_tick = crate::timer::ticks().wrapping_add(ticks);
        {
            let mut lock = SCHEDULER.lock();
            let sched = lock.as_mut().unwrap();

            let current = unsafe { CURRENT_TCB };
            if !current.is_null() {
                let task = unsafe { &mut *current };
                task.state = TaskState::Sleeping;
                task.wakeup_tick = wakeup_tick;

                task.next = sched.sleep_queue;
                sched.sleep_queue = current;
            }
        }
        crate::arch::enable_irq();

        unsafe {
            (crate::arch::YIELD_CPU_FN.expect("Arch not initialized"))();
        }
    }

    pub fn yield_task() {
        // Let trap handler do schedule to switch task
        unsafe {
            (crate::arch::YIELD_CPU_FN.expect("Arch not initialized"))();
        }
    }
}

static mut IDLE_TASK_STACK: [usize; 256] = [0; 256];
static mut ROOT_TASK_STACK: [usize; 1024] = [0; 1024];

fn idle_task_entry() -> ! {
    loop {
        unsafe {
            if let Some(yield_fn) = crate::arch::YIELD_CPU_FN {
                yield_fn();
            }
        }
    }
}
