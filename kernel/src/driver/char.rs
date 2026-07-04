use super::{Driver, DriverErr, DriverResult, GenericDeviceConfig};
use core::cell::UnsafeCell;
#[cfg(feature = "async_rt")]
use core::future::Future;
#[cfg(feature = "async_rt")]
use core::pin::Pin;
use core::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "async_rt")]
use core::task::{Context, Poll};
use spin::Mutex;

/// Byte-oriented character device, such as UART, USB CDC, or a virtual console.
pub trait CharDevice: Driver {
    /// Read one byte without blocking.
    fn read_byte(&self) -> DriverResult<u8>;

    /// Write one byte without sleeping.
    fn write_byte(&self, byte: u8) -> DriverResult<()>;

    /// Read as many bytes as are immediately available.
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

    /// Write as many bytes as the device accepts immediately.
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

pub type DynCharDevice = dyn CharDevice<Config = GenericDeviceConfig, Error = DriverErr>;

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
static RX_POLL_FN: Mutex<Option<fn() -> Option<u8>>> = Mutex::new(None);
#[cfg(feature = "async_rt")]
static RX_EVENT: crate::sched::Event<1> = crate::sched::Event::new();

pub fn set_rx_poll_fn(poll: fn() -> Option<u8>) {
    *RX_POLL_FN.lock() = Some(poll);
}

pub fn has_rx_poll_fn() -> bool {
    RX_POLL_FN.lock().is_some()
}

pub fn push_to_rx_buf(byte: u8) {
    if UART_RX_BUF.push(byte) {
        #[cfg(feature = "async_rt")]
        {
            crate::sched::async_rt::note_rx_byte(byte);
            RX_EVENT.signal();
        }
    }
}

pub fn pop_from_rx_buf() -> Option<u8> {
    if let Some(byte) = UART_RX_BUF.pop() {
        return Some(byte);
    }

    let poll = (*RX_POLL_FN.lock())?;
    crate::arch::disable_irq();
    let byte = poll();
    crate::arch::enable_irq();
    byte.or_else(|| UART_RX_BUF.pop())
}

#[cfg(feature = "async_rt")]
pub fn read_byte_async() -> ReadByte {
    ReadByte {
        wait: RX_EVENT.wait_async(),
    }
}

#[cfg(feature = "async_rt")]
pub fn wait_rx_async() -> crate::sched::wait::EventWait<'static, 1> {
    RX_EVENT.wait_async()
}

#[cfg(feature = "async_rt")]
pub struct ReadByte {
    wait: crate::sched::wait::EventWait<'static, 1>,
}

#[cfg(feature = "async_rt")]
impl Future for ReadByte {
    type Output = u8;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(byte) = pop_from_rx_buf() {
            return Poll::Ready(byte);
        }

        match Pin::new(&mut self.wait).poll(cx) {
            Poll::Ready(()) => {
                if let Some(byte) = pop_from_rx_buf() {
                    Poll::Ready(byte)
                } else {
                    self.wait = RX_EVENT.wait_async();
                    Poll::Pending
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
