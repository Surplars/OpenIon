//! 内核 POSIX 网络接口抽象层
//! 
//! 这个模块定义了通用的 POSIX 风格的网络 API。无论是底层的 smoltcp 
//! 这样内核的 VFS 或 Syscall 层就可以通过多态/泛型与具体的协议栈解耦。

use core::result::Result;

/// POSIX 错误码的抽象
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosixError {
    EACCES,        // 权限不足
    EAFNOSUPPORT,  // 地址族不支持
    EINVAL,        // 无效的参数
    ENOMEM,        // 内存不足
    EAGAIN,        // 资源暂时不可用
    EWOULDBLOCK,   // 操作将阻塞
    ENOTCONN,      // Socket未连接
    ECONNREFUSED,  // 连接被拒绝
    EADDRINUSE,    // 地址已被使用
    ENOBUFS,       // 没有可用的Buffer
    EBADF,         // 无效的文件描述符
    // 可以继续根据需要扩充
}

/// Address Family (AF_INET, AF_INET6)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressFamily {
    Inet,
    Inet6,
}

/// Socket Type (SOCK_STREAM, SOCK_DGRAM等)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketType {
    Stream, // TCP
    Dgram,  // UDP
    Raw,    // 原始套接字
}

/// Protocol (IPPROTO_TCP, IPPROTO_UDP等)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
    Icmp,
    Any,
}

/// IPv4 地址结构
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Addr(pub [u8; 4]);

impl core::fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

/// Socket 地址结构 (对应 C 的 sockaddr_in)
#[derive(Debug, Clone, Copy)]
pub struct SocketAddrV4 {
    pub ip: Ipv4Addr,
    pub port: u16,
}

/// 全局 Socket 提供者接口。
/// 底层网络栈需要实现此 Trait，由内核持有其实例 (如 static).
pub trait SocketProvider {
    /// 创建一个 Socket，返回代表这个 socket 的具柄 ID (供 VFS 管理，类似于内核的 socket() 句柄)
    fn socket(&self, domain: AddressFamily, ty: SocketType, proto: Protocol) -> Result<usize, PosixError>;
    
    /// 绑定本地 IP 及端口
    fn bind(&self, fd: usize, addr: SocketAddrV4) -> Result<(), PosixError>;
    
    /// 连接到远端 (通常用于 TCP connect 或 UDP connect)
    fn connect(&self, fd: usize, addr: SocketAddrV4) -> Result<(), PosixError>;
    
    /// 开始监听请求 (面向 TCP 等面向连接栈)
    fn listen(&self, fd: usize, backlog: usize) -> Result<(), PosixError>;
    
    /// 接受连接，返回新的 (Socket 句柄, 远端地址)
    fn accept(&self, fd: usize) -> Result<(usize, SocketAddrV4), PosixError>;
    
    /// 发送数据
    fn send(&self, fd: usize, buf: &[u8]) -> Result<usize, PosixError>;
    
    /// 接收数据
    fn recv(&self, fd: usize, buf: &mut [u8]) -> Result<usize, PosixError>;
    
    /// 发送（带目标地址）
    fn send_to(&self, fd: usize, buf: &[u8], addr: SocketAddrV4) -> Result<usize, PosixError>;
    
    /// 接收（带来源地址）
    fn recv_from(&self, fd: usize, buf: &mut [u8]) -> Result<(usize, SocketAddrV4), PosixError>;
    
    /// 关闭并释放 Socket
    fn close(&self, fd: usize) -> Result<(), PosixError>;
}
