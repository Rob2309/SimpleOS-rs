use crate::KernelHeader;

#[cfg(target_arch="x86_64")]
mod implementation {
    use common_structures::config;

    use super::*;

    pub fn goto_entrypoint(kernel_header: &KernelHeader, entry_point: u64, mut kernel_stack: *mut u8) -> ! {
        // since the kernel_stack argument will point to the beginning of the buffer,
        // but on x86_64 the stack grows downward, we have to add the size.
        kernel_stack = unsafe{kernel_stack.offset(config::KERNEL_STACK_SIZE as isize)};

        // Jump to the given entry_point.
        // On x86_64, edi should contain the first argument of a function.
        // Since we don't expect the kernel to ever return, a simple jmp
        // should suffice.
        unsafe{asm!(
            // set up stack pointer and stack base pointer
            "mov rbp, {0}",
            "mov rsp, {0}",
            // jump to the entry point
            "jmp {1}",
            in(reg) kernel_stack,
            in(reg) entry_point,
            in("edi") kernel_header as *const KernelHeader,
        )};

        panic!("Somehow failed to jump to the kernel");
    }
}

pub use implementation::*;
