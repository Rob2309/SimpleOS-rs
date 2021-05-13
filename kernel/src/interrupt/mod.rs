
#[cfg(target_arch="x86_64")]
mod x86_64;
#[cfg(target_arch="x86_64")]
use x86_64::*;

pub fn init() {
    platform_init();
}
