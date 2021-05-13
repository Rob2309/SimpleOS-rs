#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

#![feature(maybe_uninit_extra)]
#![feature(asm)]
#![feature(naked_functions)]

use common_structures::KernelHeader;

#[macro_use]
mod terminal;
mod mutex;
mod memory;
mod arch;
mod interrupt;

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
    info!("Kernel", "Starting kernel...");
    warning!("Test", "Warning");
    error!("Test", "Error");

    memory::init_phys_manager(kh);
    memory::init_virt_manager(&kh.paging_info);

    arch::init_platform();

    interrupt::init();

    loop {}
}

/// Will be called by functions like panic!(), expect(), unwrap(), etc. when errors occur.
#[cfg_attr(not(test), panic_handler)]
pub fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    // We just assume that we made it past the terminal initialization code.
    // Terminal initialization should theoretically be unfailable, let's hope.

    error!("===PANIC===", "{}", info);

    loop {}
}
