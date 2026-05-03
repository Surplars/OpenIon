pub type TaskId = u32;
pub type Priority = u8;

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
        }
    }
}

unsafe impl Send for TaskControlBlock {}
unsafe impl Sync for TaskControlBlock {}
