#![no_std]

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering, fence};

use kernel::driver::manager::AnyDriver;
use kernel::driver::rng::RngDevice;
use kernel::driver::{
    DeviceResource, Driver, DriverErr, DriverFactory, DriverResult, GenericDeviceConfig,
    StaticDriverPool,
};
use virtio_common::{
    DEVICE_ID_RNG, FeatureSet, MmioTransport, VirtqAvail, VirtqDesc, VirtqUsed, desc_flags, status,
};

const QUEUE_SIZE: usize = 8;
const QUEUE_INDEX: u32 = 0;
const RNG_BUF_SIZE: usize = 256;

pub struct VirtioRng {
    base_addr: usize,
    irq_num: u32,
    transport: MmioTransport,
    desc: *mut VirtqDesc,
    avail: *mut VirtqAvail<QUEUE_SIZE>,
    used: *mut VirtqUsed<QUEUE_SIZE>,
    entropy_buf: *mut [u8; RNG_BUF_SIZE],
    last_used_idx: UnsafeCell<u16>,
    io_busy: AtomicBool,
}

unsafe impl Send for VirtioRng {}
unsafe impl Sync for VirtioRng {}

impl VirtioRng {
    pub const fn new(base_addr: usize, irq_num: u32) -> Self {
        Self {
            base_addr,
            irq_num,
            transport: MmioTransport::new(base_addr),
            desc: core::ptr::null_mut(),
            avail: core::ptr::null_mut(),
            used: core::ptr::null_mut(),
            entropy_buf: core::ptr::null_mut(),
            last_used_idx: UnsafeCell::new(0),
            io_busy: AtomicBool::new(false),
        }
    }

    pub fn init_hw(&mut self) -> DriverResult<()> {
        let ver = self
            .transport
            .validate_device(
                DEVICE_ID_RNG,
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

        if !self.setup_queue() {
            return Err(DriverErr::InitFailed);
        }

        self.transport.set_status_bits(status::DRIVER_OK);
        kernel::kinfo!("virtio_rng: initialized");
        Ok(())
    }

    fn setup_queue(&mut self) -> bool {
        use core::cell::UnsafeCell;
        struct QueueMem(UnsafeCell<[u8; 8192]>);
        unsafe impl Sync for QueueMem {}
        static QUEUE_MEM: QueueMem = QueueMem(UnsafeCell::new([0u8; 8192]));

        let base = QUEUE_MEM.0.get() as usize;
        let aligned = (base + 4095) & !4095;

        self.desc = aligned as *mut VirtqDesc;
        self.avail = (aligned + QUEUE_SIZE * core::mem::size_of::<VirtqDesc>())
            as *mut VirtqAvail<QUEUE_SIZE>;
        self.used = (aligned + 4096) as *mut VirtqUsed<QUEUE_SIZE>;
        let entropy_off = (core::mem::size_of::<VirtqUsed<QUEUE_SIZE>>() + 7) & !7;
        self.entropy_buf = (aligned + 4096 + entropy_off) as *mut [u8; RNG_BUF_SIZE];

        unsafe {
            core::ptr::write_bytes(aligned as *mut u8, 0, 4096 + entropy_off + RNG_BUF_SIZE);
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

        unsafe {
            core::ptr::write_volatile(&mut (*self.desc).addr, self.entropy_buf as u64);
            core::ptr::write_volatile(&mut (*self.desc).len, RNG_BUF_SIZE as u32);
            core::ptr::write_volatile(&mut (*self.desc).flags, desc_flags::WRITE);
            core::ptr::write_volatile(&mut (*self.desc).next, 0);
            *self.last_used_idx.get() = 0;
        }

        true
    }
}

impl Driver for VirtioRng {
    type Config = GenericDeviceConfig;
    type Error = DriverErr;

    fn get_config(&self) -> Self::Config {
        GenericDeviceConfig::new(self.base_addr, self.irq_num)
    }

    fn name(&self) -> &'static str {
        "virtio_rng"
    }

    fn init(&self) -> DriverResult<()> {
        Ok(())
    }

    fn handle_irq(&self, irq_id: u32) -> bool {
        if irq_id != self.irq_num {
            return false;
        }
        self.transport.ack_interrupts();
        true
    }

    fn as_rng_device(&self) -> Option<&'static kernel::driver::rng::DynRngDevice> {
        let rng: &kernel::driver::rng::DynRngDevice = self;
        Some(unsafe {
            core::mem::transmute::<
                &kernel::driver::rng::DynRngDevice,
                &'static kernel::driver::rng::DynRngDevice,
            >(rng)
        })
    }
}

impl RngDevice for VirtioRng {
    fn fill_bytes(&self, buf: &mut [u8]) -> DriverResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.io_busy.swap(true, Ordering::Acquire) {
            return Err(DriverErr::Busy);
        }

        let result = self.fill_bytes_locked(buf);
        self.io_busy.store(false, Ordering::Release);
        result
    }
}

impl VirtioRng {
    fn fill_bytes_locked(&self, buf: &mut [u8]) -> DriverResult<usize> {
        let want = buf.len().min(RNG_BUF_SIZE);
        unsafe {
            core::ptr::write_bytes((*self.entropy_buf).as_mut_ptr(), 0, RNG_BUF_SIZE);
            core::ptr::write_volatile(&mut (*self.desc).len, want as u32);

            let avail = &mut *self.avail;
            let avail_idx = core::ptr::read_volatile(&avail.idx);
            let idx = avail_idx as usize % QUEUE_SIZE;
            core::ptr::write_volatile(&mut avail.ring[idx], 0);
            fence(Ordering::Release);
            core::ptr::write_volatile(&mut avail.idx, avail_idx.wrapping_add(1));
        }

        self.transport.notify_queue(QUEUE_INDEX);

        let last = unsafe { *self.last_used_idx.get() };
        let mut spins = 0usize;
        let mut used_len = 0usize;
        while spins < kernel::generated_config::OPENION_VIRTIO_BLK_POLL_LIMIT {
            fence(Ordering::Acquire);
            let used_idx = unsafe { core::ptr::read_volatile(&(*self.used).idx) };
            if used_idx != last {
                let ring_idx = last as usize % QUEUE_SIZE;
                used_len =
                    unsafe { core::ptr::read_volatile(&(*self.used).ring[ring_idx].len) as usize };
                unsafe {
                    *self.last_used_idx.get() = used_idx;
                }
                break;
            }
            spins += 1;
            core::hint::spin_loop();
        }

        if spins >= kernel::generated_config::OPENION_VIRTIO_BLK_POLL_LIMIT {
            return Err(DriverErr::Timeout);
        }

        self.transport.ack_interrupts();
        let n = used_len.min(want);
        unsafe {
            let entropy = &*self.entropy_buf;
            buf[..n].copy_from_slice(&entropy[..n]);
        }
        Ok(n)
    }
}

pub struct VirtioRngFactory;

static RNG_POOL: StaticDriverPool<VirtioRng, 1> = StaticDriverPool::new();

impl DriverFactory for VirtioRngFactory {
    fn compatible(&self) -> &[&str] {
        &["virtio,mmio"]
    }

    fn probe(&self, resource: DeviceResource) -> Option<&'static dyn AnyDriver> {
        let transport = MmioTransport::new(resource.base_addr);
        if transport.device_id() != DEVICE_ID_RNG {
            return None;
        }

        let driver = RNG_POOL.alloc(VirtioRng::new(resource.base_addr, resource.irq))?;
        if driver.init_hw().is_ok() {
            Some(driver as _)
        } else {
            None
        }
    }
}
