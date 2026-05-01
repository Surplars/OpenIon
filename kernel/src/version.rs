use crate::kinfo;

pub const OS_NAME: &str = "OpenIon";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_TIME: &str = env!("BUILD_TIMESTAMP");
pub const GIT_HASH: &str = env!("GIT_HASH");

pub fn banner() {
    kinfo!("{} v{} ({} {})", OS_NAME, VERSION, BUILD_TIME, GIT_HASH);
}
