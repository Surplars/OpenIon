use super::addr::{PAGE_SIZE, PhysAddr};
use core::sync::atomic::{AtomicU32, Ordering};

const MAX_PAGES: usize = 32768; // Support up to 128MB of page table pages
const WORD_BITS: usize = 32;
const BITMAP_WORDS: usize = MAX_PAGES / WORD_BITS;

/// Bitmap-based physical page frame allocator.
/// Tracks up to 32768 pages (128MB with 4K pages).
/// Each bit = one page: 1 = free, 0 = allocated.
pub struct FrameAllocator {
    bitmap: [AtomicU32; BITMAP_WORDS],
    base_addr: PhysAddr,
    total_pages: usize,
    free_pages: usize,
    next_word: usize,
    next_bit: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct FrameStats {
    pub initialized: bool,
    pub base: PhysAddr,
    pub total_pages: usize,
    pub free_pages: usize,
}

impl FrameAllocator {
    pub const fn new() -> Self {
        const ZERO: AtomicU32 = AtomicU32::new(0);
        Self {
            bitmap: [ZERO; BITMAP_WORDS],
            base_addr: PhysAddr::ZERO,
            total_pages: 0,
            free_pages: 0,
            next_word: 0,
            next_bit: 0,
        }
    }

    /// Initialize with a physical memory region [base, base + size).
    pub unsafe fn init(&mut self, base: PhysAddr, size: usize) {
        self.base_addr = base.page_align();
        let adjust = self.base_addr.raw().saturating_sub(base.raw());
        let usable = size.saturating_sub(adjust);
        self.total_pages = (usable / PAGE_SIZE).min(MAX_PAGES);
        self.free_pages = self.total_pages;
        self.next_word = 0;
        self.next_bit = 0;

        for word in self.bitmap.iter() {
            word.store(0, Ordering::Relaxed);
        }

        for i in 0..self.total_pages {
            let word = i / WORD_BITS;
            let bit = i % WORD_BITS;
            if word < BITMAP_WORDS {
                self.bitmap[word].fetch_or(1 << bit, Ordering::Relaxed);
            }
        }
    }

    /// Allocate a single physical page frame.
    pub fn alloc(&mut self) -> Option<PhysAddr> {
        if self.total_pages == 0 {
            return None;
        }

        let start_word = self.next_word.min(BITMAP_WORDS - 1);
        let start_bit = self.next_bit.min(WORD_BITS - 1);

        for scan in 0..BITMAP_WORDS {
            let word_idx = (start_word + scan) % BITMAP_WORDS;
            let mut word = self.bitmap[word_idx].load(Ordering::Relaxed);
            if word == 0 {
                continue;
            }
            if scan == 0 && start_bit > 0 {
                word &= !((1u32 << start_bit) - 1);
            }
            if word == 0 {
                continue;
            }

            let bit = word.trailing_zeros() as usize;
            if bit >= WORD_BITS {
                continue;
            }
            let page_idx = word_idx * WORD_BITS + bit;
            if page_idx >= self.total_pages {
                continue;
            }

            let mask = 1u32 << bit;
            loop {
                let old = self.bitmap[word_idx].load(Ordering::Relaxed);
                if old & mask == 0 {
                    break;
                }
                match self.bitmap[word_idx].compare_exchange_weak(
                    old,
                    old & !mask,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        self.free_pages -= 1;
                        self.next_word = page_idx / WORD_BITS;
                        self.next_bit = bit + 1;
                        if self.next_bit >= WORD_BITS {
                            self.next_word = (self.next_word + 1) % BITMAP_WORDS;
                            self.next_bit = 0;
                        }
                        return Some(self.base_addr + page_idx * PAGE_SIZE);
                    }
                    Err(_) => continue,
                }
            }
        }
        None
    }

    /// Free a previously allocated physical page frame.
    pub fn free(&mut self, addr: PhysAddr) {
        let raw = addr.page_align().raw();
        let base = self.base_addr.raw();
        if raw < base {
            return;
        }
        let page_idx = (raw - base) / PAGE_SIZE;
        if page_idx >= self.total_pages {
            return;
        }
        let word = page_idx / WORD_BITS;
        let bit = page_idx % WORD_BITS;
        let mask = 1u32 << bit;

        let prev = self.bitmap[word].fetch_or(mask, Ordering::Release);
        if prev & mask == 0 {
            self.free_pages += 1;
            let cur_word = self.next_word.min(BITMAP_WORDS - 1);
            let cur_bit = self.next_bit.min(WORD_BITS - 1);
            if word < cur_word || (word == cur_word && bit < cur_bit) {
                self.next_word = word;
                self.next_bit = bit;
            }
        }
    }

    pub fn total_pages(&self) -> usize {
        self.total_pages
    }

    pub fn free_pages(&self) -> usize {
        self.free_pages
    }

    pub fn stats(&self) -> FrameStats {
        FrameStats {
            initialized: self.total_pages != 0,
            base: self.base_addr,
            total_pages: self.total_pages,
            free_pages: self.free_pages,
        }
    }
}
