use crate::driver::net::DynNetDevice;
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;

pub struct SmoltcpDevice<'a> {
    pub device: &'a DynNetDevice,
}

impl<'a> SmoltcpDevice<'a> {
    pub fn new(device: &'a DynNetDevice) -> Self {
        Self { device }
    }
}

pub struct SmoltcpRxToken<'a> {
    device: &'a DynNetDevice,
}

impl<'a> RxToken for SmoltcpRxToken<'a> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let mut buf = [0u8; 1514];
        let len = self.device.receive(&mut buf).unwrap_or(0);
        f(&buf[..len])
    }
}

pub struct SmoltcpTxToken<'a> {
    device: &'a DynNetDevice,
}

impl<'a> TxToken for SmoltcpTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = [0u8; 1514];
        let result = f(&mut buf[..len]);
        let _ = self.device.transmit(&buf[..len]);
        result
    }
}

impl<'a> Device for SmoltcpDevice<'a> {
    type RxToken<'b>
        = SmoltcpRxToken<'b>
    where
        Self: 'b;
    type TxToken<'b>
        = SmoltcpTxToken<'b>
    where
        Self: 'b;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if self.device.has_rx_data() {
            Some((
                SmoltcpRxToken {
                    device: self.device,
                },
                SmoltcpTxToken {
                    device: self.device,
                },
            ))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(SmoltcpTxToken {
            device: self.device,
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1514;
        caps.max_burst_size = Some(1);
        caps.medium = Medium::Ethernet;
        caps
    }
}
