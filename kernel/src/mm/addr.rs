use core::fmt;
use core::ops::{Add, AddAssign, Sub};

pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: usize = 12;

/// Physical address — always valid, no virtual translation.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct PhysAddr(usize);

/// Virtual address — may require page table translation to become physical.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct VirtAddr(usize);

// ---------- PhysAddr ----------

impl PhysAddr {
    pub const ZERO: Self = Self(0);

    pub const fn new(addr: usize) -> Self {
        Self(addr)
    }

    pub const fn raw(self) -> usize {
        self.0
    }

    pub const fn is_aligned(self, align: usize) -> bool {
        self.0 % align == 0
    }

    pub const fn page_align(self) -> Self {
        Self(self.0 & !(PAGE_SIZE - 1))
    }

    pub const fn page_offset(self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }

    pub const fn page_number(self) -> usize {
        self.0 >> PAGE_SHIFT
    }

    pub const fn is_null(self) -> bool {
        self.0 == 0
    }

    pub fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }

    pub fn as_mut_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }
}

impl Add<usize> for PhysAddr {
    type Output = Self;
    fn add(self, rhs: usize) -> Self { Self(self.0 + rhs) }
}

impl AddAssign<usize> for PhysAddr {
    fn add_assign(&mut self, rhs: usize) { self.0 += rhs; }
}

impl Sub<usize> for PhysAddr {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self { Self(self.0 - rhs) }
}

impl Sub<Self> for PhysAddr {
    type Output = usize;
    fn sub(self, rhs: Self) -> usize { self.0 - rhs.0 }
}

impl fmt::Debug for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PA:{:#x}", self.0)
    }
}

impl fmt::Display for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

// ---------- VirtAddr ----------

impl VirtAddr {
    pub const ZERO: Self = Self(0);

    pub const fn new(addr: usize) -> Self {
        Self(addr)
    }

    pub const fn raw(self) -> usize {
        self.0
    }

    pub const fn is_aligned(self, align: usize) -> bool {
        self.0 % align == 0
    }

    pub const fn page_align(self) -> Self {
        Self(self.0 & !(PAGE_SIZE - 1))
    }

    pub const fn page_offset(self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }

    pub const fn is_null(self) -> bool {
        self.0 == 0
    }
}

impl Add<usize> for VirtAddr {
    type Output = Self;
    fn add(self, rhs: usize) -> Self { Self(self.0 + rhs) }
}

impl AddAssign<usize> for VirtAddr {
    fn add_assign(&mut self, rhs: usize) { self.0 += rhs; }
}

impl Sub<usize> for VirtAddr {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self { Self(self.0 - rhs) }
}

impl fmt::Debug for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VA:{:#x}", self.0)
    }
}

impl fmt::Display for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}
