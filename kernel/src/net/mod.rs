pub mod posix;

// 当开启 use_smoltcp 时，使用 smoltcp 模块
#[cfg(feature = "use_smoltcp")]
pub mod smoltcp;
#[cfg(feature = "use_smoltcp")]
pub use smoltcp::*; // 把方法暴露出来给外部统一调用

// 当开启 use_custom_net 时，使用 custom_impl 模块
#[cfg(feature = "use_ionnet")]
pub mod ionnet;
#[cfg(feature = "use_ionnet")]
pub use ionnet::*; 
