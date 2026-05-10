use crate::sync::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

pub const VM_NAME_MAX: usize = 24;
const MAX_VMS: usize = 4;
const MAX_VCPUS_PER_VM: usize = 1;
const GUEST_MEM_SIZE: usize = 4096;
const DEMO_GUEST_BASE: usize = 0x0000_0000;
const DEMO_GUEST_ENTRY: usize = DEMO_GUEST_BASE;
const DEMO_GUEST_IMAGE: [u8; 12] = [
    0x93, 0x08, 0x10, 0x00, // addi a7, zero, 1
    0x13, 0x05, 0x80, 0x04, // addi a0, zero, 'H'
    0x73, 0x00, 0x00, 0x00, // ecall
];

#[repr(align(4096))]
#[cfg(feature = "hypervisor")]
struct GuestMemory([[u8; GUEST_MEM_SIZE]; MAX_VMS]);

#[derive(Clone, Copy)]
pub struct GuestImage {
    pub entry: usize,
    pub bytes: &'static [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HvState {
    Disabled,
    Compiled,
    Unsupported,
    Ready,
    Faulted,
}

impl HvState {
    pub const fn as_str(self) -> &'static str {
        match self {
            HvState::Disabled => "disabled",
            HvState::Compiled => "compiled",
            HvState::Unsupported => "unsupported",
            HvState::Ready => "ready",
            HvState::Faulted => "faulted",
        }
    }

    const fn as_u32(self) -> u32 {
        match self {
            HvState::Disabled => 0,
            HvState::Compiled => 1,
            HvState::Unsupported => 2,
            HvState::Ready => 3,
            HvState::Faulted => 4,
        }
    }

    const fn from_u32(value: u32) -> Self {
        match value {
            1 => HvState::Compiled,
            2 => HvState::Unsupported,
            3 => HvState::Ready,
            4 => HvState::Faulted,
            _ => HvState::Disabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HvExitKind {
    None,
    Interrupt,
    Ecall,
    PageFault,
    IllegalInstruction,
    Unsupported,
    Fatal,
}

impl HvExitKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            HvExitKind::None => "none",
            HvExitKind::Interrupt => "interrupt",
            HvExitKind::Ecall => "ecall",
            HvExitKind::PageFault => "page_fault",
            HvExitKind::IllegalInstruction => "illegal_instruction",
            HvExitKind::Unsupported => "unsupported",
            HvExitKind::Fatal => "fatal",
        }
    }

    const fn as_u32(self) -> u32 {
        match self {
            HvExitKind::None => 0,
            HvExitKind::Interrupt => 1,
            HvExitKind::Ecall => 2,
            HvExitKind::PageFault => 3,
            HvExitKind::IllegalInstruction => 4,
            HvExitKind::Unsupported => 5,
            HvExitKind::Fatal => 6,
        }
    }

    const fn from_u32(value: u32) -> Self {
        match value {
            1 => HvExitKind::Interrupt,
            2 => HvExitKind::Ecall,
            3 => HvExitKind::PageFault,
            4 => HvExitKind::IllegalInstruction,
            5 => HvExitKind::Unsupported,
            6 => HvExitKind::Fatal,
            _ => HvExitKind::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    Empty,
    Created,
    Ready,
    Running,
    Stopped,
    Faulted,
}

impl VmState {
    pub const fn as_str(self) -> &'static str {
        match self {
            VmState::Empty => "empty",
            VmState::Created => "created",
            VmState::Ready => "ready",
            VmState::Running => "running",
            VmState::Stopped => "stopped",
            VmState::Faulted => "faulted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HvError {
    Disabled,
    Unsupported,
    NoSpace,
    AlreadyExists,
    NotFound,
    InvalidName,
    ImageTooLarge,
    NotReady,
}

impl HvError {
    pub const fn message(self) -> &'static str {
        match self {
            HvError::Disabled => "hypervisor disabled",
            HvError::Unsupported => "hypervisor unsupported",
            HvError::NoSpace => "no VM slot available",
            HvError::AlreadyExists => "VM already exists",
            HvError::NotFound => "VM not found",
            HvError::InvalidName => "invalid VM name",
            HvError::ImageTooLarge => "guest image too large",
            HvError::NotReady => "VM not ready",
        }
    }
}

#[derive(Clone, Copy)]
pub struct VmInfo {
    pub id: u32,
    pub name: [u8; VM_NAME_MAX],
    pub name_len: usize,
    pub state: VmState,
    pub vcpu_count: u8,
    pub guest_base: usize,
    pub guest_size: usize,
    pub host_base: usize,
    pub entry: usize,
}

impl VmInfo {
    const fn empty() -> Self {
        Self {
            id: 0,
            name: [0; VM_NAME_MAX],
            name_len: 0,
            state: VmState::Empty,
            vcpu_count: 0,
            guest_base: 0,
            guest_size: 0,
            host_base: 0,
            entry: 0,
        }
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
}

#[derive(Clone, Copy)]
pub struct HvStats {
    pub compiled: bool,
    pub enabled: bool,
    pub state: HvState,
    pub h_extension: bool,
    pub vm_count: u32,
    pub vcpu_count: u32,
    pub exits: u32,
    pub last_exit: HvExitKind,
    pub last_cause: usize,
    pub last_stval: usize,
}

struct VmTable {
    slots: [VmInfo; MAX_VMS],
    next_id: u32,
}

impl VmTable {
    const fn new() -> Self {
        Self {
            slots: [VmInfo::empty(); MAX_VMS],
            next_id: 1,
        }
    }
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static STATE: AtomicU32 = AtomicU32::new(HvState::Disabled.as_u32());
static H_EXTENSION: AtomicBool = AtomicBool::new(false);
static VM_COUNT: AtomicU32 = AtomicU32::new(0);
static VCPU_COUNT: AtomicU32 = AtomicU32::new(0);
static EXITS: AtomicU32 = AtomicU32::new(0);
static LAST_EXIT: AtomicU32 = AtomicU32::new(HvExitKind::None.as_u32());
static LAST_CAUSE: AtomicUsize = AtomicUsize::new(0);
static LAST_STVAL: AtomicUsize = AtomicUsize::new(0);
static VM_TABLE: Mutex<VmTable> = Mutex::new(VmTable::new());
#[cfg(feature = "hypervisor")]
static GUEST_MEMORY: Mutex<GuestMemory> = Mutex::new(GuestMemory([[0; GUEST_MEM_SIZE]; MAX_VMS]));

pub fn init(compiled: bool, h_extension: bool) {
    ENABLED.store(compiled && h_extension, Ordering::Release);
    H_EXTENSION.store(h_extension, Ordering::Release);
    STATE.store(
        if !compiled {
            HvState::Disabled
        } else if h_extension {
            HvState::Ready
        } else {
            HvState::Unsupported
        }
        .as_u32(),
        Ordering::Release,
    );
}

pub fn create_vm(name: &str) -> Result<u32, HvError> {
    match state() {
        HvState::Disabled => return Err(HvError::Disabled),
        HvState::Unsupported if cfg!(feature = "hypervisor") => {}
        HvState::Unsupported | HvState::Compiled => return Err(HvError::Unsupported),
        HvState::Faulted => return Err(HvError::NotReady),
        HvState::Ready => {}
    }

    if name.is_empty() || name.len() > VM_NAME_MAX {
        return Err(HvError::InvalidName);
    }

    let mut table = VM_TABLE.lock();
    for vm in table.slots.iter() {
        if vm.state != VmState::Empty && vm.name_str() == name {
            return Err(HvError::AlreadyExists);
        }
    }

    let Some(slot_idx) = table.slots.iter().position(|vm| vm.state == VmState::Empty) else {
        return Err(HvError::NoSpace);
    };

    let id = table.next_id;
    table.next_id = table.next_id.wrapping_add(1).max(1);

    let mut name_buf = [0u8; VM_NAME_MAX];
    name_buf[..name.len()].copy_from_slice(name.as_bytes());
    table.slots[slot_idx] = VmInfo {
        id,
        name: name_buf,
        name_len: name.len(),
        state: VmState::Created,
        vcpu_count: MAX_VCPUS_PER_VM as u8,
        guest_base: 0,
        guest_size: 0,
        host_base: 0,
        entry: 0,
    };

    VM_COUNT.fetch_add(1, Ordering::AcqRel);
    VCPU_COUNT.fetch_add(MAX_VCPUS_PER_VM as u32, Ordering::AcqRel);
    Ok(id)
}

pub fn find_vm(name: &str) -> Result<VmInfo, HvError> {
    let table = VM_TABLE.lock();
    for vm in table.slots.iter() {
        if vm.state != VmState::Empty && vm.name_str() == name {
            return Ok(*vm);
        }
    }
    Err(HvError::NotFound)
}

pub fn load_demo(name: &str) -> Result<(), HvError> {
    load_image(
        name,
        GuestImage {
            entry: DEMO_GUEST_ENTRY,
            bytes: &DEMO_GUEST_IMAGE,
        },
    )
}

pub fn load_image(name: &str, image: GuestImage) -> Result<(), HvError> {
    if image.bytes.len() > GUEST_MEM_SIZE {
        return Err(HvError::ImageTooLarge);
    }

    let mut table = VM_TABLE.lock();
    let Some(slot_idx) = table
        .slots
        .iter()
        .position(|vm| vm.state != VmState::Empty && vm.name_str() == name)
    else {
        return Err(HvError::NotFound);
    };

    if table.slots[slot_idx].state == VmState::Running {
        return Err(HvError::NotReady);
    }

    table.slots[slot_idx].guest_base = DEMO_GUEST_BASE;
    table.slots[slot_idx].guest_size = GUEST_MEM_SIZE;
    table.slots[slot_idx].host_base = load_guest_memory(slot_idx, image.bytes)?;
    table.slots[slot_idx].entry = image.entry;
    table.slots[slot_idx].state = VmState::Ready;
    Ok(())
}

pub fn list_vms() -> ([Option<VmInfo>; MAX_VMS], usize) {
    let table = VM_TABLE.lock();
    let mut out = [None; MAX_VMS];
    let mut count = 0usize;
    for vm in table.slots.iter() {
        if vm.state != VmState::Empty {
            if count < out.len() {
                out[count] = Some(*vm);
            }
            count += 1;
        }
    }
    (out, count)
}

pub fn mark_vm_faulted(name: &str, cause: usize, stval: usize) -> Result<(), HvError> {
    let mut table = VM_TABLE.lock();
    for vm in table.slots.iter_mut() {
        if vm.state != VmState::Empty && vm.name_str() == name {
            vm.state = VmState::Faulted;
            set_faulted(cause, stval);
            return Ok(());
        }
    }
    Err(HvError::NotFound)
}

pub fn record_exit(kind: HvExitKind, cause: usize, stval: usize) {
    EXITS.fetch_add(1, Ordering::AcqRel);
    LAST_EXIT.store(kind.as_u32(), Ordering::Release);
    LAST_CAUSE.store(cause, Ordering::Release);
    LAST_STVAL.store(stval, Ordering::Release);
}

pub fn set_faulted(cause: usize, stval: usize) {
    STATE.store(HvState::Faulted.as_u32(), Ordering::Release);
    record_exit(HvExitKind::Fatal, cause, stval);
}

pub fn stats() -> HvStats {
    HvStats {
        compiled: cfg!(feature = "hypervisor"),
        enabled: ENABLED.load(Ordering::Acquire),
        state: state(),
        h_extension: H_EXTENSION.load(Ordering::Acquire),
        vm_count: VM_COUNT.load(Ordering::Acquire),
        vcpu_count: VCPU_COUNT.load(Ordering::Acquire),
        exits: EXITS.load(Ordering::Acquire),
        last_exit: HvExitKind::from_u32(LAST_EXIT.load(Ordering::Acquire)),
        last_cause: LAST_CAUSE.load(Ordering::Acquire),
        last_stval: LAST_STVAL.load(Ordering::Acquire),
    }
}

fn state() -> HvState {
    HvState::from_u32(STATE.load(Ordering::Acquire))
}

#[cfg(feature = "hypervisor")]
fn load_guest_memory(slot_idx: usize, bytes: &[u8]) -> Result<usize, HvError> {
    let mut mem = GUEST_MEMORY.lock();
    let host_base = mem.0[slot_idx].as_ptr() as usize;
    mem.0[slot_idx].fill(0);
    mem.0[slot_idx][..bytes.len()].copy_from_slice(bytes);
    Ok(host_base)
}

#[cfg(not(feature = "hypervisor"))]
fn load_guest_memory(_slot_idx: usize, _bytes: &[u8]) -> Result<usize, HvError> {
    Ok(0)
}
