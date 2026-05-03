pub mod ready_queue;
pub mod task;

use crate::mm::slab::Slab;
use crate::sync::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};
use ready_queue::{MAX_PRIORITY, ReadyQueue};
use task::{Priority, TaskControlBlock, TaskId, TaskState};

static SCHEDULER: Mutex<Option<Scheduler>> = Mutex::new(None);

pub static TCB_POOL: Slab<TaskControlBlock, 32> = Slab::new();
pub const TASK_SNAPSHOT_CAP: usize = 32;

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
    preempt_pending: bool,
    context_switches: u64,
    preemptions: u64,
}

unsafe impl Send for Scheduler {}
unsafe impl Sync for Scheduler {}

#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub id: TaskId,
    pub name: &'static str,
    pub priority: Priority,
    pub state: TaskState,
    pub stack_size: usize,
    pub wakeup_tick: u32,
    pub queued: bool,
    pub current: bool,
}

#[derive(Clone, Copy)]
pub struct SchedulerStats {
    pub ready_tasks: usize,
    pub highest_ready_priority: Priority,
    pub current_task: Option<TaskInfo>,
    pub context_switches: u64,
    pub preemptions: u64,
    pub preempt_pending: bool,
}

impl Scheduler {
    pub fn init() {
        let mut sched = SCHEDULER.lock();
        *sched = Some(Scheduler {
            ready_queue: ReadyQueue::new(),
            task_id_counter: AtomicU32::new(0),
            idle_task: None,
            sleep_queue: core::ptr::null_mut(),
            preempt_pending: false,
            context_switches: 0,
            preemptions: 0,
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
        let priority = priority.min(MAX_PRIORITY);
        let entry_addr = entry as usize;
        let initial_sp = unsafe {
            (crate::arch::INIT_TASK_STACK_FN.expect("Arch not initialized"))(stack, entry_addr)
        };

        let tcb_val = TaskControlBlock::new(
            0,
            entry,
            initial_sp,
            stack.len() * core::mem::size_of::<usize>(),
            priority,
            name,
        );
        let mut tcb_ptr = TCB_POOL.alloc(tcb_val).expect("No more TCB");

        crate::arch::disable_irq();
        let id = {
            let mut lock = SCHEDULER.lock();
            if let Some(sched) = lock.as_mut() {
                let id = sched.task_id_counter.fetch_add(1, Ordering::Relaxed);
                unsafe {
                    tcb_ptr.as_mut().id = id;
                }

                if sched.ready_queue.push(unsafe { tcb_ptr.as_mut() }) {
                    sched.request_preempt_for(priority);
                }
                id
            } else {
                u32::MAX
            }
        };
        crate::arch::enable_irq();
        if id != u32::MAX {
            Self::yield_if_preempt_pending();
        }
        id
    }

    pub fn tick_update() {
        crate::arch::disable_irq();
        {
            let mut lock = SCHEDULER.lock();
            if let Some(sched) = lock.as_mut() {
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
                        if sched.ready_queue.push(task) {
                            sched.request_preempt_for(task.priority);
                        }
                    } else {
                        prev = curr;
                    }

                    curr = next;
                }
            }
        }
        crate::arch::enable_irq();
    }

    pub fn schedule() -> bool {
        crate::arch::disable_irq();
        let ret = {
            let mut lock = SCHEDULER.lock();
            if let Some(sched) = lock.as_mut() {
                sched.schedule_locked()
            } else {
                false
            }
        };
        crate::arch::enable_irq();
        ret
    }

    /// Schedule only if a higher-priority task is waiting.
    ///
    /// This is intended for trap/IRQ return paths. It sets NEXT_TCB when a
    /// switch is needed; the architecture return path performs the actual
    /// context switch.
    pub fn schedule_if_preempt_pending() -> bool {
        crate::arch::disable_irq();
        let ret = {
            let mut lock = SCHEDULER.lock();
            if let Some(sched) = lock.as_mut() {
                if sched.preempt_pending && sched.has_higher_ready_than_current() {
                    sched.schedule_locked()
                } else {
                    sched.preempt_pending = false;
                    unsafe {
                        NEXT_TCB = CURRENT_TCB;
                    }
                    false
                }
            } else {
                false
            }
        };
        crate::arch::enable_irq();
        ret
    }

    pub fn preempt_pending() -> bool {
        let lock = SCHEDULER.lock();
        let Some(sched) = lock.as_ref() else {
            return false;
        };
        sched.preempt_pending && sched.has_higher_ready_than_current()
    }

    pub fn yield_if_preempt_pending() {
        if Self::can_preempt_now() && Self::preempt_pending() {
            Self::yield_task();
        }
    }

    pub fn delay(ticks: u32) {
        crate::arch::disable_irq();
        let wakeup_tick = crate::timer::ticks().wrapping_add(ticks);
        let mut blocked = false;
        {
            let mut lock = SCHEDULER.lock();
            if let Some(sched) = lock.as_mut() {
                let current = unsafe { CURRENT_TCB };
                if !current.is_null() {
                    let task = unsafe { &mut *current };
                    task.state = TaskState::Sleeping;
                    task.wakeup_tick = wakeup_tick;

                    task.next = sched.sleep_queue;
                    sched.sleep_queue = current;
                    blocked = true;
                }
            }
        }
        crate::arch::enable_irq();

        if blocked {
            Self::yield_task();
        }
    }

    pub fn yield_task() {
        #[cfg(target_arch = "arm")]
        {
            if !Self::schedule() {
                return;
            }
        }

        // RISC-V enters the scheduler from the breakpoint trap. Cortex-M
        // reaches here after NEXT_TCB has already been selected above.
        unsafe {
            (crate::arch::YIELD_CPU_FN.expect("Arch not initialized"))();
        }
    }

    pub fn stats() -> SchedulerStats {
        let lock = SCHEDULER.lock();
        if let Some(sched) = lock.as_ref() {
            SchedulerStats {
                ready_tasks: sched.ready_queue.len(),
                highest_ready_priority: if sched.ready_queue.is_empty() {
                    0
                } else {
                    sched.ready_queue.peek_highest_priority()
                },
                current_task: sched.current_task_info(),
                context_switches: sched.context_switches,
                preemptions: sched.preemptions,
                preempt_pending: sched.preempt_pending,
            }
        } else {
            SchedulerStats {
                ready_tasks: 0,
                highest_ready_priority: 0,
                current_task: None,
                context_switches: 0,
                preemptions: 0,
                preempt_pending: false,
            }
        }
    }

    pub fn task_snapshot() -> ([Option<TaskInfo>; TASK_SNAPSHOT_CAP], usize) {
        crate::arch::disable_irq();
        let (snapshot, count) = {
            let lock = SCHEDULER.lock();
            let mut snapshot = [const { None }; TASK_SNAPSHOT_CAP];
            let mut count = 0usize;
            if let Some(sched) = lock.as_ref() {
                if let Some(info) = sched.current_task_info() {
                    push_task_info(&mut snapshot, &mut count, info);
                }

                sched.ready_queue.for_each(|task| {
                    push_task_info(&mut snapshot, &mut count, task_info(task, false));
                });

                let mut curr = sched.sleep_queue;
                while !curr.is_null() {
                    let task = unsafe { &*curr };
                    push_task_info(&mut snapshot, &mut count, task_info(task, false));
                    curr = task.next;
                }
            }

            (snapshot, count)
        };
        crate::arch::enable_irq();
        (snapshot, count)
    }

    fn schedule_locked(&mut self) -> bool {
        self.preempt_pending = false;

        let current = unsafe { CURRENT_TCB };
        let mut was_preempted = false;
        let current_running_priority = if current.is_null() {
            None
        } else {
            let task = unsafe { &mut *current };
            if task.state == TaskState::Running {
                Some(task.priority)
            } else {
                None
            }
        };

        if !current.is_null() {
            let task = unsafe { &mut *current };
            if task.state == TaskState::Running {
                task.state = TaskState::Ready;
                let _ = self.ready_queue.push(task);
            }
        }

        if let Some(next) = self.ready_queue.pop_highest() {
            next.state = TaskState::Running;
            let next_priority = next.priority;
            let next_ptr = next as *mut _;
            unsafe {
                NEXT_TCB = next_ptr;
            }
            if current != next_ptr {
                self.context_switches = self.context_switches.wrapping_add(1);
                if let Some(current_priority) = current_running_priority {
                    was_preempted = next_priority > current_priority;
                }
                if was_preempted {
                    self.preemptions = self.preemptions.wrapping_add(1);
                }
                true
            } else {
                false
            }
        } else {
            // Ready queue empty: keep running the current task.
            unsafe {
                NEXT_TCB = current;
            }
            false
        }
    }

    fn request_preempt_for(&mut self, priority: Priority) {
        if self.priority_preempts_current(priority) {
            self.preempt_pending = true;
        }
    }

    fn priority_preempts_current(&self, priority: Priority) -> bool {
        let current = unsafe { CURRENT_TCB };
        if current.is_null() {
            return false;
        }
        let task = unsafe { &*current };
        task.state == TaskState::Running && priority > task.priority
    }

    fn has_higher_ready_than_current(&self) -> bool {
        let current = unsafe { CURRENT_TCB };
        if current.is_null() || self.ready_queue.is_empty() {
            return false;
        }
        let task = unsafe { &*current };
        task.state == TaskState::Running && self.ready_queue.peek_highest_priority() > task.priority
    }

    fn current_task_info(&self) -> Option<TaskInfo> {
        let current = unsafe { CURRENT_TCB };
        if current.is_null() {
            None
        } else {
            Some(task_info(unsafe { &*current }, true))
        }
    }

    fn can_preempt_now() -> bool {
        unsafe { !CURRENT_TCB.is_null() && crate::arch::ARCH_CRIT_NEST == 0 }
    }
}

static mut IDLE_TASK_STACK: [usize; 256] = [0; 256];
static mut ROOT_TASK_STACK: [usize; 1024] = [0; 1024];

fn idle_task_entry() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

fn task_info(task: &TaskControlBlock, current: bool) -> TaskInfo {
    TaskInfo {
        id: task.id,
        name: task.name,
        priority: task.priority,
        state: task.state,
        stack_size: task.stack_size,
        wakeup_tick: task.wakeup_tick,
        queued: task.queued,
        current,
    }
}

fn push_task_info(
    snapshot: &mut [Option<TaskInfo>; TASK_SNAPSHOT_CAP],
    count: &mut usize,
    info: TaskInfo,
) {
    if *count < snapshot.len() {
        snapshot[*count] = Some(info);
    }
    *count += 1;
}
