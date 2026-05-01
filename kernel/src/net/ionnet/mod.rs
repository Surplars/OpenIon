use crate::platform::Platform;

pub mod socket;

// struct 

pub fn init<P: Platform>() {
    if let Some(_dev) = P::net_device() {
        crate::kinfo!("Initializing OpenIon network stack...");
        // 初始化你的栈
    }
}

pub fn poll() {
    // 你的协议栈的轮询或后续中断处理逻辑
}



