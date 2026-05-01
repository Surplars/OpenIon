use rlsf::Tlsf;
use spin::Mutex;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;

pub struct GlobalTlsfAlloc {
    inner: Mutex<Option<Tlsf<'static, usize, usize, 8, 8>>>,
}

impl GlobalTlsfAlloc {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    pub unsafe fn init(&self, memory: &'static mut [u8]) {
        let mut tlsf: Tlsf<'static, usize, usize, 8, 8> = Tlsf::new();
        let ptr = unsafe { NonNull::slice_from_raw_parts(NonNull::new_unchecked(memory.as_mut_ptr()), memory.len()) };
        unsafe { tlsf.insert_free_block_ptr(ptr); }
        *self.inner.lock() = Some(tlsf);
    }
}

unsafe impl GlobalAlloc for GlobalTlsfAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut lock = self.inner.lock();
        if let Some(ref mut tlsf) = *lock {
            if let Some(ptr) = tlsf.allocate(layout) {
                return ptr.as_ptr();
            }
        }
        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut lock = self.inner.lock();
        if let Some(ref mut tlsf) = *lock {
            if let Some(non_null) = NonNull::new(ptr) {
                unsafe { tlsf.deallocate(non_null, layout.align()); }
            }
        }
    }
}

