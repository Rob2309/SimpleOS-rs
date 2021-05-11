#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

#![feature(maybe_uninit_extra)]
#![feature(asm)]

use core::fmt::Write;

use common_structures::KernelHeader;

#[macro_use]
mod terminal;
mod mutex;
mod memory;

/// The kernel entry point.
/// This function will be called by the bootloader after preparing the environment.
#[cfg_attr(not(test), no_mangle)]
extern "C" fn _start(kernel_header: *const KernelHeader) -> ! {
    main(kernel_header);
}

// Since this crate is not "no_main" while testing, rust-analyzer and similar tools will spit out
// false warnings that certain functions are "never used". By naming the below function "main", this
// does not seem to happen.

fn main(kernel_header: *const KernelHeader) -> ! {
    let kh = unsafe{&*kernel_header};

    memory::set_high_mem_base(kh.high_memory_base);

    terminal::init(kh);
    terminal::clear();
    log!("Starting kernel...");

    memory::init_phys_manager(kh);
    memory::init_virt_manager(&kh.paging_info);

    loop {}
}

/// Will be called by functions like panic!(), expect(), unwrap(), etc. when errors occur.
#[cfg_attr(not(test), panic_handler)]
pub fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    // Since we have no printing functionality yet, we just loop and cry :(
    loop {}
}
