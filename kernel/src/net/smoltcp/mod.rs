use crate::platform::Platform;
use smoltcp::iface::{Config, Interface, SocketSet, SocketStorage};
use smoltcp::socket::dhcpv4::{Socket as DhcpSocket, Event as DhcpEvent};
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpCidr};
// use smoltcp::time::Instant;
use spin::Mutex;

pub mod device;
pub mod socket;

pub use socket::SmoltcpProvider;

use device::SmoltcpDevice;
use crate::driver::net::DynNetDevice;

pub static GLOBAL_NET_PROVIDER: SmoltcpProvider = SmoltcpProvider;

pub static NETWORK_IFACE: Mutex<Option<Interface>> = Mutex::new(None);
static RX_TX_DEVICE: Mutex<Option<&'static DynNetDevice>> = Mutex::new(None);

pub static NETWORK_SOCKETS: Mutex<Option<SocketSet<'static>>> = Mutex::new(None);
static mut DHCP_HANDLE: Option<smoltcp::iface::SocketHandle> = None;
static mut SOCKETS_STORAGE: [SocketStorage<'static>; 8] = [SocketStorage::EMPTY; 8];

pub fn init<P: Platform>() {
    if let Some(dev) = P::net_device() {
        crate::kinfo!("Initializing smoltcp network stack...");

        let mac = dev.mac_address();
        let hw_addr = HardwareAddress::Ethernet(EthernetAddress(mac));

        let mut config = Config::new(hw_addr);
        config.random_seed = 0x12345678;

        let iface = Interface::new(config, &mut SmoltcpDevice::new(dev), smoltcp::time::Instant::from_micros(crate::timer::time_us() as i64));

        // Initialize sockets
        unsafe {
            let mut socket_set = SocketSet::new(&mut (&mut *core::ptr::addr_of_mut!(SOCKETS_STORAGE))[..]);
            let dhcp_socket = DhcpSocket::new();
            DHCP_HANDLE = Some(socket_set.add(dhcp_socket));
            *NETWORK_SOCKETS.lock() = Some(socket_set);
        }

        *NETWORK_IFACE.lock() = Some(iface);
        *RX_TX_DEVICE.lock() = Some(dev);

        crate::kinfo!("Network stack initialized (DHCP pending)");
    }
}

pub fn poll() {
    let mut iface_lock = NETWORK_IFACE.lock();
    let mut dev_lock = RX_TX_DEVICE.lock();
    let mut sockets_lock = NETWORK_SOCKETS.lock();

    if let (Some(iface), Some(dev), Some(sockets)) = (iface_lock.as_mut(), dev_lock.as_mut(), sockets_lock.as_mut()) {
        let timestamp = smoltcp::time::Instant::from_micros(crate::timer::time_us() as i64);
        let mut sdev = SmoltcpDevice::new(*dev);

        let _ = iface.poll(timestamp, &mut sdev, sockets);

        // Handle DHCP
        unsafe {
            if let Some(handle) = DHCP_HANDLE {
                let dhcp_socket = sockets.get_mut::<DhcpSocket>(handle);
                if let Some(event) = dhcp_socket.poll() {
                    match event {
                        DhcpEvent::Configured(config) => {
                            crate::kinfo!("DHCP Configured! IP: {}", config.address);
                            if let Some(router) = config.router {
                                crate::kinfo!("Router IP: {}", router);
                                iface.routes_mut().add_default_ipv4_route(router).unwrap();
                            }
                            iface.update_ip_addrs(|ip_addrs| {
                                ip_addrs.clear();
                                let _ = ip_addrs.push(IpCidr::Ipv4(config.address));
                            });
                        }
                        DhcpEvent::Deconfigured => {
                            crate::kinfo!("DHCP Deconfigured!");
                            iface.update_ip_addrs(|ip_addrs| ip_addrs.clear());
                            iface.routes_mut().remove_default_ipv4_route();
                        }
                    }
                }
            }
        }
    }
}
