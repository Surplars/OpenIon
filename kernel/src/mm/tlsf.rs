use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use rlsf::Tlsf;
use spin::Mutex;

#[derive(Clone, Copy, Debug)]
pub struct HeapStats {
    pub initialized: bool,
    pub start: usize,
    pub size: usize,
    pub used: usize,
    pub allocations: usize,
    pub failed_allocations: usize,
}

pub struct GlobalTlsfAlloc {
    inner: Mutex<Option<Tlsf<'static, usize, usize, 8, 8>>>,
    initialized: AtomicBool,
    start: AtomicUsize,
    size: AtomicUsize,
    used: AtomicUsize,
    allocations: AtomicUsize,
    failed_allocations: AtomicUsize,
}

impl GlobalTlsfAlloc {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            initialized: AtomicBool::new(false),
            start: AtomicUsize::new(0),
            size: AtomicUsize::new(0),
            used: AtomicUsize::new(0),
            allocations: AtomicUsize::new(0),
            failed_allocations: AtomicUsize::new(0),
        }
    }

    pub unsafe fn init(&self, memory: &'static mut [u8]) {
        let mut guard = self.inner.lock();
        if guard.is_some() {
            return;
        }

        let mut tlsf: Tlsf<'static, usize, usize, 8, 8> = Tlsf::new();
        let ptr = unsafe {
            NonNull::slice_from_raw_parts(NonNull::new_unchecked(memory.as_mut_ptr()), memory.len())
        };
        unsafe {
            tlsf.insert_free_block_ptr(ptr);
        }
        *guard = Some(tlsf);

        self.start
            .store(memory.as_ptr() as usize, Ordering::Release);
        self.size.store(memory.len(), Ordering::Release);
        self.used.store(0, Ordering::Release);
        self.allocations.store(0, Ordering::Release);
        self.failed_allocations.store(0, Ordering::Release);
        self.initialized.store(true, Ordering::Release);
    }

    pub fn stats(&self) -> HeapStats {
        HeapStats {
            initialized: self.initialized.load(Ordering::Acquire),
            start: self.start.load(Ordering::Acquire),
            size: self.size.load(Ordering::Acquire),
            used: self.used.load(Ordering::Acquire),
            allocations: self.allocations.load(Ordering::Acquire),
            failed_allocations: self.failed_allocations.load(Ordering::Acquire),
        }
    }
}

unsafe impl GlobalAlloc for GlobalTlsfAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut lock = self.inner.lock();
        if let Some(ref mut tlsf) = *lock {
            if let Some(ptr) = tlsf.allocate(layout) {
                self.used.fetch_add(layout.size(), Ordering::AcqRel);
                self.allocations.fetch_add(1, Ordering::AcqRel);
                return ptr.as_ptr();
            }
        }
        self.failed_allocations.fetch_add(1, Ordering::AcqRel);
        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut lock = self.inner.lock();
        if let Some(ref mut tlsf) = *lock {
            if let Some(non_null) = NonNull::new(ptr) {
                unsafe {
                    tlsf.deallocate(non_null, layout.align());
                }
                self.used.fetch_sub(layout.size(), Ordering::AcqRel);
                self.allocations.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }
}
