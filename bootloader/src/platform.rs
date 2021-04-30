use crate::KernelHeader;

#[cfg(target_arch="x86_64")]
mod x86_64 {
    use super::*;

    pub fn goto_entrypoint(kernel_header: &KernelHeader, entry_point: u64) {
        unsafe{asm!(
            "jmp rax",
            in("edi") kernel_header as *const KernelHeader,
            in("rax") entry_point
        )};
    }
}

#[cfg(target_arch="x86_64")]
pub use x86_64::*;
