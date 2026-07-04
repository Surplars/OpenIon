pub type TaskId = u32;
pub type Priority = u8;

/// CPU affinity for a task.
///
/// When SMP is enabled, tasks can be pinned to a specific CPU
/// or allowed to run on any CPU (None).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuAffinity {
    /// Run on any CPU.
    Any,
    /// Pin to a specific CPU.
    Pin(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Suspended,
    Terminated,
    Sleeping,
}

#[repr(C)]
pub struct TaskControlBlock {
    pub sp: usize, // Stack pointer, placed first for easy access from assembly (offset 0)
    pub id: TaskId,
    pub priority: Priority,
    pub wakeup_tick: u32,
    pub state: TaskState,
    pub stack_size: usize,
    pub entry: fn() -> !,
    pub name: &'static str,
    pub next: *mut TaskControlBlock,
    pub queued: bool,
    /// CPU affinity (only meaningful when SMP is enabled).
    pub affinity: CpuAffinity,
    /// CPU this task is currently running on (set when scheduled).
    pub cpu_id: u32,
}

impl TaskControlBlock {
    pub const fn new(
        id: TaskId,
        entry: fn() -> !,
        initial_sp: usize,
        stack_size: usize,
        priority: Priority,
        name: &'static str,
    ) -> Self {
        Self {
            sp: initial_sp,
            id,
            priority,
            state: TaskState::Ready,
            wakeup_tick: 0,
            stack_size,
            entry,
            name,
            next: core::ptr::null_mut(),
            queued: false,
            affinity: CpuAffinity::Any,
            cpu_id: 0,
        }
    }

    /// Set CPU affinity for this task.
    pub fn set_affinity(&mut self, affinity: CpuAffinity) {
        self.affinity = affinity;
    }

    /// Check if this task can run on the given CPU.
    pub fn can_run_on(&self, cpu_id: u32) -> bool {
        match self.affinity {
            CpuAffinity::Any => true,
            CpuAffinity::Pin(pin_cpu) => pin_cpu == cpu_id,
        }
    }
}

unsafe impl Send for TaskControlBlock {}
unsafe impl Sync for TaskControlBlock {}
