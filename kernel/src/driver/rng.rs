use super::{Driver, DriverErr, DriverResult, GenericDeviceConfig};

pub trait RngDevice: Driver {
    fn fill_bytes(&self, buf: &mut [u8]) -> DriverResult<usize>;

    fn read_u32(&self) -> DriverResult<u32> {
        let mut buf = [0u8; 4];
        let n = self.fill_bytes(&mut buf)?;
        if n == buf.len() {
            Ok(u32::from_le_bytes(buf))
        } else {
            Err(DriverErr::HardwareFault)
        }
    }
}

pub type DynRngDevice = dyn RngDevice<Config = GenericDeviceConfig, Error = DriverErr>;
