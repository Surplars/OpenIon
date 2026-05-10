//! Architecture-neutral syscall dispatch.
//!
//! OpenIon keeps the syscall ABI stable and small. Higher-level Rust or C
//! libraries should wrap this layer instead of linking user programs against
//! kernel internals.

use crate::driver::{DriverErr, char::pop_from_rx_buf, manager::DriverManager};

pub type SysResult = isize;

pub mod nr {
    pub const WRITE: usize = 1;
    pub const READ: usize = 2;
    pub const EXIT: usize = 3;
    pub const YIELD: usize = 4;
}

pub mod errno {
    pub const EPERM: isize = 1;
    pub const ENOENT: isize = 2;
    pub const EIO: isize = 5;
    pub const EBADF: isize = 9;
    pub const EAGAIN: isize = 11;
    pub const ENOMEM: isize = 12;
    pub const EFAULT: isize = 14;
    pub const EINVAL: isize = 22;
    pub const ENOSYS: isize = 38;
    pub const ENOTSUP: isize = 95;
}

const STDIN_FILENO: usize = 0;
const STDOUT_FILENO: usize = 1;
const STDERR_FILENO: usize = 2;

#[derive(Clone, Copy)]
pub struct SyscallArgs {
    pub nr: usize,
    pub args: [usize; 6],
}

impl SyscallArgs {
    pub const fn new(nr: usize, args: [usize; 6]) -> Self {
        Self { nr, args }
    }
}

#[derive(Clone, Copy)]
pub struct SyscallReturn {
    pub value: SysResult,
    pub schedule: bool,
}

impl SyscallReturn {
    pub const fn new(value: SysResult) -> Self {
        Self {
            value,
            schedule: false,
        }
    }

    pub const fn schedule(value: SysResult) -> Self {
        Self {
            value,
            schedule: true,
        }
    }
}

pub fn dispatch(call: SyscallArgs) -> SyscallReturn {
    let value = match call.nr {
        nr::WRITE => sys_write(call.args[0], call.args[1], call.args[2]),
        nr::READ => sys_read(call.args[0], call.args[1], call.args[2]),
        nr::EXIT => return sys_exit(call.args[0] as i32),
        nr::YIELD => return sys_yield(),
        _ => err(errno::ENOSYS),
    };
    SyscallReturn::new(value)
}

#[inline]
pub const fn err(errno: isize) -> SysResult {
    -errno
}

fn sys_write(fd: usize, ptr: usize, len: usize) -> SysResult {
    let Some(buf) = user_slice(ptr, len) else {
        return err(errno::EFAULT);
    };

    match fd {
        STDOUT_FILENO | STDERR_FILENO => write_console(buf),
        _ => err(errno::EBADF),
    }
}

fn sys_read(fd: usize, ptr: usize, len: usize) -> SysResult {
    let Some(buf) = user_slice_mut(ptr, len) else {
        return err(errno::EFAULT);
    };

    match fd {
        STDIN_FILENO => read_console(buf),
        _ => err(errno::EBADF),
    }
}

fn sys_exit(_code: i32) -> SyscallReturn {
    crate::sched::Scheduler::terminate_current();
    SyscallReturn::schedule(0)
}

fn sys_yield() -> SyscallReturn {
    crate::sched::Scheduler::schedule();
    SyscallReturn::schedule(0)
}

fn write_console(buf: &[u8]) -> SysResult {
    let mut written = 0usize;
    let mut saw_terminal = false;
    let mut first_error: Option<DriverErr> = None;

    DriverManager::for_each_driver(|drv| {
        if saw_terminal {
            return;
        }
        if let Some(term) = drv.as_terminal_device() {
            saw_terminal = true;
            match term.write_buffer(buf) {
                Ok(n) => written = n,
                Err(e) => first_error = Some(e),
            }
        }
    });

    if saw_terminal {
        return first_error.map_or(written as SysResult, driver_err);
    }

    if let Some(console) = crate::log::console() {
        for &byte in buf {
            if byte == b'\n' {
                console.putc(b'\r');
            }
            console.putc(byte);
        }
        return buf.len() as SysResult;
    }

    err(errno::EIO)
}

fn read_console(buf: &mut [u8]) -> SysResult {
    if buf.is_empty() {
        return 0;
    }

    let mut count = 0usize;
    for slot in buf {
        let Some(byte) = pop_from_rx_buf() else {
            break;
        };
        *slot = byte;
        count += 1;
    }

    if count == 0 {
        err(errno::EAGAIN)
    } else {
        count as SysResult
    }
}

fn driver_err(err: DriverErr) -> SysResult {
    match err {
        DriverErr::Timeout | DriverErr::Busy => self::err(errno::EAGAIN),
        DriverErr::InvalidConfig => self::err(errno::EINVAL),
        DriverErr::HardwareFault | DriverErr::InitFailed | DriverErr::Custom => {
            self::err(errno::EIO)
        }
        DriverErr::NotSupported => self::err(errno::ENOTSUP),
        DriverErr::RegistryFull => self::err(errno::ENOMEM),
        DriverErr::NotFound => self::err(errno::ENOENT),
    }
}

fn user_slice<'a>(ptr: usize, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if ptr == 0 || ptr.checked_add(len).is_none() {
        return None;
    }

    Some(unsafe { core::slice::from_raw_parts(ptr as *const u8, len) })
}

fn user_slice_mut<'a>(ptr: usize, len: usize) -> Option<&'a mut [u8]> {
    if len == 0 {
        return Some(&mut []);
    }
    if ptr == 0 || ptr.checked_add(len).is_none() {
        return None;
    }

    Some(unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len) })
}
