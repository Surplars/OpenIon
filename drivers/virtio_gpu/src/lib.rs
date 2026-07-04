#![no_std]

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering, fence};

use kernel::driver::framebuffer::{FramebufferDevice, FramebufferInfo, PixelFormat};
use kernel::driver::manager::AnyDriver;
use kernel::driver::{
    DeviceResource, Driver, DriverErr, DriverFactory, DriverResult, GenericDeviceConfig,
    StaticDriverPool,
};
use virtio_common::{
    DEVICE_ID_GPU, FeatureSet, MmioTransport, VirtqAvail, VirtqDesc, VirtqUsed, desc_flags, status,
};

const CONTROL_QUEUE_SIZE: usize = 8;
const CONTROL_QUEUE_INDEX: u32 = 0;
const RESOURCE_ID: u32 = 1;
const BYTES_PER_PIXEL: u32 = 4;
const MAX_WIDTH: u32 = 1024;
const MAX_HEIGHT: u32 = 768;
const FRAMEBUFFER_SIZE: usize = (MAX_WIDTH as usize) * (MAX_HEIGHT as usize) * 4;
const RESP_SIZE: usize = core::mem::size_of::<GpuDisplayInfo>();

const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const CMD_SET_SCANOUT: u32 = 0x0103;
const CMD_RESOURCE_FLUSH: u32 = 0x0104;
const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;

const RESP_OK_NODATA: u32 = 0x1100;
const RESP_OK_DISPLAY_INFO: u32 = 0x1101;
const GPU_FORMAT_B8G8R8A8_UNORM: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct GpuCtrlHdr {
    type_: u32,
    flags: u32,
    fence_id: u64,
    ctx_id: u32,
    ring_idx: u8,
    padding: [u8; 3],
}

impl GpuCtrlHdr {
    const fn new(type_: u32) -> Self {
        Self {
            type_,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            ring_idx: 0,
            padding: [0; 3],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GpuRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl GpuRect {
    const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[repr(C)]
struct GpuDisplayOne {
    r: GpuRect,
    enabled: u32,
    flags: u32,
}

#[repr(C)]
struct GpuDisplayInfo {
    hdr: GpuCtrlHdr,
    pmodes: [GpuDisplayOne; 16],
}

#[repr(C)]
struct ResourceCreate2d {
    hdr: GpuCtrlHdr,
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
struct ResourceAttachBacking {
    hdr: GpuCtrlHdr,
    resource_id: u32,
    nr_entries: u32,
    entry: MemEntry,
}

#[repr(C)]
struct MemEntry {
    addr: u64,
    length: u32,
    padding: u32,
}

#[repr(C)]
struct SetScanout {
    hdr: GpuCtrlHdr,
    r: GpuRect,
    scanout_id: u32,
    resource_id: u32,
}

#[repr(C)]
struct TransferToHost2d {
    hdr: GpuCtrlHdr,
    r: GpuRect,
    offset: u64,
    resource_id: u32,
    padding: u32,
}

#[repr(C)]
struct ResourceFlush {
    hdr: GpuCtrlHdr,
    r: GpuRect,
    resource_id: u32,
    padding: u32,
}

pub struct VirtioGpu {
    base_addr: usize,
    irq_num: u32,
    transport: MmioTransport,
    desc: *mut VirtqDesc,
    avail: *mut VirtqAvail<CONTROL_QUEUE_SIZE>,
    used: *mut VirtqUsed<CONTROL_QUEUE_SIZE>,
    request: *mut [u8; 256],
    response: *mut [u8; RESP_SIZE],
    framebuffer: *mut [u8; FRAMEBUFFER_SIZE],
    last_used_idx: UnsafeCell<u16>,
    io_busy: AtomicBool,
    width: u32,
    height: u32,
    scanout_id: u32,
}

unsafe impl Send for VirtioGpu {}
unsafe impl Sync for VirtioGpu {}

impl VirtioGpu {
    pub const fn new(base_addr: usize, irq_num: u32) -> Self {
        Self {
            base_addr,
            irq_num,
            transport: MmioTransport::new(base_addr),
            desc: core::ptr::null_mut(),
            avail: core::ptr::null_mut(),
            used: core::ptr::null_mut(),
            request: core::ptr::null_mut(),
            response: core::ptr::null_mut(),
            framebuffer: core::ptr::null_mut(),
            last_used_idx: UnsafeCell::new(0),
            io_busy: AtomicBool::new(false),
            width: 0,
            height: 0,
            scanout_id: 0,
        }
    }

    pub fn init_hw(&mut self) -> DriverResult<()> {
        let ver = self
            .transport
            .validate_device(
                DEVICE_ID_GPU,
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
            self.transport.fail();
            return Err(DriverErr::InitFailed);
        }

        let (scanout_id, width, height) = self.read_display_info()?;
        self.scanout_id = scanout_id;
        self.width = width.min(MAX_WIDTH);
        self.height = height.min(MAX_HEIGHT);
        if self.width == 0 || self.height == 0 {
            self.transport.fail();
            return Err(DriverErr::InvalidConfig);
        }

        self.create_framebuffer_resource()?;
        self.transport.set_status_bits(status::DRIVER_OK);
        self.clear(0xff00_0000)?;
        kernel::kinfo!(
            "virtio_gpu: {}x{} scanout {}",
            self.width,
            self.height,
            self.scanout_id
        );
        Ok(())
    }

    fn setup_queue(&mut self) -> bool {
        use core::cell::UnsafeCell;
        struct QueueMem(UnsafeCell<[u8; 12288]>);
        unsafe impl Sync for QueueMem {}
        static QUEUE_MEM: QueueMem = QueueMem(UnsafeCell::new([0u8; 12288]));

        struct FbMem(UnsafeCell<[u8; FRAMEBUFFER_SIZE]>);
        unsafe impl Sync for FbMem {}
        static FB_MEM: FbMem = FbMem(UnsafeCell::new([0u8; FRAMEBUFFER_SIZE]));

        let base = QUEUE_MEM.0.get() as usize;
        let aligned = (base + 4095) & !4095;
        self.desc = aligned as *mut VirtqDesc;
        self.avail = (aligned + CONTROL_QUEUE_SIZE * core::mem::size_of::<VirtqDesc>())
            as *mut VirtqAvail<CONTROL_QUEUE_SIZE>;
        self.used = (aligned + 4096) as *mut VirtqUsed<CONTROL_QUEUE_SIZE>;
        self.request = (aligned + 8192) as *mut [u8; 256];
        self.response = (aligned + 8192 + 256) as *mut [u8; RESP_SIZE];
        self.framebuffer = FB_MEM.0.get();

        unsafe {
            core::ptr::write_bytes(aligned as *mut u8, 0, 12288);
            core::ptr::write_bytes((*self.framebuffer).as_mut_ptr(), 0, FRAMEBUFFER_SIZE);
            *self.last_used_idx.get() = 0;
        }

        self.transport
            .setup_queue(
                CONTROL_QUEUE_INDEX,
                CONTROL_QUEUE_SIZE,
                self.desc as u64,
                self.avail as u64,
                self.used as u64,
            )
            .is_ok()
    }

    fn read_display_info(&self) -> DriverResult<(u32, u32, u32)> {
        self.send_command(
            &GpuCtrlHdr::new(CMD_GET_DISPLAY_INFO),
            RESP_OK_DISPLAY_INFO,
            RESP_SIZE,
        )?;

        let info = unsafe { &*(self.response as *const GpuDisplayInfo) };
        for i in 0..info.pmodes.len() {
            let mode = &info.pmodes[i];
            if mode.enabled != 0 && mode.r.width != 0 && mode.r.height != 0 {
                return Ok((i as u32, mode.r.width, mode.r.height));
            }
        }
        Ok((0, MAX_WIDTH, MAX_HEIGHT))
    }

    fn create_framebuffer_resource(&self) -> DriverResult<()> {
        let rect = self.full_rect();
        let create = ResourceCreate2d {
            hdr: GpuCtrlHdr::new(CMD_RESOURCE_CREATE_2D),
            resource_id: RESOURCE_ID,
            format: GPU_FORMAT_B8G8R8A8_UNORM,
            width: self.width,
            height: self.height,
        };
        self.send_command(&create, RESP_OK_NODATA, core::mem::size_of::<GpuCtrlHdr>())?;

        let backing = ResourceAttachBacking {
            hdr: GpuCtrlHdr::new(CMD_RESOURCE_ATTACH_BACKING),
            resource_id: RESOURCE_ID,
            nr_entries: 1,
            entry: MemEntry {
                addr: self.framebuffer as u64,
                length: self.framebuffer_len() as u32,
                padding: 0,
            },
        };
        self.send_command(&backing, RESP_OK_NODATA, core::mem::size_of::<GpuCtrlHdr>())?;

        let scanout = SetScanout {
            hdr: GpuCtrlHdr::new(CMD_SET_SCANOUT),
            r: rect,
            scanout_id: self.scanout_id,
            resource_id: RESOURCE_ID,
        };
        self.send_command(&scanout, RESP_OK_NODATA, core::mem::size_of::<GpuCtrlHdr>())?;
        self.flush_rect(rect)
    }

    fn flush_rect(&self, rect: GpuRect) -> DriverResult<()> {
        let transfer = TransferToHost2d {
            hdr: GpuCtrlHdr::new(CMD_TRANSFER_TO_HOST_2D),
            r: rect,
            offset: 0,
            resource_id: RESOURCE_ID,
            padding: 0,
        };
        self.send_command(
            &transfer,
            RESP_OK_NODATA,
            core::mem::size_of::<GpuCtrlHdr>(),
        )?;

        let flush = ResourceFlush {
            hdr: GpuCtrlHdr::new(CMD_RESOURCE_FLUSH),
            r: rect,
            resource_id: RESOURCE_ID,
            padding: 0,
        };
        self.send_command(&flush, RESP_OK_NODATA, core::mem::size_of::<GpuCtrlHdr>())
    }

    fn clear(&self, bgra: u32) -> DriverResult<()> {
        self.fill_rect(0, 0, self.width, self.height, bgra)
    }

    fn send_command<T>(
        &self,
        cmd: &T,
        expected_type: u32,
        response_len: usize,
    ) -> DriverResult<()> {
        if self.io_busy.swap(true, Ordering::Acquire) {
            return Err(DriverErr::Busy);
        }
        let result = self.send_command_locked(cmd, expected_type, response_len);
        self.io_busy.store(false, Ordering::Release);
        result
    }

    fn send_command_locked<T>(
        &self,
        cmd: &T,
        expected_type: u32,
        response_len: usize,
    ) -> DriverResult<()> {
        let request_len = core::mem::size_of::<T>();
        if request_len > 256 || response_len > RESP_SIZE {
            return Err(DriverErr::InvalidConfig);
        }

        unsafe {
            core::ptr::write_bytes((*self.request).as_mut_ptr(), 0, 256);
            core::ptr::write_bytes((*self.response).as_mut_ptr(), 0, RESP_SIZE);
            core::ptr::copy_nonoverlapping(
                cmd as *const T as *const u8,
                (*self.request).as_mut_ptr(),
                request_len,
            );

            core::ptr::write_volatile(&mut (*self.desc).addr, self.request as u64);
            core::ptr::write_volatile(&mut (*self.desc).len, request_len as u32);
            core::ptr::write_volatile(&mut (*self.desc).flags, desc_flags::NEXT);
            core::ptr::write_volatile(&mut (*self.desc).next, 1);

            core::ptr::write_volatile(&mut (*self.desc.add(1)).addr, self.response as u64);
            core::ptr::write_volatile(&mut (*self.desc.add(1)).len, response_len as u32);
            core::ptr::write_volatile(&mut (*self.desc.add(1)).flags, desc_flags::WRITE);
            core::ptr::write_volatile(&mut (*self.desc.add(1)).next, 0);

            let avail = &mut *self.avail;
            let avail_idx = core::ptr::read_volatile(&avail.idx);
            let idx = avail_idx as usize % CONTROL_QUEUE_SIZE;
            core::ptr::write_volatile(&mut avail.ring[idx], 0);
            fence(Ordering::Release);
            core::ptr::write_volatile(&mut avail.idx, avail_idx.wrapping_add(1));
        }

        self.transport.notify_queue(CONTROL_QUEUE_INDEX);

        let last = unsafe { *self.last_used_idx.get() };
        let mut spins = 0usize;
        while spins < kernel::generated_config::OPENION_VIRTIO_GPU_POLL_LIMIT {
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

        if spins >= kernel::generated_config::OPENION_VIRTIO_GPU_POLL_LIMIT {
            return Err(DriverErr::Timeout);
        }

        self.transport.ack_interrupts();
        let hdr = unsafe { &*(self.response as *const GpuCtrlHdr) };
        if hdr.type_ == expected_type {
            Ok(())
        } else {
            Err(DriverErr::HardwareFault)
        }
    }

    fn framebuffer_len(&self) -> usize {
        (self.width as usize) * (self.height as usize) * (BYTES_PER_PIXEL as usize)
    }

    fn full_rect(&self) -> GpuRect {
        GpuRect::new(0, 0, self.width, self.height)
    }
}

impl Driver for VirtioGpu {
    type Config = GenericDeviceConfig;
    type Error = DriverErr;

    fn get_config(&self) -> Self::Config {
        GenericDeviceConfig::new(self.base_addr, self.irq_num)
    }

    fn name(&self) -> &'static str {
        "virtio_gpu"
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

    fn as_framebuffer_device(
        &self,
    ) -> Option<&'static kernel::driver::framebuffer::DynFramebufferDevice> {
        let fb: &kernel::driver::framebuffer::DynFramebufferDevice = self;
        Some(unsafe {
            core::mem::transmute::<
                &kernel::driver::framebuffer::DynFramebufferDevice,
                &'static kernel::driver::framebuffer::DynFramebufferDevice,
            >(fb)
        })
    }
}

impl FramebufferDevice for VirtioGpu {
    fn info(&self) -> Option<FramebufferInfo> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        Some(FramebufferInfo {
            width: self.width,
            height: self.height,
            stride_bytes: self.width * BYTES_PER_PIXEL,
            format: PixelFormat::Bgra8888,
        })
    }

    fn write_framebuffer(&self, offset: usize, data: &[u8]) -> DriverResult<usize> {
        let len = self.framebuffer_len();
        if offset >= len {
            return Err(DriverErr::InvalidConfig);
        }
        let n = data.len().min(len - offset);
        unsafe {
            (*self.framebuffer).as_mut_slice()[offset..offset + n].copy_from_slice(&data[..n]);
        }
        Ok(n)
    }

    fn fill_rect(&self, x: u32, y: u32, width: u32, height: u32, bgra: u32) -> DriverResult<()> {
        if x >= self.width || y >= self.height {
            return Err(DriverErr::InvalidConfig);
        }
        let end_x = x.saturating_add(width).min(self.width);
        let end_y = y.saturating_add(height).min(self.height);
        let color = bgra.to_le_bytes();
        let stride = (self.width * BYTES_PER_PIXEL) as usize;
        unsafe {
            let fb = (*self.framebuffer).as_mut_slice();
            for py in y..end_y {
                let row = py as usize * stride;
                for px in x..end_x {
                    let off = row + px as usize * 4;
                    fb[off..off + 4].copy_from_slice(&color);
                }
            }
        }
        self.flush_rect(GpuRect::new(x, y, end_x - x, end_y - y))
    }

    fn flush(&self) -> DriverResult<()> {
        self.flush_rect(self.full_rect())
    }
}

pub struct VirtioGpuFactory;

static GPU_POOL: StaticDriverPool<VirtioGpu, 1> = StaticDriverPool::new();

impl DriverFactory for VirtioGpuFactory {
    fn compatible(&self) -> &[&str] {
        &["virtio,mmio"]
    }

    fn probe(&self, resource: DeviceResource) -> Option<&'static dyn AnyDriver> {
        let transport = MmioTransport::new(resource.base_addr);
        if transport.device_id() != DEVICE_ID_GPU {
            return None;
        }

        let driver = GPU_POOL.alloc(VirtioGpu::new(resource.base_addr, resource.irq))?;
        if driver.init_hw().is_ok() {
            Some(driver as _)
        } else {
            None
        }
    }
}
