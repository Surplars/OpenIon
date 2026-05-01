use spin::Mutex;
use core::mem::MaybeUninit;
use core::ptr::NonNull;

/// Static Object Pool
pub struct Slab<T, const N: usize> {
    inner: Mutex<SlabInner<T, N>>,
}

struct SlabInner<T, const N: usize> {
    data: [MaybeUninit<T>; N],
    next_free: [usize; N],
    free_head: Option<usize>,
    free_count: usize,
}

impl<T, const N: usize> Slab<T, N> {
    pub const fn new() -> Self {
        let mut next_free = [0; N];
        let mut i = 0;
        while i < N {
            next_free[i] = i + 1;
            i += 1;
        }

        Self {
            inner: Mutex::new(SlabInner {
                data: [const { MaybeUninit::uninit() }; N],
                next_free,
                free_head: if N > 0 { Some(0) } else { None },
                free_count: N,
            }),
        }
    }

    pub fn alloc(&self, val: T) -> Option<NonNull<T>> {
        let mut inner = self.inner.lock();
        if let Some(idx) = inner.free_head {
            let next = inner.next_free[idx];
            inner.free_head = if next < N { Some(next) } else { None };
            inner.free_count -= 1;

            let ptr = inner.data[idx].as_mut_ptr();
            unsafe { core::ptr::write(ptr, val); }
            Some(unsafe { NonNull::new_unchecked(ptr) })
        } else {
            None
        }
    }

    pub unsafe fn free(&self, ptr: NonNull<T>) {
        let p = ptr.as_ptr();
        let mut inner = self.inner.lock();
        
        let base = inner.data.as_ptr() as *const _ as usize;
        let p_usize = p as usize;
        let step = core::mem::size_of::<MaybeUninit<T>>();
        
        let idx = (p_usize - base) / step;
        if idx >= N {
            return;
        }

        unsafe { core::ptr::drop_in_place(p); }

        if let Some(head) = inner.free_head {
            inner.next_free[idx] = head;
        } else {
            inner.next_free[idx] = N;
        }
        
        inner.free_head = Some(idx);
        inner.free_count += 1;
    }

    pub fn free_count(&self) -> usize {
        self.inner.lock().free_count
    }
}

