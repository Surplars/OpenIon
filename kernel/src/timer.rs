use core::sync::atomic::{AtomicU32, Ordering};

/// SysTick 每次增加的微秒数（由平台配置）
static TICK_US: AtomicU32 = AtomicU32::new(0);

/// 当前 tick 计数
static TICKS: AtomicU32 = AtomicU32::new(0);

/// 初始化（由 platform 提供 systick 频率）
pub fn init(systick_hz: u32) {
    let tick_us = 1_000_000 / systick_hz;
    TICK_US.store(tick_us, Ordering::Relaxed);
}

/// SysTick 中断调用
pub fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// 获取 tick 计数
pub fn ticks() -> u32 {
    TICKS.load(Ordering::Relaxed)
}

/// Get time in microseconds (safe against intermediate overflow)
pub fn time_us() -> u64 {
    let ticks = TICKS.load(Ordering::Relaxed) as u64;
    let tick_us = TICK_US.load(Ordering::Relaxed) as u64;
    ticks * tick_us
}