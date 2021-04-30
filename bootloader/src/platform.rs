use crate::KernelHeader;

#[cfg(target_arch="x86_64")]
mod platform {
    use super::*;

    pub fn goto_entrypoint(kernel_header: &KernelHeader, entry_point: u64) {
        // Jump to the given entry_point.
        // On x86_64, edi should contain the first argument of a function.
        // Since we don't expect the kernel to ever return, a simple jmp
        // should suffice.
        unsafe{asm!(
            "jmp rax",
            in("edi") kernel_header as *const KernelHeader,
            in("rax") entry_point
        )};
    }
}

pub use platform::*;
