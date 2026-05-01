#![no_std]

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicUsize, Ordering};

use kernel::driver::{Driver, DriverErr, DriverFactory, DriverResult, GenericDeviceConfig};
use kernel::driver::block::BlockDevice;
use kernel::driver::manager::AnyDriver;

// VirtIO MMIO register offsets
const MAGIC: usize = 0x000;
const VERSION: usize = 0x004;
const DEVICE_ID: usize = 0x008;
const STATUS: usize = 0x070;
const DEVICE_FEATURES: usize = 0x010;
const DRIVER_FEATURES: usize = 0x020;
const QUEUE_SEL: usize = 0x030;
const QUEUE_NUM_MAX: usize = 0x034;
const QUEUE_NUM: usize = 0x038;
const QUEUE_READY: usize = 0x044;
const QUEUE_DESC_LOW: usize = 0x080;
const QUEUE_DESC_HIGH: usize = 0x084;
const QUEUE_DRIVER_LOW: usize = 0x090;
const QUEUE_DRIVER_HIGH: usize = 0x094;
const QUEUE_DEVICE_LOW: usize = 0x0a0;
const QUEUE_DEVICE_HIGH: usize = 0x0a4;
const QUEUE_NOTIFY: usize = 0x050;
const INTERRUPT_STATUS: usize = 0x060;
const INTERRUPT_ACK: usize = 0x064;

// Status bits
const STATUS_ACK: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;

// VirtIO block device request types
const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;

// Descriptor flags
const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

const QUEUE_SIZE: usize = 16;

#[repr(C)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
struct VirtqAvail {
    flags: u16,
    idx: u16,
    ring: [u16; QUEUE_SIZE],
}

#[repr(C)]
struct VirtqUsed {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; QUEUE_SIZE],
}

#[repr(C)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

#[repr(C)]
struct BlkRequest {
    type_: u32,
    reserved: u32,
    sector: u64,
    data: [u8; 512],
    status: u8,
}

pub struct VirtioBlk {
    base_addr: usize,
    irq_num: u32,
    capacity: u64,
    // Virtqueue memory (must be contiguous and aligned)
    desc: *mut VirtqDesc,
    avail: *mut VirtqAvail,
    used: *mut VirtqUsed,
    request: *mut BlkRequest,
    free_head: u16,
    last_used_idx: UnsafeCell<u16>,
}

// Safety: accessed only through Driver trait methods with proper synchronization
unsafe impl Send for VirtioBlk {}
unsafe impl Sync for VirtioBlk {}

impl VirtioBlk {
    pub const fn new(base_addr: usize, irq_num: u32) -> Self {
        Self {
            base_addr,
            irq_num,
            capacity: 0,
            desc: core::ptr::null_mut(),
            avail: core::ptr::null_mut(),
            used: core::ptr::null_mut(),
            request: core::ptr::null_mut(),
            free_head: 0,
            last_used_idx: UnsafeCell::new(0),
        }
    }

    fn reg_read32(&self, offset: usize) -> u32 {
        unsafe { read_volatile((self.base_addr + offset) as *const u32) }
    }

    fn reg_write32(&self, offset: usize, val: u32) {
        unsafe { write_volatile((self.base_addr + offset) as *mut u32, val) }
    }

    fn reg_read64(&self, offset: usize) -> u64 {
        let lo = self.reg_read32(offset) as u64;
        let hi = self.reg_read32(offset + 4) as u64;
        (hi << 32) | lo
    }

    fn setup_queue(&mut self) -> bool {
        self.reg_write32(QUEUE_SEL, 0);
        let max = self.reg_read32(QUEUE_NUM_MAX);
        if max == 0 || (max as usize) < QUEUE_SIZE {
            return false;
        }
        self.reg_write32(QUEUE_NUM, QUEUE_SIZE as u32);

        // Allocate aligned memory for virtqueue structures + request buffer
        // Needs: desc(256) + avail(~36) + page_gap + used(~132) + request(529+align)
        // Worst case: 4095(align) + 4096 + 136 + 529 = 8856 bytes → use 12KB (3 pages)
        use core::cell::UnsafeCell;
        struct QueueMem(UnsafeCell<[u8; 12288]>);
        unsafe impl Sync for QueueMem {}
        static QUEUE_MEM: QueueMem = QueueMem(UnsafeCell::new([0u8; 12288]));
        let base = QUEUE_MEM.0.get() as usize;
        let aligned = (base + 4095) & !4095;

        self.desc = aligned as *mut VirtqDesc;
        self.avail = (aligned + QUEUE_SIZE * 16) as *mut VirtqAvail;
        self.used = (aligned + 4096) as *mut VirtqUsed;
        // BlkRequest.sector is u64 (needs 8-byte align); VirtqUsed is 132 bytes (not 8-aligned)
        let request_off = (core::mem::size_of::<VirtqUsed>() + 7) & !7;
        self.request = (aligned + 4096 + request_off) as *mut BlkRequest;

        // Zero out queue memory
        unsafe {
            core::ptr::write_bytes(aligned as *mut u8, 0, 4096);
        }

        // Set descriptor table address
        let desc_addr = self.desc as u64;
        self.reg_write32(QUEUE_DESC_LOW, desc_addr as u32);
        self.reg_write32(QUEUE_DESC_HIGH, (desc_addr >> 32) as u32);

        // Set available ring address
        let avail_addr = self.avail as u64;
        self.reg_write32(QUEUE_DRIVER_LOW, avail_addr as u32);
        self.reg_write32(QUEUE_DRIVER_HIGH, (avail_addr >> 32) as u32);

        // Set used ring address
        let used_addr = self.used as u64;
        self.reg_write32(QUEUE_DEVICE_LOW, used_addr as u32);
        self.reg_write32(QUEUE_DEVICE_HIGH, (used_addr >> 32) as u32);

        self.reg_write32(QUEUE_READY, 1);

        // Set up descriptor chain: [header, data, status]
        unsafe {
            // Descriptor 0: header (BlkRequest type + sector)
            (*self.desc).addr = self.request as u64;
            (*self.desc).len = 16; // type(4) + reserved(4) + sector(8)
            (*self.desc).flags = VIRTQ_DESC_F_NEXT;
            (*self.desc).next = 1;

            // Descriptor 1: data (512 bytes) — device-writable
            (*self.desc.add(1)).addr = (self.request as usize + 16) as u64;
            (*self.desc.add(1)).len = 512;
            (*self.desc.add(1)).flags = VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE;
            (*self.desc.add(1)).next = 2;

            // Descriptor 2: status (1 byte)
            (*self.desc.add(2)).addr = (self.request as usize + 16 + 512) as u64;
            (*self.desc.add(2)).len = 1;
            (*self.desc.add(2)).flags = VIRTQ_DESC_F_WRITE;

            self.free_head = 0;
            *self.last_used_idx.get() = 0;
        }

        true
    }

    fn read_capacity(&mut self) {
        // The capacity is at offset 0 in the config space (0x100)
        self.capacity = self.reg_read64(0x100);
    }

    pub fn read_sector(&self, sector: u64, buf: &mut [u8; 512]) -> bool {
        unsafe {
            (*self.request).type_ = VIRTIO_BLK_T_IN;
            (*self.request).reserved = 0;
            (*self.request).sector = sector;
            (*self.request).status = 0xFF;
        }

        // Put descriptor 0 in available ring
        unsafe {
            let avail = &mut *self.avail;
            let idx = avail.idx as usize % QUEUE_SIZE;
            avail.ring[idx] = 0;
            fence(Ordering::Release);
            avail.idx = avail.idx.wrapping_add(1);
        }

        // Notify device
        self.reg_write32(QUEUE_NOTIFY, 0);

        // Wait for used ring
        let last = unsafe { *self.last_used_idx.get() };
        loop {
            fence(Ordering::Acquire);
            let used_idx = unsafe { (*self.used).idx };
            if used_idx != last {
                unsafe { *self.last_used_idx.get() = used_idx; }
                break;
            }
        }

        unsafe {
            let status = (*self.request).status;
            if status == 0 {
                buf.copy_from_slice(&(*self.request).data);
                true
            } else {
                false
            }
        }
    }

    pub fn capacity_sectors(&self) -> u64 {
        self.capacity
    }
}

impl Driver for VirtioBlk {
    type Config = GenericDeviceConfig;
    type Error = DriverErr;

    fn get_config(&self) -> Self::Config {
        GenericDeviceConfig::new(self.base_addr, self.irq_num)
    }

    fn name(&self) -> &'static str {
        "VirtIO Block"
    }

    fn init(&self) -> DriverResult<()> {
        // init is called through &self, but we need &mut self for setup
        // The actual init is done in init_hw
        Ok(())
    }

    fn handle_irq(&self, irq_id: u32) -> bool {
        if irq_id != self.irq_num {
            return false;
        }
        // Acknowledge interrupt
        let status = self.reg_read32(INTERRUPT_STATUS);
        self.reg_write32(INTERRUPT_ACK, status);
        true
    }

    fn as_block_device(&self) -> Option<&'static kernel::driver::block::DynBlockDevice> {
        // Safety: VirtioBlk instances live in a static BLK_POOL, so self is 'static
        let fat: &kernel::driver::block::DynBlockDevice = self;
        Some(unsafe { core::mem::transmute::<&kernel::driver::block::DynBlockDevice, &'static kernel::driver::block::DynBlockDevice>(fat) })
    }
}

impl BlockDevice for VirtioBlk {
    fn block_count(&self) -> usize {
        self.capacity as usize
    }

    fn read_block(&self, block_id: usize, buf: &mut [u8]) -> DriverResult<()> {
        let sector = block_id as u64;
        let mut sector_buf = [0u8; 512];
        if !self.read_sector(sector, &mut sector_buf) {
            return Err(DriverErr::HardwareFault);
        }
        let copy_len = buf.len().min(512);
        buf[..copy_len].copy_from_slice(&sector_buf[..copy_len]);
        Ok(())
    }

    fn write_block(&self, _block_id: usize, _buf: &[u8]) -> DriverResult<()> {
        Err(DriverErr::NotSupported)
    }
}

impl VirtioBlk {
    pub fn init_hw(&mut self) -> DriverResult<()> {
        // Check magic
        if self.reg_read32(MAGIC) != 0x74726976 {
            return Err(DriverErr::InitFailed);
        }
        // Check version (1=legacy, 2=modern)
        let ver = self.reg_read32(VERSION);
        if ver != 1 && ver != 2 {
            kernel::kdebug!("VirtIO: unsupported version {}", ver);
            return Err(DriverErr::InitFailed);
        }
        // Check device ID (2 = block)
        if self.reg_read32(DEVICE_ID) != 2 {
            return Err(DriverErr::InitFailed);
        }

        // Reset
        self.reg_write32(STATUS, 0);
        // Acknowledge
        self.reg_write32(STATUS, STATUS_ACK);
        // Driver
        self.reg_write32(STATUS, STATUS_ACK | STATUS_DRIVER);
        // Features (no special features needed)
        self.reg_write32(DRIVER_FEATURES, 0);
        self.reg_write32(STATUS, STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK);

        // Setup queue
        if !self.setup_queue() {
            return Err(DriverErr::InitFailed);
        }

        // Driver OK
        self.reg_write32(STATUS, STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK);

        self.read_capacity();

        kernel::kinfo!("VirtIO Blk: {} sectors ({} MB)",
            self.capacity,
            self.capacity * 512 / 1024 / 1024
        );

        Ok(())
    }
}

/// FDT-compatible factory for VirtIO MMIO block devices.
/// Matches compatible = "virtio,mmio" and probes for block device (device_id=2).
pub struct VirtioBlkFactory;

struct BlkSlot(UnsafeCell<MaybeUninit<VirtioBlk>>);
unsafe impl Sync for BlkSlot {}

static BLK_POOL: [BlkSlot; 1] = [BlkSlot(UnsafeCell::new(MaybeUninit::uninit()))];
static BLK_IDX: AtomicUsize = AtomicUsize::new(0);

impl DriverFactory for VirtioBlkFactory {
    fn compatible(&self) -> &[&str] {
        &["virtio,mmio"]
    }

    fn probe(&self, base_addr: usize, irq: u32) -> Option<&'static dyn AnyDriver> {
        // Read device_id to check if this is a block device
        let device_id = unsafe { read_volatile((base_addr + 0x08) as *const u32) };
        if device_id != 2 {
            return None; // Not a block device
        }

        let idx = BLK_IDX.fetch_add(1, Ordering::Relaxed);
        if idx >= 1 {
            return None;
        }
        let slot = &BLK_POOL[idx];
        let driver = VirtioBlk::new(base_addr, irq);
        unsafe {
            (*slot.0.get()).write(driver);
            let driver_ref = &mut *(*slot.0.get()).as_mut_ptr();
            if driver_ref.init_hw().is_ok() {
                Some(driver_ref)
            } else {
                None
            }
        }
    }
}
