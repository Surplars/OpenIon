//! ELF loader and process support.

pub mod elf;

/// Process ID type
pub type Pid = u32;

/// Process state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Sleeping,
    Zombie,
}

/// Process control block (extends TaskControlBlock with address space info)
pub struct Process {
    pub pid: Pid,
    pub entry_point: usize,
    pub stack_base: usize,
    pub stack_size: usize,
    pub state: ProcessState,
    pub name: &'static str,
}
