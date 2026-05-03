use super::{Driver, DriverErr, DriverResult, GenericDeviceConfig};

/// Trait for fixed-block-size random-access storage (SD card, disk, flash).
pub trait BlockDevice: Driver {
    /// Block/sector size in bytes (typically 512 or 4096)
    fn block_size(&self) -> usize {
        512
    }

    /// Total number of blocks in the device
    fn block_count(&self) -> usize;

    /// Read a block into the buffer.
    /// `buf.len()` must equal or be a multiple of `block_size()`.
    fn read_block(&self, block_id: usize, buf: &mut [u8]) -> DriverResult<()>;

    /// Write buffer data to a block.
    /// `buf.len()` must equal or be a multiple of `block_size()`.
    fn write_block(&self, block_id: usize, buf: &[u8]) -> DriverResult<()>;
}

/// Object-safe trait object for BlockDevice with concrete associated types.
pub type DynBlockDevice = dyn BlockDevice<Config = GenericDeviceConfig, Error = DriverErr>;
