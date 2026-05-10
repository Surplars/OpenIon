#![no_std]

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{Ordering, fence};

pub const MMIO_MAGIC_VALUE: u32 = 0x7472_6976;
pub const MMIO_VERSION_LEGACY: u32 = 1;
pub const MMIO_VERSION_MODERN: u32 = 2;

pub const DEVICE_ID_BLOCK: u32 = 2;
pub const DEVICE_ID_CONSOLE: u32 = 3;
pub const DEVICE_ID_NET: u32 = 1;
pub const DEVICE_ID_RNG: u32 = 4;
pub const DEVICE_ID_GPU: u32 = 16;

pub const VIRTIO_F_VERSION_1_BIT: u32 = 32;

pub mod status {
    pub const ACKNOWLEDGE: u32 = 1;
    pub const DRIVER: u32 = 2;
    pub const DRIVER_OK: u32 = 4;
    pub const FEATURES_OK: u32 = 8;
    pub const FAILED: u32 = 128;
}

pub mod desc_flags {
    pub const NEXT: u16 = 1;
    pub const WRITE: u16 = 2;
}

mod regs {
    pub const MAGIC: usize = 0x000;
    pub const VERSION: usize = 0x004;
    pub const DEVICE_ID: usize = 0x008;
    pub const DEVICE_FEATURES: usize = 0x010;
    pub const DEVICE_FEATURES_SEL: usize = 0x014;
    pub const DRIVER_FEATURES: usize = 0x020;
    pub const DRIVER_FEATURES_SEL: usize = 0x024;
    pub const QUEUE_SEL: usize = 0x030;
    pub const QUEUE_NUM_MAX: usize = 0x034;
    pub const QUEUE_NUM: usize = 0x038;
    pub const QUEUE_READY: usize = 0x044;
    pub const QUEUE_NOTIFY: usize = 0x050;
    pub const INTERRUPT_STATUS: usize = 0x060;
    pub const INTERRUPT_ACK: usize = 0x064;
    pub const STATUS: usize = 0x070;
    pub const QUEUE_DESC_LOW: usize = 0x080;
    pub const QUEUE_DRIVER_LOW: usize = 0x090;
    pub const QUEUE_DEVICE_LOW: usize = 0x0a0;
}

#[repr(C)]
pub struct VirtqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

#[repr(C)]
pub struct VirtqAvail<const N: usize> {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; N],
}

#[repr(C)]
pub struct VirtqUsedElem {
    pub id: u32,
    pub len: u32,
}

#[repr(C)]
pub struct VirtqUsed<const N: usize> {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VirtqUsedElem; N],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtioError {
    BadMagic,
    UnsupportedVersion,
    WrongDevice,
    LegacyDisabled,
    MissingModernFeature,
    FeaturesRejected,
    QueueUnavailable,
    Timeout,
    DeviceError,
}

#[derive(Clone, Copy)]
pub struct FeatureSet {
    pub lo: u32,
    pub hi: u32,
}

impl FeatureSet {
    pub const fn empty() -> Self {
        Self { lo: 0, hi: 0 }
    }

    pub const fn with_modern() -> Self {
        Self {
            lo: 0,
            hi: 1u32 << (VIRTIO_F_VERSION_1_BIT - 32),
        }
    }
}

#[derive(Clone, Copy)]
pub struct MmioTransport {
    base_addr: usize,
}

impl MmioTransport {
    pub const fn new(base_addr: usize) -> Self {
        Self { base_addr }
    }

    pub const fn base_addr(&self) -> usize {
        self.base_addr
    }

    pub fn magic(&self) -> u32 {
        self.read32(regs::MAGIC)
    }

    pub fn version(&self) -> u32 {
        self.read32(regs::VERSION)
    }

    pub fn device_id(&self) -> u32 {
        self.read32(regs::DEVICE_ID)
    }

    pub fn read_config64(&self, offset: usize) -> u64 {
        let lo = self.read32(0x100 + offset) as u64;
        let hi = self.read32(0x100 + offset + 4) as u64;
        (hi << 32) | lo
    }

    pub fn read_config32(&self, offset: usize) -> u32 {
        self.read32(0x100 + offset)
    }

    pub fn validate_device(
        &self,
        expected_device_id: u32,
        allow_legacy: bool,
    ) -> Result<u32, VirtioError> {
        if self.magic() != MMIO_MAGIC_VALUE {
            return Err(VirtioError::BadMagic);
        }

        let version = self.version();
        if version != MMIO_VERSION_LEGACY && version != MMIO_VERSION_MODERN {
            return Err(VirtioError::UnsupportedVersion);
        }
        if version == MMIO_VERSION_LEGACY && !allow_legacy {
            return Err(VirtioError::LegacyDisabled);
        }
        if self.device_id() != expected_device_id {
            return Err(VirtioError::WrongDevice);
        }
        Ok(version)
    }

    pub fn reset(&self) {
        self.write32(regs::STATUS, 0);
        fence(Ordering::SeqCst);
    }

    pub fn set_status_bits(&self, bits: u32) {
        let status = self.read32(regs::STATUS);
        self.write32(regs::STATUS, status | bits);
    }

    pub fn fail(&self) {
        self.set_status_bits(status::FAILED);
    }

    pub fn negotiate_features(
        &self,
        version: u32,
        requested: FeatureSet,
    ) -> Result<FeatureSet, VirtioError> {
        let device = self.device_features();
        let mut selected = FeatureSet {
            lo: device.lo & requested.lo,
            hi: device.hi & requested.hi,
        };

        if version == MMIO_VERSION_MODERN {
            let modern = FeatureSet::with_modern().hi;
            if device.hi & modern == 0 {
                return Err(VirtioError::MissingModernFeature);
            }
            selected.hi |= modern;
        }

        self.write_driver_features(selected);
        self.set_status_bits(status::FEATURES_OK);
        if self.read32(regs::STATUS) & status::FEATURES_OK == 0 {
            return Err(VirtioError::FeaturesRejected);
        }
        Ok(selected)
    }

    pub fn select_queue(&self, queue: u32) {
        self.write32(regs::QUEUE_SEL, queue);
    }

    pub fn setup_queue(
        &self,
        queue: u32,
        queue_size: usize,
        desc_addr: u64,
        avail_addr: u64,
        used_addr: u64,
    ) -> Result<(), VirtioError> {
        self.select_queue(queue);
        let max = self.read32(regs::QUEUE_NUM_MAX);
        if max == 0 || (max as usize) < queue_size {
            return Err(VirtioError::QueueUnavailable);
        }

        self.write32(regs::QUEUE_NUM, queue_size as u32);
        self.write64_pair(regs::QUEUE_DESC_LOW, desc_addr);
        self.write64_pair(regs::QUEUE_DRIVER_LOW, avail_addr);
        self.write64_pair(regs::QUEUE_DEVICE_LOW, used_addr);
        self.write32(regs::QUEUE_READY, 1);
        Ok(())
    }

    pub fn notify_queue(&self, queue: u32) {
        self.write32(regs::QUEUE_NOTIFY, queue);
    }

    pub fn ack_interrupts(&self) -> u32 {
        let status = self.read32(regs::INTERRUPT_STATUS);
        if status != 0 {
            self.write32(regs::INTERRUPT_ACK, status);
        }
        status
    }

    fn device_features(&self) -> FeatureSet {
        self.write32(regs::DEVICE_FEATURES_SEL, 0);
        let lo = self.read32(regs::DEVICE_FEATURES);
        self.write32(regs::DEVICE_FEATURES_SEL, 1);
        let hi = self.read32(regs::DEVICE_FEATURES);
        FeatureSet { lo, hi }
    }

    fn write_driver_features(&self, features: FeatureSet) {
        self.write32(regs::DRIVER_FEATURES_SEL, 0);
        self.write32(regs::DRIVER_FEATURES, features.lo);
        self.write32(regs::DRIVER_FEATURES_SEL, 1);
        self.write32(regs::DRIVER_FEATURES, features.hi);
    }

    fn read32(&self, offset: usize) -> u32 {
        unsafe { read_volatile((self.base_addr + offset) as *const u32) }
    }

    fn write32(&self, offset: usize, val: u32) {
        unsafe { write_volatile((self.base_addr + offset) as *mut u32, val) }
    }

    fn write64_pair(&self, offset: usize, value: u64) {
        self.write32(offset, value as u32);
        self.write32(offset + 4, (value >> 32) as u32);
    }
}
