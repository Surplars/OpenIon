use super::{Driver, DriverErr, DriverResult, GenericDeviceConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra8888,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramebufferInfo {
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub format: PixelFormat,
}

pub trait FramebufferDevice: Driver {
    fn info(&self) -> Option<FramebufferInfo>;
    fn write_framebuffer(&self, offset: usize, data: &[u8]) -> DriverResult<usize>;
    fn fill_rect(&self, x: u32, y: u32, width: u32, height: u32, bgra: u32) -> DriverResult<()>;
    fn flush(&self) -> DriverResult<()>;
}

pub type DynFramebufferDevice =
    dyn FramebufferDevice<Config = GenericDeviceConfig, Error = DriverErr>;
