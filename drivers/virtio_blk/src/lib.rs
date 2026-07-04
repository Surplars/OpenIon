#![no_std]

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering, fence};

use kernel::driver::block::BlockDevice;
use kernel::driver::manager::AnyDriver;
use kernel::driver::{
    DeviceResource, Driver, DriverErr, DriverFactory, DriverResult, GenericDeviceConfig,
    StaticDriverPool,
};
use virtio_common::{
    DEVICE_ID_BLOCK, FeatureSet, MmioTransport, VirtqAvail, VirtqDesc, VirtqUsed, desc_flags,
    status,
};

// VirtIO block device request types
const VIRTIO_BLK_T_IN: u32 = 0;

const QUEUE_SIZE: usize = 16;
const QUEUE_INDEX: u32 = 0;

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
    transport: MmioTransport,
    capacity: u64,
    // Virtqueue memory (must be contiguous and aligned)
    desc: *mut VirtqDesc,
    avail: *mut VirtqAvail<QUEUE_SIZE>,
    used: *mut VirtqUsed<QUEUE_SIZE>,
    request: *mut BlkRequest,
    free_head: u16,
    last_used_idx: UnsafeCell<u16>,
    io_busy: AtomicBool,
}

// Safety: accessed only through Driver trait methods with proper synchronization
unsafe impl Send for VirtioBlk {}
unsafe impl Sync for VirtioBlk {}

impl VirtioBlk {
    pub const fn new(base_addr: usize, irq_num: u32) -> Self {
        Self {
            base_addr,
            irq_num,
            transport: MmioTransport::new(base_addr),
            capacity: 0,
            desc: core::ptr::null_mut(),
            avail: core::ptr::null_mut(),
            used: core::ptr::null_mut(),
            request: core::ptr::null_mut(),
            free_head: 0,
            last_used_idx: UnsafeCell::new(0),
            io_busy: AtomicBool::new(false),
        }
    }

    fn setup_queue(&mut self) -> bool {
        // Allocate aligned memory for virtqueue structures + request buffer
        // Needs: desc(256) + avail(~36) + page_gap + used(~132) + request(529+align)
        // Worst case: 4095(align) + 4096 + 136 + 529 = 8856 bytes → use 12KB (3 pages)
        use core::cell::UnsafeCell;
        struct QueueMem(UnsafeCell<[u8; 16384]>);
        unsafe impl Sync for QueueMem {}
        static QUEUE_MEM: QueueMem = QueueMem(UnsafeCell::new([0u8; 16384]));
        let base = QUEUE_MEM.0.get() as usize;
        let aligned = (base + 4095) & !4095;

        self.desc = aligned as *mut VirtqDesc;
        self.avail = (aligned + QUEUE_SIZE * 16) as *mut VirtqAvail<QUEUE_SIZE>;
        self.used = (aligned + 4096) as *mut VirtqUsed<QUEUE_SIZE>;
        // BlkRequest.sector is u64 (needs 8-byte align); VirtqUsed is 132 bytes (not 8-aligned)
        let request_off = (core::mem::size_of::<VirtqUsed<QUEUE_SIZE>>() + 7) & !7;
        self.request = (aligned + 4096 + request_off) as *mut BlkRequest;

        // Zero out queue memory
        unsafe {
            core::ptr::write_bytes(aligned as *mut u8, 0, 12288);
        }

        if self
            .transport
            .setup_queue(
                QUEUE_INDEX,
                QUEUE_SIZE,
                self.desc as u64,
                self.avail as u64,
                self.used as u64,
            )
            .is_err()
        {
            return false;
        }

        // Set up descriptor chain: [header, data, status]
        unsafe {
            // Descriptor 0: header (BlkRequest type + sector)
            core::ptr::write_volatile(&mut (*self.desc).addr, self.request as u64);
            core::ptr::write_volatile(&mut (*self.desc).len, 16); // type(4) + reserved(4) + sector(8)
            core::ptr::write_volatile(&mut (*self.desc).flags, desc_flags::NEXT);
            core::ptr::write_volatile(&mut (*self.desc).next, 1);

            // Descriptor 1: data (512 bytes) — device-writable
            core::ptr::write_volatile(
                &mut (*self.desc.add(1)).addr,
                (self.request as usize + 16) as u64,
            );
            core::ptr::write_volatile(&mut (*self.desc.add(1)).len, 512);
            core::ptr::write_volatile(
                &mut (*self.desc.add(1)).flags,
                desc_flags::NEXT | desc_flags::WRITE,
            );
            core::ptr::write_volatile(&mut (*self.desc.add(1)).next, 2);

            // Descriptor 2: status (1 byte)
            core::ptr::write_volatile(
                &mut (*self.desc.add(2)).addr,
                (self.request as usize + 16 + 512) as u64,
            );
            core::ptr::write_volatile(&mut (*self.desc.add(2)).len, 1);
            core::ptr::write_volatile(&mut (*self.desc.add(2)).flags, desc_flags::WRITE);

            self.free_head = 0;
            *self.last_used_idx.get() = 0;
        }

        true
    }

    fn read_capacity(&mut self) {
        self.capacity = self.transport.read_config64(0);
    }

    pub fn read_sector(&self, sector: u64, buf: &mut [u8; 512]) -> bool {
        if self.io_busy.swap(true, Ordering::Acquire) {
            return false;
        }

        let result = self.read_sector_locked(sector, buf);
        self.io_busy.store(false, Ordering::Release);
        result
    }

    fn read_sector_locked(&self, sector: u64, buf: &mut [u8; 512]) -> bool {
        unsafe {
            core::ptr::write_volatile(&mut (*self.request).type_, VIRTIO_BLK_T_IN);
            core::ptr::write_volatile(&mut (*self.request).reserved, 0);
            core::ptr::write_volatile(&mut (*self.request).sector, sector);
            core::ptr::write_volatile(&mut (*self.request).status, 0xFF);
        }

        // Put descriptor 0 in available ring
        unsafe {
            let avail = &mut *self.avail;
            let avail_idx = core::ptr::read_volatile(&avail.idx);
            let idx = avail_idx as usize % QUEUE_SIZE;
            core::ptr::write_volatile(&mut avail.ring[idx], 0);
            fence(Ordering::Release);
            core::ptr::write_volatile(&mut avail.idx, avail_idx.wrapping_add(1));
        }

        // Notify device
        self.transport.notify_queue(QUEUE_INDEX);

        // Wait for used ring
        let last = unsafe { *self.last_used_idx.get() };
        let mut spins = 0usize;
        while spins < kernel::generated_config::OPENION_VIRTIO_BLK_POLL_LIMIT {
            fence(Ordering::Acquire);
            let used_idx = unsafe { core::ptr::read_volatile(&(*self.used).idx) };
            if used_idx != last {
                unsafe {
                    *self.last_used_idx.get() = used_idx;
                }
                break;
            }
            spins += 1;
            core::hint::spin_loop();
        }

        if spins >= kernel::generated_config::OPENION_VIRTIO_BLK_POLL_LIMIT {
            return false;
        }

        self.transport.ack_interrupts();

        unsafe {
            let status = core::ptr::read_volatile(&(*self.request).status);
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
        "virtio_blk"
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
        self.transport.ack_interrupts();
        true
    }

    fn as_block_device(&self) -> Option<&'static kernel::driver::block::DynBlockDevice> {
        // Safety: VirtioBlk instances live in a static BLK_POOL, so self is 'static
        let fat: &kernel::driver::block::DynBlockDevice = self;
        Some(unsafe {
            core::mem::transmute::<
                &kernel::driver::block::DynBlockDevice,
                &'static kernel::driver::block::DynBlockDevice,
            >(fat)
        })
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
        let ver = self
            .transport
            .validate_device(
                DEVICE_ID_BLOCK,
                kernel::generated_config::OPENION_VIRTIO_MMIO_LEGACY,
            )
            .map_err(|err| match err {
                virtio_common::VirtioError::LegacyDisabled
                | virtio_common::VirtioError::MissingModernFeature
                | virtio_common::VirtioError::FeaturesRejected => DriverErr::InvalidConfig,
                _ => DriverErr::InitFailed,
            })?;

        self.transport.reset();
        self.transport.set_status_bits(status::ACKNOWLEDGE);
        self.transport.set_status_bits(status::DRIVER);
        if self
            .transport
            .negotiate_features(ver, FeatureSet::empty())
            .is_err()
        {
            self.transport.fail();
            return Err(DriverErr::InvalidConfig);
        }

        // Setup queue
        if !self.setup_queue() {
            return Err(DriverErr::InitFailed);
        }

        // Driver OK
        self.transport.set_status_bits(status::DRIVER_OK);

        self.read_capacity();

        kernel::kinfo!(
            "virtio_blk: {} sectors ({} MiB)",
            self.capacity,
            self.capacity * 512 / 1024 / 1024
        );

        Ok(())
    }
}

/// FDT-compatible factory for VirtIO MMIO block devices.
/// Matches compatible = "virtio,mmio" and probes for block device (device_id=2).
pub struct VirtioBlkFactory;

static BLK_POOL: StaticDriverPool<VirtioBlk, 1> = StaticDriverPool::new();

impl DriverFactory for VirtioBlkFactory {
    fn compatible(&self) -> &[&str] {
        &["virtio,mmio"]
    }

    fn probe(&self, resource: DeviceResource) -> Option<&'static dyn AnyDriver> {
        let transport = MmioTransport::new(resource.base_addr);
        if transport.device_id() != DEVICE_ID_BLOCK {
            return None;
        }

        let driver = BLK_POOL.alloc(VirtioBlk::new(resource.base_addr, resource.irq))?;
        if driver.init_hw().is_ok() {
            Some(driver as _)
        } else {
            None
        }
    }
}
