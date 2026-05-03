use crate::arch::{disable_irq, enable_irq};
use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};
use spin::Mutex as SpinMutex;
use spin::MutexGuard as SpinMutexGuard;

/// RTOS 中断安全互斥锁 (IRQ-safe Mutex)
///
/// 在获取锁之前会关闭全局中断以防止在中断处理程序中发生死锁，
/// 在释放锁时重新计算嵌套并启用中断。
pub struct Mutex<T: ?Sized> {
    inner: SpinMutex<T>,
}

unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            inner: SpinMutex::new(value),
        }
    }

    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }
}

impl<T: ?Sized> Mutex<T> {
    /// 锁定互斥量。此操作将禁用中断，然后尝试获取自旋锁。
    pub fn lock(&self) -> MutexGuard<'_, T> {
        disable_irq(); // 先关闭中断，防止中断上下文中重入导致死锁
        MutexGuard {
            inner_guard: ManuallyDrop::new(self.inner.lock()),
        }
    }

    /// 尝试锁定，如果被占用则立刻返回 None
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        disable_irq();
        match self.inner.try_lock() {
            Some(guard) => Some(MutexGuard {
                inner_guard: ManuallyDrop::new(guard),
            }),
            None => {
                enable_irq(); // 没获取到锁，立刻恢复中断状态
                None
            }
        }
    }
}

/// 互斥锁守卫，在离开作用域时自动释放锁并恢复中断状态
pub struct MutexGuard<'a, T: ?Sized + 'a> {
    // 使用 ManuallyDrop 以便能够在 Drop 中显式控制解锁顺序
    inner_guard: ManuallyDrop<SpinMutexGuard<'a, T>>,
}

impl<'a, T: ?Sized> Deref for MutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.inner_guard.deref()
    }
}

impl<'a, T: ?Sized> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner_guard.deref_mut()
    }
}

impl<'a, T: ?Sized> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        unsafe {
            // 先释放自旋锁，再恢复中断。
            // 顺序至关重要：如果先开中断，在这瞬间如果有中断来拿同一个锁，就会死锁。
            ManuallyDrop::drop(&mut self.inner_guard);
            enable_irq();
        }
    }
}
