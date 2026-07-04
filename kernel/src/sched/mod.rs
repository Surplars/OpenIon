#[cfg(feature = "async_rt")]
pub mod async_rt;
pub mod percpu;
pub mod ready_queue;
pub mod task;
pub mod wait;

pub use wait::{Event, EventStats, Semaphore, SemaphoreStats, WaitQueue, WaitQueueStats};

#[cfg(all(
    not(feature = "mcu_profile"),
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
use crate::mm::PAGE_SIZE;
use crate::mm::slab::Slab;
use crate::sync::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};
use ready_queue::{MAX_PRIORITY, ReadyQueue};
use task::{Priority, TaskControlBlock, TaskId, TaskState};

static SCHEDULER: Mutex<Option<Scheduler>> = Mutex::new(None);

#[cfg(feature = "mcu_profile")]
const TASK_CAP: usize = 8;
#[cfg(not(feature = "mcu_profile"))]
const TASK_CAP: usize = crate::generated_config::OPENION_TASK_CAP;

pub static TCB_POOL: Slab<TaskControlBlock, TASK_CAP> = Slab::new();
pub const TASK_SNAPSHOT_CAP: usize = TASK_CAP;

// Legacy context-switch ABI symbols used directly by RISC-V and Cortex-M assembly.
// Keep Rust scheduler access behind the helpers below so this can become per-CPU
// without changing scheduler call sites.
#[unsafe(no_mangle)]
pub static mut CURRENT_TCB: *mut TaskControlBlock = core::ptr::null_mut();

#[unsafe(no_mangle)]
pub static mut NEXT_TCB: *mut TaskControlBlock = core::ptr::null_mut();

#[inline]
fn active_cpu_mask() -> usize {
    let mask = crate::platform::smp_status().active_mask;
    if mask == 0 { 1 } else { mask }
}

#[inline]
fn cpu_bit(cpu_id: usize) -> Option<usize> {
    if cpu_id < usize::BITS as usize {
        Some(1usize << cpu_id)
    } else {
        None
    }
}

#[inline]
fn is_cpu_active(cpu_id: usize) -> bool {
    cpu_bit(cpu_id)
        .map(|bit| active_cpu_mask() & bit != 0)
        .unwrap_or(false)
}

#[inline]
fn current_scheduler_cpu_id() -> usize {
    let current = percpu::current_cpu_id().raw() as usize % percpu::max_cpus();
    if is_cpu_active(current) { current } else { 0 }
}
#[inline]
fn context_switch_abi_current_task_ptr() -> *mut TaskControlBlock {
    let current = unsafe { CURRENT_TCB };
    sync_current_task_shadow(current);
    current
}

#[inline]
fn context_switch_abi_next_task_ptr() -> *mut TaskControlBlock {
    unsafe { NEXT_TCB }
}

#[inline]
fn context_switch_abi_set_next_task_ptr(task: *mut TaskControlBlock) {
    percpu::set_next_task_ptr(task);
    unsafe {
        NEXT_TCB = task;
    }
}

#[inline]
fn sync_current_task_shadow(task: *mut TaskControlBlock) {
    if percpu::current_task_ptr() != task {
        percpu::set_current_task_ptr(task);
    }
}

#[inline]
pub(crate) fn current_task_ptr() -> *mut TaskControlBlock {
    context_switch_abi_current_task_ptr()
}

#[inline]
fn set_next_task_ptr(task: *mut TaskControlBlock) {
    context_switch_abi_set_next_task_ptr(task);
}

#[inline]
fn keep_current_task() {
    set_next_task_ptr(current_task_ptr());
}

#[inline]
pub(crate) fn has_current_task() -> bool {
    !current_task_ptr().is_null()
}

/// Per-CPU scheduler state.
struct CpuScheduler {
    ready_queue: ReadyQueue,
    idle_task: Option<TaskId>,
    preempt_pending: bool,
    context_switches: u64,
    preemptions: u64,
    /// Number of work-stealing attempts from this CPU.
    steal_attempts: u64,
    /// Number of successful work-steals.
    steal_successes: u64,
}

impl CpuScheduler {
    const fn new() -> Self {
        Self {
            ready_queue: ReadyQueue::new(),
            idle_task: None,
            preempt_pending: false,
            context_switches: 0,
            preemptions: 0,
            steal_attempts: 0,
            steal_successes: 0,
        }
    }
}

pub struct Scheduler {
    /// Per-CPU scheduler states (single element when SMP disabled).
    cpu_scheds: [CpuScheduler; percpu::max_cpus()],
    task_id_counter: AtomicU32,
    sleep_queue: *mut TaskControlBlock,
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
    pub current_cpu: usize,
    pub active_cpu_mask: usize,
    /// Total work-stealing attempts across all CPUs.
    pub steal_attempts: u64,
    /// Successful work-steals across all CPUs.
    pub steal_successes: u64,
}

#[derive(Clone, Copy)]
pub struct ContextSwitchAbiSnapshot {
    pub current_abi: usize,
    pub next_abi: usize,
    pub current_shadow: usize,
    pub next_shadow: usize,
    pub current_slot: usize,
    pub next_slot: usize,
}

impl Scheduler {
    pub fn init() {
        let mut sched = SCHEDULER.lock();
        *sched = Some(Scheduler {
            cpu_scheds: [const { CpuScheduler::new() }; percpu::max_cpus()],
            task_id_counter: AtomicU32::new(0),
            sleep_queue: core::ptr::null_mut(),
        });
    }

    pub fn init_system_tasks(root_entry: fn() -> !) {
        let num_cpus = percpu::max_cpus();

        // Create one idle task per possible CPU. Inactive/parked CPUs keep their
        // idle task queued locally until the platform promotes them to active.
        for cpu_id in 0..num_cpus {
            let idle_id =
                Self::create_task_on_cpu(idle_task_entry, idle_task_stack(), 0, "IDLE", cpu_id);

            if let Some(sched) = SCHEDULER.lock().as_mut() {
                sched.cpu_scheds[cpu_id].idle_task = Some(idle_id);
            }
        }

        Self::create_task(root_entry, root_task_stack(), 1, "ROOT");
    }

    pub fn create_task(
        entry: fn() -> !,
        stack: &'static mut [usize],
        priority: Priority,
        name: &'static str,
    ) -> TaskId {
        Self::create_task_on_cpu(entry, stack, priority, name, current_scheduler_cpu_id())
    }

    fn create_task_on_cpu(
        entry: fn() -> !,
        stack: &'static mut [usize],
        priority: Priority,
        name: &'static str,
        target_cpu: usize,
    ) -> TaskId {
        let priority = priority.min(MAX_PRIORITY);
        let entry_addr = entry as usize;
        let initial_sp = crate::arch::init_task_stack(stack, entry_addr);

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

                let cpu_id = target_cpu % percpu::max_cpus();
                let cpu_sched = &mut sched.cpu_scheds[cpu_id];
                if cpu_sched.ready_queue.push(unsafe { tcb_ptr.as_mut() }) {
                    cpu_sched.preempt_pending = Self::check_preempt_needed(cpu_sched, priority);
                    id
                } else {
                    u32::MAX
                }
            } else {
                u32::MAX
            }
        };
        crate::arch::enable_irq();
        if id != u32::MAX {
            Self::yield_if_preempt_pending();
        } else {
            unsafe {
                TCB_POOL.free(tcb_ptr);
            }
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

                        // Push to appropriate CPU's run queue based on affinity
                        let cpu_id = Self::select_cpu_for_task(sched, task);
                        let cpu_sched = &mut sched.cpu_scheds[cpu_id];
                        if cpu_sched.ready_queue.push(task) {
                            cpu_sched.preempt_pending =
                                Self::check_preempt_needed(cpu_sched, task.priority);
                        } else {
                            task.state = TaskState::Sleeping;
                            task.wakeup_tick = current_tick;
                            task.next = sched.sleep_queue;
                            sched.sleep_queue = curr;
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
                let cpu_id = current_scheduler_cpu_id();
                Self::schedule_cpu_locked(sched, cpu_id)
            } else {
                false
            }
        };
        crate::arch::enable_irq();
        ret
    }

    /// Schedule only if a higher-priority task is waiting.
    pub fn schedule_if_preempt_pending() -> bool {
        crate::arch::disable_irq();
        let ret = {
            let mut lock = SCHEDULER.lock();
            if let Some(sched) = lock.as_mut() {
                let cpu_id = current_scheduler_cpu_id();
                let cpu_sched = &mut sched.cpu_scheds[cpu_id];
                if cpu_sched.preempt_pending && Self::has_higher_ready_than_current(cpu_sched) {
                    Self::schedule_cpu_locked(sched, cpu_id)
                } else {
                    cpu_sched.preempt_pending = false;
                    keep_current_task();
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
        let cpu_id = current_scheduler_cpu_id();
        let cpu_sched = &sched.cpu_scheds[cpu_id];
        cpu_sched.preempt_pending && Self::has_higher_ready_than_current(cpu_sched)
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
                let current = current_task_ptr();
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

    pub fn terminate_current() -> bool {
        crate::arch::disable_irq();
        let terminated = {
            let mut lock = SCHEDULER.lock();
            if let Some(sched) = lock.as_mut() {
                let current = current_task_ptr();
                if current.is_null() {
                    false
                } else {
                    let task = unsafe { &mut *current };
                    task.state = TaskState::Terminated;
                    task.next = core::ptr::null_mut();
                    let cpu_id = current_scheduler_cpu_id();
                    Self::schedule_cpu_locked(sched, cpu_id)
                }
            } else {
                false
            }
        };
        crate::arch::enable_irq();
        terminated
    }

    pub(crate) fn current_task_ptr() -> *mut TaskControlBlock {
        current_task_ptr()
    }

    pub(crate) fn block_current_task_irq_disabled() -> bool {
        let _lock = SCHEDULER.lock();
        let current = current_task_ptr();
        if current.is_null() {
            false
        } else {
            let task = unsafe { &mut *current };
            if task.state == TaskState::Running {
                task.state = TaskState::Blocked;
                task.wakeup_tick = 0;
                task.next = core::ptr::null_mut();
                true
            } else {
                false
            }
        }
    }

    pub(crate) fn wake_blocked_task(task: *mut TaskControlBlock) -> bool {
        if task.is_null() {
            return false;
        }

        crate::arch::disable_irq();
        let woke = {
            let mut lock = SCHEDULER.lock();
            if let Some(sched) = lock.as_mut() {
                let task = unsafe { &mut *task };
                if task.state == TaskState::Blocked {
                    task.state = TaskState::Ready;
                    task.wakeup_tick = 0;
                    task.next = core::ptr::null_mut();

                    // Select CPU based on affinity
                    let cpu_id = Self::select_cpu_for_task(sched, task);
                    let cpu_sched = &mut sched.cpu_scheds[cpu_id];
                    if cpu_sched.ready_queue.push(task) {
                        cpu_sched.preempt_pending =
                            Self::check_preempt_needed(cpu_sched, task.priority);
                        true
                    } else {
                        task.state = TaskState::Blocked;
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        };
        crate::arch::enable_irq();

        if woke {
            Self::yield_if_preempt_pending();
        }
        woke
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
        crate::arch::yield_cpu();
    }

    pub fn context_switch_abi_snapshot() -> ContextSwitchAbiSnapshot {
        let current_abi = context_switch_abi_current_task_ptr() as usize;
        ContextSwitchAbiSnapshot {
            current_abi,
            next_abi: context_switch_abi_next_task_ptr() as usize,
            current_shadow: percpu::current_task_ptr() as usize,
            next_shadow: percpu::next_task_ptr() as usize,
            current_slot: percpu::current_task_slot() as usize,
            next_slot: percpu::next_task_slot() as usize,
        }
    }
    pub fn stats() -> SchedulerStats {
        let lock = SCHEDULER.lock();
        if let Some(sched) = lock.as_ref() {
            let cpu_id = current_scheduler_cpu_id();
            let cpu_sched = &sched.cpu_scheds[cpu_id];

            // Aggregate work-stealing stats across all CPUs
            let mut total_steal_attempts = 0;
            let mut total_steal_successes = 0;
            for cs in sched.cpu_scheds.iter() {
                total_steal_attempts += cs.steal_attempts;
                total_steal_successes += cs.steal_successes;
            }

            SchedulerStats {
                ready_tasks: cpu_sched.ready_queue.len(),
                highest_ready_priority: if cpu_sched.ready_queue.is_empty() {
                    0
                } else {
                    cpu_sched.ready_queue.peek_highest_priority()
                },
                current_task: Self::current_task_info_from_global(),
                context_switches: cpu_sched.context_switches,
                preemptions: cpu_sched.preemptions,
                preempt_pending: cpu_sched.preempt_pending,
                current_cpu: cpu_id,
                active_cpu_mask: active_cpu_mask(),
                steal_attempts: total_steal_attempts,
                steal_successes: total_steal_successes,
            }
        } else {
            SchedulerStats {
                ready_tasks: 0,
                highest_ready_priority: 0,
                current_task: None,
                context_switches: 0,
                preemptions: 0,
                preempt_pending: false,
                current_cpu: 0,
                active_cpu_mask: active_cpu_mask(),
                steal_attempts: 0,
                steal_successes: 0,
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
                if let Some(info) = Self::current_task_info_from_global() {
                    push_task_info(&mut snapshot, &mut count, info);
                }

                // Snapshot from all CPU run queues
                for cpu_sched in sched.cpu_scheds.iter() {
                    cpu_sched.ready_queue.for_each(|task| {
                        push_task_info(&mut snapshot, &mut count, task_info(task, false));
                    });
                }

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

    /// Schedule on a specific CPU.
    fn schedule_cpu_locked(sched: &mut Scheduler, cpu_id: usize) -> bool {
        let cpu_sched = &mut sched.cpu_scheds[cpu_id];
        cpu_sched.preempt_pending = false;

        let current = current_task_ptr();
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

        if let Some(current_priority) = current_running_priority {
            let highest_ready = cpu_sched.ready_queue.peek_highest_priority();
            if cpu_sched.ready_queue.is_empty() || highest_ready < current_priority {
                set_next_task_ptr(current);
                return false;
            }
        }

        if !current.is_null() {
            let task = unsafe { &mut *current };
            if task.state == TaskState::Running {
                task.state = TaskState::Ready;
                if !cpu_sched.ready_queue.push(task) {
                    task.state = TaskState::Running;
                    set_next_task_ptr(current);
                    return false;
                }
            }
        }

        // Try to get a task from local queue first
        let mut next = cpu_sched.ready_queue.pop_highest();

        // If local queue is empty, try work-stealing
        if next.is_none() && percpu::max_cpus() > 1 {
            next = Self::try_steal_task(sched, cpu_id);
        }

        if let Some(next) = next {
            next.state = TaskState::Running;
            next.cpu_id = cpu_id as u32;
            let next_priority = next.priority;
            let next_ptr = next as *mut _;
            set_next_task_ptr(next_ptr);
            if current != next_ptr {
                sched.cpu_scheds[cpu_id].context_switches =
                    sched.cpu_scheds[cpu_id].context_switches.wrapping_add(1);
                if let Some(current_priority) = current_running_priority {
                    was_preempted = next_priority > current_priority;
                }
                if was_preempted {
                    sched.cpu_scheds[cpu_id].preemptions =
                        sched.cpu_scheds[cpu_id].preemptions.wrapping_add(1);
                }
                true
            } else {
                false
            }
        } else {
            // Ready queue empty: keep running the current task.
            set_next_task_ptr(current);
            false
        }
    }

    /// Try to steal a task from another CPU's run queue.
    fn try_steal_task(sched: &mut Scheduler, cpu_id: usize) -> Option<&mut TaskControlBlock> {
        let num_cpus = percpu::max_cpus();
        sched.cpu_scheds[cpu_id].steal_attempts += 1;

        // Try to steal from other CPUs, starting from the next one
        for offset in 1..num_cpus {
            let victim_id = (cpu_id + offset) % num_cpus;
            if !is_cpu_active(victim_id) {
                continue;
            }

            // Only steal if victim has enough tasks
            if sched.cpu_scheds[victim_id].ready_queue.len() > 1 {
                // Use raw pointer to split borrow - we know cpu_id != victim_id
                let victim_queue = &mut sched.cpu_scheds[victim_id].ready_queue as *mut ReadyQueue;
                let task_ptr = unsafe { (*victim_queue).pop_lowest_raw() };

                if !task_ptr.is_null() {
                    let task = unsafe { &mut *task_ptr };
                    // Check affinity
                    if task.can_run_on(cpu_id as u32) {
                        sched.cpu_scheds[cpu_id].steal_successes += 1;
                        return Some(task);
                    } else {
                        // Task can't run on this CPU, push it back
                        unsafe {
                            (*victim_queue).push_raw(task_ptr);
                        }
                    }
                }
            }
        }

        None
    }

    /// Select the best CPU for a task based on affinity and load.
    fn select_cpu_for_task(sched: &Scheduler, task: &TaskControlBlock) -> usize {
        match task.affinity {
            task::CpuAffinity::Pin(cpu) => {
                let target = cpu as usize % percpu::max_cpus();
                if is_cpu_active(target) {
                    target
                } else {
                    current_scheduler_cpu_id()
                }
            }
            task::CpuAffinity::Any => {
                // Find the active CPU with the shortest run queue.
                let mut best_cpu = current_scheduler_cpu_id();
                let mut best_len = usize::MAX;
                for (i, cpu_sched) in sched.cpu_scheds.iter().enumerate() {
                    if !is_cpu_active(i) {
                        continue;
                    }
                    let len = cpu_sched.ready_queue.len();
                    if len < best_len {
                        best_len = len;
                        best_cpu = i;
                    }
                }
                best_cpu
            }
        }
    }

    /// Check if a new task priority should trigger preemption.
    fn check_preempt_needed(_cpu_sched: &CpuScheduler, priority: Priority) -> bool {
        let current = current_task_ptr();
        if current.is_null() {
            return false;
        }
        let task = unsafe { &*current };
        task.state == TaskState::Running && priority > task.priority
    }

    /// Check if there's a higher priority task ready than the current one.
    fn has_higher_ready_than_current(cpu_sched: &CpuScheduler) -> bool {
        let current = current_task_ptr();
        if current.is_null() || cpu_sched.ready_queue.is_empty() {
            return false;
        }
        let task = unsafe { &*current };
        task.state == TaskState::Running
            && cpu_sched.ready_queue.peek_highest_priority() > task.priority
    }

    fn current_task_info_from_global() -> Option<TaskInfo> {
        let current = current_task_ptr();
        if current.is_null() {
            None
        } else {
            Some(task_info(unsafe { &*current }, true))
        }
    }

    fn can_preempt_now() -> bool {
        has_current_task() && !crate::arch::in_critical_section()
    }
}

#[cfg(all(
    not(feature = "mcu_profile"),
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
#[repr(C, align(4096))]
struct GuardedStack<const WORDS: usize> {
    guard: [u8; PAGE_SIZE],
    stack: [usize; WORDS],
}

#[cfg(all(
    not(feature = "mcu_profile"),
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
impl<const WORDS: usize> GuardedStack<WORDS> {
    const fn new() -> Self {
        Self {
            guard: [0; PAGE_SIZE],
            stack: [0; WORDS],
        }
    }
}

#[cfg(all(
    not(feature = "mcu_profile"),
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
unsafe fn guarded_stack_slice<const WORDS: usize>(
    stack: *mut GuardedStack<WORDS>,
) -> &'static mut [usize] {
    unsafe { &mut (*stack).stack }
}

#[cfg(all(
    not(feature = "mcu_profile"),
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
unsafe fn guarded_stack_guard<const WORDS: usize>(stack: *mut GuardedStack<WORDS>) -> usize {
    unsafe { (*stack).guard.as_ptr() as usize }
}

#[cfg(all(
    not(feature = "mcu_profile"),
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
pub fn for_each_stack_guard(mut f: impl FnMut(usize)) {
    unsafe {
        f(guarded_stack_guard(core::ptr::addr_of_mut!(
            IDLE_TASK_STACK
        )));
        f(guarded_stack_guard(core::ptr::addr_of_mut!(
            ROOT_TASK_STACK
        )));
    }
}

#[cfg(any(
    feature = "mcu_profile",
    not(any(target_arch = "riscv32", target_arch = "riscv64"))
))]
pub fn for_each_stack_guard(_f: impl FnMut(usize)) {}

#[cfg(all(
    not(feature = "mcu_profile"),
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
fn idle_task_stack() -> &'static mut [usize] {
    unsafe { guarded_stack_slice(core::ptr::addr_of_mut!(IDLE_TASK_STACK)) }
}

#[cfg(any(
    feature = "mcu_profile",
    not(any(target_arch = "riscv32", target_arch = "riscv64"))
))]
fn idle_task_stack() -> &'static mut [usize] {
    unsafe { &mut *core::ptr::addr_of_mut!(IDLE_TASK_STACK) }
}

#[cfg(all(
    not(feature = "mcu_profile"),
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
fn root_task_stack() -> &'static mut [usize] {
    unsafe { guarded_stack_slice(core::ptr::addr_of_mut!(ROOT_TASK_STACK)) }
}

#[cfg(any(
    feature = "mcu_profile",
    not(any(target_arch = "riscv32", target_arch = "riscv64"))
))]
fn root_task_stack() -> &'static mut [usize] {
    unsafe { &mut *core::ptr::addr_of_mut!(ROOT_TASK_STACK) }
}

#[cfg(feature = "mcu_profile")]
static mut IDLE_TASK_STACK: [usize; 128] = [0; 128];
#[cfg(all(
    not(feature = "mcu_profile"),
    not(any(target_arch = "riscv32", target_arch = "riscv64"))
))]
static mut IDLE_TASK_STACK: [usize; crate::generated_config::OPENION_IDLE_STACK_WORDS] =
    [0; crate::generated_config::OPENION_IDLE_STACK_WORDS];
#[cfg(all(
    not(feature = "mcu_profile"),
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
static mut IDLE_TASK_STACK: GuardedStack<{ crate::generated_config::OPENION_IDLE_STACK_WORDS }> =
    GuardedStack::new();
#[cfg(feature = "mcu_profile")]
static mut ROOT_TASK_STACK: [usize; 512] = [0; 512];
#[cfg(all(
    not(feature = "mcu_profile"),
    not(any(target_arch = "riscv32", target_arch = "riscv64"))
))]
static mut ROOT_TASK_STACK: [usize; crate::generated_config::OPENION_ROOT_STACK_WORDS] =
    [0; crate::generated_config::OPENION_ROOT_STACK_WORDS];
#[cfg(all(
    not(feature = "mcu_profile"),
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
static mut ROOT_TASK_STACK: GuardedStack<{ crate::generated_config::OPENION_ROOT_STACK_WORDS }> =
    GuardedStack::new();

fn idle_task_entry() -> ! {
    loop {
        crate::arch::idle_hint();
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
