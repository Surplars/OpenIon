/// Per-CPU data structures for SMP support.
///
/// When `OPENION_SMP` is disabled, this module provides a single-CPU view.
/// When enabled, it provides per-CPU data for up to `OPENION_SMP_MAX_CPUS` CPUs.
use crate::generated_config::{OPENION_SMP, OPENION_SMP_MAX_CPUS};
use crate::sched::task::TaskControlBlock;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Logical CPU identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CpuId(u32);

impl CpuId {
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Maximum number of CPUs supported (from config).
const MAX_CPUS: usize = if OPENION_SMP { OPENION_SMP_MAX_CPUS } else { 1 };

/// Per-CPU data structure.
#[repr(C)]
pub struct PerCpuData {
    /// Current running task shadow for this CPU.
    current_tcb: AtomicPtr<TaskControlBlock>,
    /// Next task shadow for this CPU.
    next_tcb: AtomicPtr<TaskControlBlock>,
    /// Per-CPU critical section nesting counter.
    crit_nest: AtomicUsize,
    /// CPU id.
    pub cpu_id: CpuId,
}

impl PerCpuData {
    pub const fn new(cpu_id: CpuId) -> Self {
        Self {
            current_tcb: AtomicPtr::new(core::ptr::null_mut()),
            next_tcb: AtomicPtr::new(core::ptr::null_mut()),
            crit_nest: AtomicUsize::new(0),
            cpu_id,
        }
    }
}

/// Per-CPU data array.
static PER_CPU: [PerCpuData; MAX_CPUS] = {
    let mut arr = [const { PerCpuData::new(CpuId(0)) }; MAX_CPUS];
    let mut i = 0;
    while i < MAX_CPUS {
        arr[i] = PerCpuData::new(CpuId(i as u32));
        i += 1;
    }
    arr
};

/// Get the current CPU id.
pub fn current_cpu_id() -> CpuId {
    if OPENION_SMP {
        CpuId::new(crate::arch::current_cpu_id())
    } else {
        CpuId::new(0)
    }
}

/// Get per-CPU data for the current CPU.
pub fn current_cpu() -> &'static PerCpuData {
    let id = current_cpu_id().raw() as usize;
    &PER_CPU[id % MAX_CPUS]
}

/// Get per-CPU data for a specific CPU.
pub fn cpu_data(cpu_id: CpuId) -> &'static PerCpuData {
    &PER_CPU[cpu_id.raw() as usize % MAX_CPUS]
}

/// Return this CPU's current-task shadow.
pub fn current_task_ptr() -> *mut TaskControlBlock {
    current_cpu().current_tcb.load(Ordering::Relaxed)
}

/// Update this CPU's current-task shadow.
pub fn set_current_task_ptr(task: *mut TaskControlBlock) {
    current_cpu().current_tcb.store(task, Ordering::Relaxed);
}

/// Return the current CPU's current-task ABI slot.
pub fn current_task_slot() -> *const AtomicPtr<TaskControlBlock> {
    &current_cpu().current_tcb as *const _
}

/// Return this CPU's next-task shadow.
pub fn next_task_ptr() -> *mut TaskControlBlock {
    current_cpu().next_tcb.load(Ordering::Relaxed)
}

/// Update this CPU's next-task shadow.
pub fn set_next_task_ptr(task: *mut TaskControlBlock) {
    current_cpu().next_tcb.store(task, Ordering::Relaxed);
}

/// Return the current CPU's next-task ABI slot.
pub fn next_task_slot() -> *const AtomicPtr<TaskControlBlock> {
    &current_cpu().next_tcb as *const _
}

/// Enter a per-CPU critical section and return the new nesting depth.
pub fn enter_critical() -> usize {
    current_cpu().crit_nest.fetch_add(1, Ordering::Relaxed) + 1
}

/// Leave a per-CPU critical section and return the new nesting depth.
pub fn exit_critical() -> usize {
    let cpu = current_cpu();
    let nest = cpu.crit_nest.load(Ordering::Relaxed);
    if nest == 0 {
        0
    } else {
        let next = nest - 1;
        cpu.crit_nest.store(next, Ordering::Relaxed);
        next
    }
}

/// Return this CPU's critical nesting depth.
pub fn critical_nest() -> usize {
    current_cpu().crit_nest.load(Ordering::Relaxed)
}

/// Set this CPU's critical nesting depth.
pub fn set_critical_nest(nest: usize) {
    current_cpu().crit_nest.store(nest, Ordering::Relaxed);
}

/// Returns true if SMP is enabled.
pub const fn smp_enabled() -> bool {
    OPENION_SMP
}

/// Returns the configured maximum number of CPUs.
pub const fn max_cpus() -> usize {
    MAX_CPUS
}
