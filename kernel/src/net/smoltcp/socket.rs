use crate::net::NETWORK_SOCKETS;
use crate::net::posix::{
    AddressFamily, Ipv4Addr, PosixError, Protocol, SocketAddrV4, SocketProvider, SocketType,
};
use core::sync::atomic::{AtomicBool, Ordering};
use smoltcp::iface::SocketHandle;
use smoltcp::socket::udp::{
    PacketBuffer as UdpPacketBuffer, PacketMetadata as UdpPacketMetadata, Socket as UdpSocket,
};
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address as SmolIpv4Address};

static mut UDP_RX_META: [UdpPacketMetadata; 4] = [UdpPacketMetadata::EMPTY; 4];
static mut UDP_RX_DATA: [u8; 1024] = [0; 1024];
static mut UDP_TX_META: [UdpPacketMetadata; 4] = [UdpPacketMetadata::EMPTY; 4];
static mut UDP_TX_DATA: [u8; 1024] = [0; 1024];

static UDP_IN_USE: AtomicBool = AtomicBool::new(false);

use crate::sync::Mutex;

static HANDLE_POOL: Mutex<[Option<SocketHandle>; 4]> = Mutex::new([None; 4]);

fn allocate_fd(handle: SocketHandle) -> Result<usize, PosixError> {
    let mut pool = HANDLE_POOL.lock();
    for (i, slot) in pool.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(handle);
            return Ok(i);
        }
    }
    Err(PosixError::ENOMEM)
}

fn get_handle(fd: usize) -> Result<SocketHandle, PosixError> {
    let pool = HANDLE_POOL.lock();
    if fd < pool.len() {
        if let Some(h) = pool[fd] {
            return Ok(h);
        }
    }
    Err(PosixError::EBADF)
}

fn free_fd(fd: usize) {
    let mut pool = HANDLE_POOL.lock();
    if fd < pool.len() {
        pool[fd] = None;
    }
}

pub struct SmoltcpProvider;

impl SocketProvider for SmoltcpProvider {
    fn socket(
        &self,
        domain: AddressFamily,
        ty: SocketType,
        proto: Protocol,
    ) -> Result<usize, PosixError> {
        if domain != AddressFamily::Inet {
            return Err(PosixError::EAFNOSUPPORT);
        }

        match (ty, proto) {
            (SocketType::Dgram, Protocol::Udp) | (SocketType::Dgram, Protocol::Any) => {
                if UDP_IN_USE
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    return Err(PosixError::ENOBUFS);
                }

                let rx_buffer = UdpPacketBuffer::new(unsafe { &mut UDP_RX_META[..] }, unsafe {
                    &mut UDP_RX_DATA[..]
                });
                let tx_buffer = UdpPacketBuffer::new(unsafe { &mut UDP_TX_META[..] }, unsafe {
                    &mut UDP_TX_DATA[..]
                });
                let socket = UdpSocket::new(rx_buffer, tx_buffer);

                let mut sockets_lock = NETWORK_SOCKETS.lock();
                if let Some(sockets) = sockets_lock.as_mut() {
                    let handle = sockets.add(socket);
                    match allocate_fd(handle) {
                        Ok(fd) => Ok(fd),
                        Err(e) => {
                            sockets.remove(handle);
                            UDP_IN_USE.store(false, Ordering::SeqCst);
                            Err(e)
                        }
                    }
                } else {
                    Err(PosixError::ENOMEM)
                }
            }
            _ => Err(PosixError::EAFNOSUPPORT),
        }
    }

    fn bind(&self, fd: usize, addr: SocketAddrV4) -> Result<(), PosixError> {
        let handle = get_handle(fd)?;
        let mut sockets_lock = NETWORK_SOCKETS.lock();
        if let Some(sockets) = sockets_lock.as_mut() {
            let socket = sockets.get_mut::<UdpSocket>(handle);
            socket.bind(addr.port).map_err(|_| PosixError::EINVAL)
        } else {
            Err(PosixError::ENOTCONN)
        }
    }

    fn connect(&self, _fd: usize, _addr: SocketAddrV4) -> Result<(), PosixError> {
        Ok(())
    }

    fn listen(&self, _fd: usize, _backlog: usize) -> Result<(), PosixError> {
        Err(PosixError::EINVAL)
    }

    fn accept(&self, _fd: usize) -> Result<(usize, SocketAddrV4), PosixError> {
        Err(PosixError::EINVAL)
    }

    fn send(&self, _fd: usize, _buf: &[u8]) -> Result<usize, PosixError> {
        Err(PosixError::ENOTCONN)
    }

    fn recv(&self, fd: usize, buf: &mut [u8]) -> Result<usize, PosixError> {
        let handle = get_handle(fd)?;
        let mut sockets_lock = NETWORK_SOCKETS.lock();
        if let Some(sockets) = sockets_lock.as_mut() {
            let socket = sockets.get_mut::<UdpSocket>(handle);
            match socket.recv_slice(buf) {
                Ok((len, _meta)) => Ok(len),
                Err(_) => Err(PosixError::EAGAIN),
            }
        } else {
            Err(PosixError::ENOTCONN)
        }
    }

    fn send_to(&self, fd: usize, buf: &[u8], addr: SocketAddrV4) -> Result<usize, PosixError> {
        let handle = get_handle(fd)?;
        let mut sockets_lock = NETWORK_SOCKETS.lock();
        if let Some(sockets) = sockets_lock.as_mut() {
            let socket = sockets.get_mut::<UdpSocket>(handle);
            let endpoint = IpEndpoint::new(
                IpAddress::Ipv4(SmolIpv4Address::new(
                    addr.ip.0[0],
                    addr.ip.0[1],
                    addr.ip.0[2],
                    addr.ip.0[3],
                )),
                addr.port,
            );
            match socket.send_slice(buf, endpoint) {
                Ok(()) => Ok(buf.len()),
                Err(_) => Err(PosixError::ENOBUFS),
            }
        } else {
            Err(PosixError::ENOTCONN)
        }
    }

    fn recv_from(&self, fd: usize, buf: &mut [u8]) -> Result<(usize, SocketAddrV4), PosixError> {
        let handle = get_handle(fd)?;
        let mut sockets_lock = NETWORK_SOCKETS.lock();
        if let Some(sockets) = sockets_lock.as_mut() {
            let socket = sockets.get_mut::<UdpSocket>(handle);
            match socket.recv_slice(buf) {
                Ok((len, meta)) => {
                    let ip = match meta.endpoint.addr {
                        #[allow(unreachable_patterns)]
                        IpAddress::Ipv4(v4) => {
                            let b = v4.octets();
                            Ipv4Addr(b)
                        }
                        #[allow(unreachable_patterns)]
                        _ => Ipv4Addr([0, 0, 0, 0]),
                    };
                    Ok((
                        len,
                        SocketAddrV4 {
                            ip,
                            port: meta.endpoint.port,
                        },
                    ))
                }
                Err(_) => Err(PosixError::EAGAIN),
            }
        } else {
            Err(PosixError::ENOTCONN)
        }
    }

    fn close(&self, fd: usize) -> Result<(), PosixError> {
        let handle = get_handle(fd)?;
        let mut sockets_lock = NETWORK_SOCKETS.lock();
        if let Some(sockets) = sockets_lock.as_mut() {
            sockets.remove(handle);
            free_fd(fd);
            UDP_IN_USE.store(false, Ordering::SeqCst);
            Ok(())
        } else {
            Err(PosixError::ENOTCONN)
        }
    }
}
