use super::{Driver, DriverResult};

/// 字符设备 Trait
/// 用于所有按字节读写的设备流（如 UART, 键盘, 鼠标, 虚拟控制台等）
pub trait CharDevice: Driver {
    /// 从设备读取一个字节 (非阻塞)
    ///
    /// # 返回
    /// * `Ok(u8)` - 成功读取到数据
    /// * `Err(DriverErr::Busy)` - 当前没有数据可读
    /// * `Err(...)` - 其他错误
    fn read_byte(&self) -> DriverResult<u8>;

    /// 向设备写入一个字节 (非阻塞)
    ///
    /// # 返回
    /// * `Ok(())` - 写入成功或已缓存
    /// * `Err(DriverErr::Busy)` - 缓存已满或设备忙
    fn write_byte(&self, byte: u8) -> DriverResult<()>;

    /// 持续读取数据到缓冲区（默认提供实现）
    /// 尽量多读，遇到没数据或错误就返回当前已读长度
    fn read_buffer(&self, buf: &mut [u8]) -> DriverResult<usize> {
        let mut count = 0;
        for b in buf.iter_mut() {
            if let Ok(byte) = self.read_byte() {
                *b = byte;
                count += 1;
            } else {
                break;
            }
        }
        Ok(count)
    }

    /// 将缓冲区数据持续写入设备（默认提供实现）
    /// 尽量多写，如果设备满则返回已写长度
    fn write_buffer(&self, buf: &[u8]) -> DriverResult<usize> {
        let mut count = 0;
        for &b in buf.iter() {
            if self.write_byte(b).is_ok() {
                count += 1;
            } else {
                break;
            }
        }
        Ok(count)
    }
}

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Once;

const BUF_SIZE: usize = 128;
pub struct RxBuffer {
    data: UnsafeCell<[u8; BUF_SIZE]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

unsafe impl Sync for RxBuffer {}

impl RxBuffer {
    pub const fn new() -> Self {
        Self {
            data: UnsafeCell::new([0; BUF_SIZE]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Single-producer push. Intended producer is the UART IRQ handler.
    pub fn push(&self, val: u8) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = (head + 1) % BUF_SIZE;
        if next_head == self.tail.load(Ordering::Acquire) {
            return false;
        }
        unsafe {
            (*self.data.get())[head] = val;
        }
        self.head.store(next_head, Ordering::Release);
        true
    }

    /// Single-consumer pop. Intended consumer is the shell task.
    pub fn pop(&self) -> Option<u8> {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail == self.head.load(Ordering::Acquire) {
            return None;
        }
        let val = unsafe { (*self.data.get())[tail] };
        self.tail.store((tail + 1) % BUF_SIZE, Ordering::Release);
        Some(val)
    }
}

static UART_RX_BUF: RxBuffer = RxBuffer::new();
static RX_POLL_FN: Once<fn() -> Option<u8>> = Once::new();

pub fn set_rx_poll_fn(poll: fn() -> Option<u8>) {
    RX_POLL_FN.call_once(|| poll);
}

pub fn push_to_rx_buf(byte: u8) {
    let _ = UART_RX_BUF.push(byte);
}

pub fn pop_from_rx_buf() -> Option<u8> {
    if let Some(byte) = UART_RX_BUF.pop() {
        return Some(byte);
    }

    let poll = *RX_POLL_FN.get()?;
    crate::arch::disable_irq();
    let byte = poll();
    crate::arch::enable_irq();
    byte.or_else(|| UART_RX_BUF.pop())
}
