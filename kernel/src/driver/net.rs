use super::{Driver, DriverResult};

/// MAC address, represented as a 6-byte array.
pub type MacAddress = [u8; 6];

pub trait NetDevice: Driver {
    /// Get the MAC address of the network interface.
    fn mac_address(&self) -> MacAddress;

    /// Checks if there is any received packet waiting.
    fn has_rx_data(&self) -> bool;

    /// Transmit a network packet.
    ///
    /// # Arguments
    /// * `buf` - The Ethernet frame / packet data to be transmitted.
    fn transmit(&self, buf: &[u8]) -> DriverResult<()>;

    /// Receive a network packet.
    ///
    /// # Arguments
    /// * `buf` - A buffer to store the received Ethernet frame.
    ///
    /// # Returns
    /// The number of bytes received.
    fn receive(&self, buf: &mut [u8]) -> DriverResult<usize>;
}
pub type DynNetDevice = dyn NetDevice<Config = crate::driver::GenericDeviceConfig, Error = crate::driver::DriverErr>;
