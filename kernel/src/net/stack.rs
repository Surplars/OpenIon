use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr};
use smoltcp::socket::tcp::{Socket as TcpSocket, SocketBuffer as TcpSocketBuffer};
use spin::Mutex;

static ETHERNET_IFACE: Mutex<Option<Interface>> = Mutex::new(None);
static SOCKETS: Mutex<Option<SocketSet<'static>>> = Mutex::new(None);

pub fn init_stack() {
    // We would need the NetDevice here. Let's find it.
    // For now, we will poll the net abstraction.
}
