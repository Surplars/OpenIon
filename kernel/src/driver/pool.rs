use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

struct DriverSlot<T> {
    value: UnsafeCell<MaybeUninit<T>>,
}

impl<T> DriverSlot<T> {
    const fn new() -> Self {
        Self {
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

unsafe impl<T: Send> Sync for DriverSlot<T> {}

pub struct StaticDriverPool<T, const N: usize> {
    slots: [DriverSlot<T>; N],
    next: AtomicUsize,
}

impl<T, const N: usize> StaticDriverPool<T, N> {
    pub const fn new() -> Self {
        Self {
            slots: [const { DriverSlot::new() }; N],
            next: AtomicUsize::new(0),
        }
    }

    pub fn alloc(&'static self, value: T) -> Option<&'static mut T> {
        let idx = self.next.fetch_add(1, Ordering::AcqRel);
        if idx >= N {
            return None;
        }

        let slot = &self.slots[idx];
        unsafe {
            (*slot.value.get()).write(value);
            Some(&mut *(*slot.value.get()).as_mut_ptr())
        }
    }
}

unsafe impl<T: Send, const N: usize> Sync for StaticDriverPool<T, N> {}
